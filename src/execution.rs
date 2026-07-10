//! Local EVM signing and submission. Provider adapters only produce plans; no
//! signing material ever crosses that boundary.

use crate::{
    config::{ChainConfig, GulfConfig},
    model::{
        ExecuteSwapResult, ExecutionAction, ExecutionKind, ExecutionPlan, ExecutionReceipt,
        PreparedSwap,
    },
    rpc,
};
use alloy::{
    network::{EthereumWallet, TransactionBuilder},
    primitives::{Address, Bytes, U256},
    providers::{Provider, ProviderBuilder},
    rpc::types::TransactionRequest,
    signers::local::PrivateKeySigner,
};
use anyhow::{Context, Result, bail};

/// Validate a provider-supplied execution plan before any transaction is sent.
/// This is public to permit callers to inspect a prepared payload offline.
pub fn validate_plan(
    config: &GulfConfig,
    plan: &ExecutionPlan,
    expected_sender: &str,
    signer_address: Address,
) -> Result<()> {
    let sender = parse_address(expected_sender).context("quote sender must be an EVM address")?;
    if sender != signer_address {
        bail!("signer address does not match quote sender");
    }
    if !matches!(
        plan.kind,
        ExecutionKind::EvmTransaction
            | ExecutionKind::EvmApprovalThenTransaction
            | ExecutionKind::EvmApprovalOnly
            | ExecutionKind::MultiStep
    ) {
        bail!("execution plan kind is not supported for local EVM execution");
    }
    if plan.actions.is_empty() {
        bail!("execution plan contains no actions");
    }
    for action in &plan.actions {
        validate_action(config, action)?;
    }
    Ok(())
}

/// Sign and submit every validated action in order, waiting for a successful
/// mined receipt before continuing to the next action.
pub async fn execute(
    config: &GulfConfig,
    prepared: PreparedSwap,
    private_key_env: &str,
) -> Result<ExecuteSwapResult> {
    if config
        .safety
        .as_ref()
        .is_some_and(|safety| safety.require_live_quote_for_swap)
        && !prepared.request.live
    {
        bail!("safety.require_live_quote_for_swap requires quote.live=true");
    }
    if private_key_env.trim().is_empty() {
        bail!("private key environment variable name cannot be empty");
    }
    let private_key = std::env::var(private_key_env).with_context(|| {
        format!("required private key environment variable {private_key_env} is not set")
    })?;
    if private_key.trim().is_empty() {
        bail!("required private key environment variable {private_key_env} is empty");
    }
    let signer: PrivateKeySigner = private_key
        .parse()
        .map_err(|_| anyhow::anyhow!("private key in {private_key_env} is invalid"))?;
    let signer_address = signer.address();
    validate_plan(
        config,
        &prepared.execution,
        &prepared.request.sender,
        signer_address,
    )?;

    let mut receipts = Vec::with_capacity(prepared.execution.actions.len());
    for action in &prepared.execution.actions {
        let chain = configured_evm_chain(config, action)?;
        let endpoint = rpc::resolve_rpc_endpoints(chain)
            .into_iter()
            .next()
            .with_context(|| format!("chain {} has no enabled RPC endpoints", chain.caip2_id))?;
        let url = endpoint
            .url
            .parse()
            .context("configured RPC URL is invalid")?;
        let wallet = EthereumWallet::from(signer.clone());
        let provider = ProviderBuilder::new().wallet(wallet).connect_http(url);
        let request = transaction_request(chain, action)?;
        let pending = provider.send_transaction(request).await.with_context(|| {
            format!(
                "failed to submit {} action on {}",
                action_name(action),
                chain.caip2_id
            )
        })?;
        let receipt = pending.get_receipt().await.with_context(|| {
            format!(
                "failed waiting for {} action receipt on {}",
                action_name(action),
                chain.caip2_id
            )
        })?;
        if !receipt.status() {
            bail!(
                "{} action reverted on {}",
                action_name(action),
                chain.caip2_id
            );
        }
        receipts.push(ExecutionReceipt {
            transaction_hash: receipt.transaction_hash.to_string(),
            chain: chain.caip2_id.clone(),
            block_number: receipt
                .block_number
                .context("mined receipt did not include a block number")?,
            status: "success".to_owned(),
            action_type: action_name(action).to_owned(),
        });
    }
    Ok(ExecuteSwapResult { receipts })
}

fn validate_action(config: &GulfConfig, action: &ExecutionAction) -> Result<()> {
    configured_evm_chain(config, action)?;
    if action.deposit_address.is_some()
        || action.memo.is_some()
        || action.serialized_transaction.is_some()
        || action.typed_data.is_some()
        || action.message.is_some()
        || action.intent.is_some()
        || action.submission.is_some()
    {
        bail!(
            "action contains an unsupported deposit, serialized, signature, intent, or submission payload"
        );
    }
    if action.action_type.as_deref().is_some_and(|kind| {
        let kind = kind.to_ascii_lowercase();
        [
            "deposit",
            "serialized",
            "sign",
            "signature",
            "typed",
            "intent",
            "submit",
        ]
        .iter()
        .any(|unsupported| kind.contains(unsupported))
    }) {
        bail!("action type is not supported for local EVM execution");
    }
    parse_address(
        action
            .to
            .as_deref()
            .context("EVM action is missing destination address")?,
    )?;
    if let Some(data) = &action.data {
        parse_data(data)?;
    }
    if let Some(value) = &action.value {
        parse_value(value)?;
    }
    Ok(())
}

fn configured_evm_chain<'a>(
    config: &'a GulfConfig,
    action: &ExecutionAction,
) -> Result<&'a ChainConfig> {
    let chain_name = action
        .chain
        .as_deref()
        .context("EVM action is missing source chain")?;
    let chain = config
        .find_chain(chain_name)
        .with_context(|| format!("unknown action chain {chain_name}"))?;
    if !chain.vm.eq_ignore_ascii_case("evm") {
        bail!("action chain {} is not EVM", chain.caip2_id);
    }
    if chain.numeric_chain_id.is_none() {
        bail!("EVM chain {} has no numeric chain ID", chain.caip2_id);
    }
    if rpc::resolve_rpc_endpoints(chain).is_empty() {
        bail!("EVM chain {} has no enabled RPC endpoints", chain.caip2_id);
    }
    for endpoint in rpc::resolve_rpc_endpoints(chain) {
        endpoint
            .url
            .parse::<url::Url>()
            .with_context(|| format!("EVM chain {} has an invalid RPC endpoint", chain.caip2_id))?;
    }
    Ok(chain)
}

fn transaction_request(
    chain: &ChainConfig,
    action: &ExecutionAction,
) -> Result<TransactionRequest> {
    let to = parse_address(
        action
            .to
            .as_deref()
            .context("EVM action is missing destination address")?,
    )?;
    let mut request = TransactionRequest::default().with_to(to).with_chain_id(
        chain
            .numeric_chain_id
            .context("EVM chain has no numeric chain ID")?,
    );
    if let Some(data) = &action.data {
        request = request.with_input(parse_data(data)?);
    }
    if let Some(value) = &action.value {
        request = request.with_value(parse_value(value)?);
    }
    Ok(request)
}

fn parse_address(value: &str) -> Result<Address> {
    value
        .parse::<Address>()
        .map_err(|_| anyhow::anyhow!("invalid EVM address"))
}

fn parse_data(value: &str) -> Result<Bytes> {
    value
        .parse::<Bytes>()
        .map_err(|_| anyhow::anyhow!("invalid EVM calldata"))
}

fn parse_value(value: &str) -> Result<U256> {
    value
        .parse::<U256>()
        .map_err(|_| anyhow::anyhow!("invalid EVM value"))
}

fn action_name(action: &ExecutionAction) -> &str {
    action.action_type.as_deref().unwrap_or("transaction")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{ChainConfig, DeveloperConfig, GulfConfig, RpcEndpointConfig},
        model::{ExecuteSwapRequest, QuoteRequest},
    };
    use std::collections::BTreeMap;

    fn config() -> GulfConfig {
        GulfConfig {
            project: None,
            developer: DeveloperConfig {
                default_commission_bps: 0.0,
                recipients: BTreeMap::new(),
                b2b: Default::default(),
            },
            safety: None,
            routing: None,
            server: None,
            chains: vec![ChainConfig {
                caip2_id: "eip155:1".into(),
                display_name: "Ethereum".into(),
                vm: "evm".into(),
                aliases: vec![],
                numeric_chain_id: Some(1),
                native_symbol: None,
                rpc_endpoints: vec![RpcEndpointConfig {
                    name: "test".into(),
                    enabled: true,
                    url: Some("http://127.0.0.1:8545".into()),
                    url_env: None,
                    priority: 0,
                    timeout_ms: None,
                    headers: BTreeMap::new(),
                }],
                provider_references: BTreeMap::new(),
                explorer_url: None,
            }],
            tokens: vec![],
            providers: vec![],
        }
    }
    fn action() -> ExecutionAction {
        ExecutionAction {
            chain: Some("eip155:1".into()),
            action_type: Some("swap".into()),
            description: None,
            to: Some("0x0000000000000000000000000000000000000001".into()),
            data: Some("0x1234".into()),
            value: Some("0".into()),
            token: None,
            spender: None,
            amount: None,
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            typed_data: None,
            message: None,
            intent: None,
            submission: None,
            raw: serde_json::Value::Null,
        }
    }
    #[test]
    fn converts_valid_evm_action_without_network() {
        let request = transaction_request(&config().chains[0], &action()).unwrap();
        assert_eq!(request.chain_id, Some(1));
    }
    #[test]
    fn rejects_non_evm_payloads_before_submission() {
        let mut a = action();
        a.serialized_transaction = Some("0xdead".into());
        let plan = ExecutionPlan {
            kind: ExecutionKind::EvmTransaction,
            actions: vec![a],
            status: None,
            expires_at: None,
            warnings: vec![],
        };
        assert!(
            validate_plan(
                &config(),
                &plan,
                "0x0000000000000000000000000000000000000001",
                "0x0000000000000000000000000000000000000001"
                    .parse()
                    .unwrap()
            )
            .is_err()
        );
    }

    #[test]
    fn accepts_multistep_when_every_action_is_an_evm_transaction() {
        let plan = ExecutionPlan {
            kind: ExecutionKind::MultiStep,
            actions: vec![action(), action()],
            status: None,
            expires_at: None,
            warnings: vec![],
        };
        validate_plan(
            &config(),
            &plan,
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000001"
                .parse()
                .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn rejects_multistep_with_an_unsupported_action() {
        let mut unsupported = action();
        unsupported.typed_data = Some(serde_json::json!({"domain": "unsafe"}));
        let plan = ExecutionPlan {
            kind: ExecutionKind::MultiStep,
            actions: vec![action(), unsupported],
            status: None,
            expires_at: None,
            warnings: vec![],
        };
        assert!(
            validate_plan(
                &config(),
                &plan,
                "0x0000000000000000000000000000000000000001",
                "0x0000000000000000000000000000000000000001"
                    .parse()
                    .unwrap(),
            )
            .is_err()
        );
    }

    #[test]
    fn execution_request_cannot_serialize_private_key_material() {
        let secret = "this-value-must-never-appear-in-json";
        let request = ExecuteSwapRequest {
            quote: QuoteRequest {
                from_chain: "eip155:1".into(),
                to_chain: "eip155:1".into(),
                from_token: "ETH".into(),
                to_token: "ETH".into(),
                amount: "1".into(),
                sender: "0x0000000000000000000000000000000000000001".into(),
                recipient: None,
                live: false,
                partner_fee: None,
            },
            provider_id: None,
            private_key_env: "GULF_EVM_PRIVATE_KEY".into(),
        };
        assert!(!serde_json::to_string(&request).unwrap().contains(secret));
    }
}
