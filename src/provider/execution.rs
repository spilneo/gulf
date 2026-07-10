use crate::{
    config::{GulfConfig, ProviderConfig, ResponseMappingConfig},
    model::{ExecutionAction, ExecutionKind, ExecutionPlan, QuoteRequest, StatusHandle},
};
use serde_json::Value;

use super::response::{json_path, string_value};

pub(super) fn approval_token_address(
    provider: &ProviderConfig,
    quote: &QuoteRequest,
    config: &GulfConfig,
) -> Option<String> {
    let chain = config.find_chain(&quote.from_chain)?;
    let token = config.find_token(&quote.from_token, &chain.caip2_id)?;
    let address = token.address.clone()?;
    if address.trim().is_empty() {
        return None;
    }
    if token.provider_symbols.contains_key(&provider.id) {
        token.provider_symbols.get(&provider.id).cloned()
    } else {
        Some(address)
    }
}

pub(super) fn erc20_approve_calldata(spender: &str, amount: &str) -> Option<String> {
    let spender = spender.strip_prefix("0x").unwrap_or(spender);
    if spender.len() != 40 || !spender.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let amount = amount.parse::<u128>().ok()?;
    Some(format!(
        "0x095ea7b3{spender:0>64}{amount:064x}",
        spender = spender.to_ascii_lowercase(),
        amount = amount
    ))
}

pub(super) fn find_first_evm_action(actions: &[ExecutionAction]) -> Option<&ExecutionAction> {
    actions.iter().find(|a| {
        a.to.is_some()
            && (a.data.is_some() || a.value.is_some())
            && a.action_type.as_deref().is_some_and(|t| t != "approval")
    })
}

pub(super) fn has_transaction(transaction: &crate::model::NormalizedTransaction) -> bool {
    transaction.to.is_some() || transaction.data.is_some() || transaction.value.is_some()
}

pub(super) fn normalize_execution_plan(
    mapping: &ResponseMappingConfig,
    raw: &serde_json::Value,
    provider: &ProviderConfig,
    quote: &QuoteRequest,
    config: &GulfConfig,
    approval_token_addr: Option<String>,
) -> ExecutionPlan {
    let tx_to = mapping
        .transaction_to
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let tx_data = mapping
        .transaction_data
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let tx_value = mapping
        .transaction_value
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);

    // Extract provider-level error message (e.g. deBridge ERROR_LOW_GIVE_AMOUNT)
    let response_error = mapping
        .error
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);

    let approval_to = mapping
        .approval_transaction_to
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let approval_data = mapping
        .approval_transaction_data
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let approval_value = mapping
        .approval_transaction_value
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);

    let deposit_addr = mapping
        .deposit_address
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let memo = mapping
        .memo
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let serialized_tx = mapping
        .serialized_transaction
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let top_level_typed_data = mapping
        .typed_data
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .cloned();
    let top_level_message = mapping
        .message
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let top_level_intent = mapping
        .intent
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .cloned();
    let top_level_submission = mapping
        .submission
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .cloned();
    let expires = mapping
        .expires_at
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let status_id = mapping
        .status_id
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let status_endpoint = mapping
        .status_endpoint
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let status_tx_hash = mapping
        .status_tx_hash
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);

    let erc20_spender = mapping
        .erc20_approval_spender
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value);
    let erc20_amount = mapping
        .erc20_approval_amount
        .as_deref()
        .and_then(|path| json_path(raw, path))
        .and_then(string_value)
        .or_else(|| erc20_spender.as_ref().map(|_| quote.amount.clone()));

    let configured_actions = build_configured_actions(mapping, raw, quote);

    let from_chain = config
        .find_chain(&quote.from_chain)
        .map(|c| c.vm.as_str())
        .unwrap_or("evm");

    let has_swap = tx_to.is_some() || tx_data.is_some() || tx_value.is_some();
    let has_dedicated_approval =
        approval_to.is_some() || approval_data.is_some() || approval_value.is_some();

    let has_erc20_approval =
        erc20_spender.is_some() && erc20_amount.is_some() && approval_token_addr.is_some();

    let swap_action = if has_swap {
        Some(build_swap_action(
            &tx_to,
            &tx_data,
            &tx_value,
            &quote.from_chain,
            raw,
        ))
    } else {
        None
    };

    let approval_action_from_dedicated = if has_dedicated_approval {
        Some(ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type: Some("approval".to_string()),
            description: Some("token approval".to_string()),
            to: approval_to,
            data: approval_data,
            value: approval_value.or_else(|| Some("0".to_string())),
            token: approval_token_addr.clone(),
            spender: None,
            amount: None,
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            typed_data: None,
            message: None,
            intent: None,
            submission: None,
            raw: raw.clone(),
        })
    } else {
        None
    };

    let approval_action_from_erc20 = if !has_dedicated_approval && has_erc20_approval {
        let spender = erc20_spender.as_deref().unwrap_or("");
        let amount = erc20_amount.as_deref().unwrap_or("");
        let token = approval_token_addr.clone().unwrap_or_default();
        erc20_approve_calldata(spender, amount).map(|data| ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type: Some("approval".to_string()),
            description: Some("ERC-20 token approval".to_string()),
            to: Some(token.clone()),
            data: Some(data),
            value: Some("0".to_string()),
            token: Some(token),
            spender: Some(spender.to_string()),
            amount: Some(amount.to_string()),
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            typed_data: None,
            message: None,
            intent: None,
            submission: None,
            raw: Value::Null,
        })
    } else {
        None
    };

    let approval_action = approval_action_from_dedicated.or(approval_action_from_erc20);

    let status = if status_id.is_some() || status_tx_hash.is_some() || status_endpoint.is_some() {
        Some(StatusHandle {
            provider_id: provider.id.clone(),
            status_endpoint,
            id: status_id,
            tx_hash: status_tx_hash,
        })
    } else {
        None
    };

    let mut warnings = Vec::new();

    // If the API returned a provider-level error in the body (e.g. deBridge
    // ERROR_LOW_GIVE_AMOUNT with HTTP 200), surface it as a warning so
    // downstream consumers can classify the failure.
    if let Some(ref err) = response_error
        && !err.is_empty()
    {
        warnings.push(format!("provider error in response: {err}", err = err));
    }

    if !configured_actions.is_empty() {
        let kind = kind_for_configured_actions(&configured_actions, from_chain);
        return ExecutionPlan {
            kind,
            actions: configured_actions,
            status,
            expires_at: expires,
            warnings,
        };
    }

    let top_level_action = if top_level_typed_data.is_some()
        || top_level_message.is_some()
        || top_level_intent.is_some()
        || top_level_submission.is_some()
    {
        let action_type = if top_level_typed_data.is_some() || top_level_message.is_some() {
            Some("sign".to_string())
        } else if top_level_intent.is_some() {
            Some("intent".to_string())
        } else {
            Some("submit".to_string())
        };
        Some(ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type,
            description: Some("provider signature/intent/submission action".to_string()),
            to: None,
            data: None,
            value: None,
            token: None,
            spender: None,
            amount: None,
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            typed_data: top_level_typed_data,
            message: top_level_message,
            intent: top_level_intent,
            submission: top_level_submission,
            raw: Value::Null,
        })
    } else {
        None
    };

    if let Some(ref action) = top_level_action {
        let top_kind = if (action.typed_data.is_some() || action.message.is_some()) && !has_swap {
            ExecutionKind::TypedSignatureRequest
        } else if action.intent.is_some() && !has_swap {
            ExecutionKind::IntentOrder
        } else {
            ExecutionKind::Unsupported
        };
        let mut top_actions = Vec::new();
        if let Some(approval) = approval_action {
            top_actions.push(approval);
        }
        if let Some(swap) = swap_action {
            top_actions.push(swap);
        }
        top_actions.push(action.clone());
        return ExecutionPlan {
            kind: forced_execution_kind(mapping).unwrap_or(top_kind),
            actions: top_actions,
            status,
            expires_at: expires,
            warnings,
        };
    }

    let kind = match (
        &swap_action,
        &approval_action,
        &deposit_addr,
        &serialized_tx,
    ) {
        (Some(_), Some(_), _, _) => ExecutionKind::EvmApprovalThenTransaction,
        (Some(_), None, _, _) => ExecutionKind::EvmTransaction,
        (None, Some(_), _, _) => {
            warnings.push(
                "approval is required — this response only contains a token approval action. \
                 Execute the approval transaction, then re-run /prepare to obtain the \
                 bridge or swap transaction."
                    .to_string(),
            );
            ExecutionKind::EvmApprovalOnly
        }
        (None, None, Some(_), _) if memo.is_some() => ExecutionKind::NativeMemoDeposit,
        (None, None, Some(_), _) => ExecutionKind::NativeDeposit,
        (None, None, None, Some(_)) => match from_chain {
            "solana" => ExecutionKind::SolanaTransaction,
            "tron" => ExecutionKind::TronTransaction,
            _ => ExecutionKind::SerializedTransaction,
        },
        (None, None, None, None) => {
            warnings.push("no executable transaction, approval, deposit address, or serialized transaction found".to_string());
            ExecutionKind::Unsupported
        }
    };

    let mut actions: Vec<ExecutionAction> = Vec::new();

    if let Some(action) = approval_action {
        actions.push(action);
    }
    if let Some(action) = swap_action {
        actions.push(action);
    }

    if matches!(
        kind,
        ExecutionKind::NativeDeposit
            | ExecutionKind::NativeMemoDeposit
            | ExecutionKind::CentralizedDeposit
    ) && let Some(ref addr) = deposit_addr
    {
        actions.push(ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type: Some("deposit".to_string()),
            description: Some("native deposit".to_string()),
            to: None,
            data: None,
            value: None,
            token: approval_token_addr.clone(),
            spender: None,
            amount: Some(quote.amount.clone()),
            deposit_address: Some(addr.clone()),
            memo: memo.clone(),
            serialized_transaction: None,
            typed_data: None,
            message: None,
            intent: None,
            submission: None,
            raw: Value::Null,
        });
    }

    if let Some(ref tx) = serialized_tx
        && matches!(
            kind,
            ExecutionKind::SolanaTransaction
                | ExecutionKind::TronTransaction
                | ExecutionKind::SerializedTransaction
                | ExecutionKind::MultiStep
        )
    {
        actions.push(ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type: Some("transaction".to_string()),
            description: Some("serialized transaction".to_string()),
            to: None,
            data: None,
            value: None,
            token: None,
            spender: None,
            amount: None,
            deposit_address: None,
            memo: None,
            serialized_transaction: Some(tx.clone()),
            typed_data: None,
            message: None,
            intent: None,
            submission: None,
            raw: Value::Null,
        });
    }

    ExecutionPlan {
        kind: forced_execution_kind(mapping).unwrap_or(kind),
        actions,
        status,
        expires_at: expires,
        warnings,
    }
}

fn forced_execution_kind(mapping: &ResponseMappingConfig) -> Option<ExecutionKind> {
    match mapping.execution_kind.as_deref() {
        Some("evm_transaction") => Some(ExecutionKind::EvmTransaction),
        Some("evm_approval_then_transaction") => Some(ExecutionKind::EvmApprovalThenTransaction),
        Some("multi_step") => Some(ExecutionKind::MultiStep),
        Some("native_deposit") => Some(ExecutionKind::NativeDeposit),
        Some("native_memo_deposit") => Some(ExecutionKind::NativeMemoDeposit),
        Some("solana_transaction") => Some(ExecutionKind::SolanaTransaction),
        Some("tron_transaction") => Some(ExecutionKind::TronTransaction),
        Some("intent_order") => Some(ExecutionKind::IntentOrder),
        Some("centralized_deposit") => Some(ExecutionKind::CentralizedDeposit),
        Some("evm_approval_only") => Some(ExecutionKind::EvmApprovalOnly),
        Some("serialized_transaction") => Some(ExecutionKind::SerializedTransaction),
        Some("typed_signature_request") => Some(ExecutionKind::TypedSignatureRequest),
        Some("unsupported") => Some(ExecutionKind::Unsupported),
        _ => None,
    }
}

fn build_configured_actions(
    mapping: &ResponseMappingConfig,
    raw: &serde_json::Value,
    quote: &QuoteRequest,
) -> Vec<ExecutionAction> {
    let Some(actions_path) = mapping.actions.as_deref() else {
        return vec![];
    };
    let Some(items) = json_path(raw, actions_path).and_then(|value| value.as_array()) else {
        return vec![];
    };

    items
        .iter()
        .filter_map(|item| {
            let action_type = mapped_string(item, &mapping.action_type)
                .or_else(|| infer_action_type(item))
                .or_else(|| Some("transaction".to_string()));
            let description = mapped_string(item, &mapping.action_description).or_else(|| {
                action_type
                    .as_deref()
                    .map(|action_type| format!("provider {action_type}"))
            });
            let action = ExecutionAction {
                chain: Some(quote.from_chain.clone()),
                action_type,
                description,
                to: mapped_string(item, &mapping.action_to),
                data: mapped_string(item, &mapping.action_data),
                value: mapped_string(item, &mapping.action_value),
                token: mapped_string(item, &mapping.action_token),
                spender: mapped_string(item, &mapping.action_spender),
                amount: mapped_string(item, &mapping.action_amount),
                deposit_address: mapped_string(item, &mapping.action_deposit_address),
                memo: mapped_string(item, &mapping.action_memo),
                serialized_transaction: mapped_string(item, &mapping.action_serialized_transaction),
                typed_data: mapping
                    .action_typed_data
                    .as_deref()
                    .and_then(|path| json_path(item, path))
                    .cloned(),
                message: mapped_string(item, &mapping.action_message),
                intent: mapping
                    .action_intent
                    .as_deref()
                    .and_then(|path| json_path(item, path))
                    .cloned(),
                submission: mapping
                    .action_submission
                    .as_deref()
                    .and_then(|path| json_path(item, path))
                    .cloned(),
                raw: item.clone(),
            };
            if action.to.is_some()
                || action.data.is_some()
                || action.value.is_some()
                || action.deposit_address.is_some()
                || action.serialized_transaction.is_some()
                || action.typed_data.is_some()
                || action.message.is_some()
                || action.intent.is_some()
                || action.submission.is_some()
            {
                Some(action)
            } else {
                None
            }
        })
        .collect()
}

fn mapped_string(item: &serde_json::Value, path: &Option<String>) -> Option<String> {
    path.as_deref()
        .and_then(|path| json_path(item, path))
        .and_then(string_value)
}

fn infer_action_type(item: &serde_json::Value) -> Option<String> {
    for key in ["action", "type", "kind", "name"] {
        if let Some(value) = item.get(key).and_then(string_value) {
            return Some(value.to_ascii_lowercase());
        }
    }
    None
}

fn kind_for_configured_actions(actions: &[ExecutionAction], from_chain_vm: &str) -> ExecutionKind {
    if actions.len() > 1 {
        if actions.len() == 2
            && actions[0]
                .action_type
                .as_deref()
                .is_some_and(|value| value.contains("approval") || value.contains("approve"))
            && actions[1].to.is_some()
        {
            return ExecutionKind::EvmApprovalThenTransaction;
        }
        return ExecutionKind::MultiStep;
    }

    let action = &actions[0];
    let has_executable = action.to.is_some()
        || action.data.is_some()
        || action.value.is_some()
        || action.deposit_address.is_some()
        || action.serialized_transaction.is_some();

    if action.deposit_address.is_some() && action.memo.is_some() {
        ExecutionKind::NativeMemoDeposit
    } else if action.deposit_address.is_some() {
        ExecutionKind::NativeDeposit
    } else if action.serialized_transaction.is_some() {
        match from_chain_vm {
            "solana" => ExecutionKind::SolanaTransaction,
            "tron" => ExecutionKind::TronTransaction,
            _ => ExecutionKind::SerializedTransaction,
        }
    } else if (action.typed_data.is_some() || action.message.is_some()) && !has_executable {
        ExecutionKind::TypedSignatureRequest
    } else if action.intent.is_some() && !has_executable {
        ExecutionKind::IntentOrder
    } else if action.to.is_some() || action.data.is_some() || action.value.is_some() {
        if action
            .action_type
            .as_deref()
            .is_some_and(|value| value.contains("approval") || value.contains("approve"))
        {
            ExecutionKind::EvmApprovalOnly
        } else {
            ExecutionKind::EvmTransaction
        }
    } else if action
        .action_type
        .as_deref()
        .is_some_and(|value| value.contains("approval") || value.contains("approve"))
    {
        ExecutionKind::EvmApprovalOnly
    } else {
        ExecutionKind::Unsupported
    }
}

fn build_swap_action(
    to: &Option<String>,
    data: &Option<String>,
    value: &Option<String>,
    chain: &str,
    raw: &serde_json::Value,
) -> ExecutionAction {
    ExecutionAction {
        chain: Some(chain.to_string()),
        action_type: Some("swap".to_string()),
        description: Some("token swap".to_string()),
        to: to.clone(),
        data: data.clone(),
        value: value.clone().or_else(|| Some("0".to_string())),
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
        raw: raw.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        CommissionConfig, FeeConfig, FeeUnit, GulfConfig, ProviderConfig, ServiceType,
    };
    use std::collections::BTreeMap;

    fn make_provider() -> ProviderConfig {
        ProviderConfig {
            id: "test_provider".to_string(),
            name: "Test Provider".to_string(),
            enabled: true,
            service_type: ServiceType::DexAggregator,
            supported_chains: vec![],
            supported_tokens: vec![],
            fee: FeeConfig {
                unit: FeeUnit::Bps,
                dynamic: false,
                estimated_bps: Some(10.0),
                flat_usd: None,
                settlement_seconds: None,
                notes: None,
            },
            commission: CommissionConfig {
                supported: false,
                strategy: "none".to_string(),
                default_bps: None,
                max_bps: None,
                parameters: BTreeMap::new(),
                notes: None,
            },
            docs: vec![],
            http: None,
            blockchain: None,
            notes: None,
        }
    }

    fn make_quote(from_token: &str) -> QuoteRequest {
        QuoteRequest {
            from_chain: "eip155:1".to_string(),
            to_chain: "eip155:1".to_string(),
            from_token: from_token.to_string(),
            to_token: "weth".to_string(),
            amount: "1000000".to_string(),
            sender: "0xSender".to_string(),
            recipient: None,
            live: false,
            partner_fee: None,
        }
    }

    fn make_config() -> GulfConfig {
        GulfConfig {
            project: None,
            developer: crate::config::DeveloperConfig {
                default_commission_bps: 10.0,
                recipients: BTreeMap::new(),
                b2b: Default::default(),
            },
            safety: None,
            routing: None,
            server: None,
            chains: vec![crate::config::ChainConfig {
                caip2_id: "eip155:1".to_string(),
                display_name: "Ethereum".to_string(),
                vm: "evm".to_string(),
                aliases: vec![],
                numeric_chain_id: Some(1),
                native_symbol: Some("ETH".to_string()),
                rpc_endpoints: vec![],
                provider_references: BTreeMap::new(),
                explorer_url: None,
            }],
            tokens: vec![crate::config::TokenConfig {
                id: "usdc".to_string(),
                chain_caip2_id: "eip155:1".to_string(),
                symbol: "USDC".to_string(),
                decimals: 6,
                address: Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string()),
                aliases: vec![],
                provider_symbols: BTreeMap::new(),
            }],
            providers: vec![],
        }
    }

    #[test]
    fn test_erc20_approve_calldata_usdc() {
        let data = erc20_approve_calldata("0x1111111254EEB25477B68fb85Ed929f73A960582", "1000000");
        assert!(data.is_some());
        let data = data.unwrap();
        assert!(data.starts_with("0x095ea7b3"));
        assert!(data.contains("1111111254eeb25477b68fb85ed929f73a960582"));
        assert!(data.contains("00000000000000000000000000000000000000000000000000000000000f4240"));
    }

    #[test]
    fn test_erc20_approve_calldata_invalid() {
        assert_eq!(erc20_approve_calldata("invalid", "100"), None);
        assert_eq!(erc20_approve_calldata("0xabc", "100"), None);
    }

    #[test]
    fn test_find_first_evm_action_prefers_swap() {
        let swap = ExecutionAction {
            chain: Some("eip155:1".into()),
            action_type: Some("swap".into()),
            description: Some("swap".into()),
            to: Some("0xSwap".into()),
            data: Some("0xdeadbeef".into()),
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
        };
        let approval = ExecutionAction {
            chain: Some("eip155:1".into()),
            action_type: Some("approval".into()),
            description: Some("approval".into()),
            to: Some("0xToken".into()),
            data: Some("0x095ea7b3".into()),
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
        };
        let actions = vec![approval.clone(), swap.clone()];
        let found = find_first_evm_action(&actions);
        assert!(found.is_some());
        assert_eq!(found.unwrap().action_type.as_deref(), Some("swap"));

        let actions = vec![swap.clone(), approval.clone()];
        let found = find_first_evm_action(&actions);
        assert_eq!(found.unwrap().action_type.as_deref(), Some("swap"));

        let actions = vec![approval.clone()];
        let found = find_first_evm_action(&actions);
        assert!(found.is_none());
    }

    #[test]
    fn test_normalize_execution_plan_evm_swap_only() {
        let mapping = ResponseMappingConfig {
            output_amount: None,
            estimated_fee_bps: None,
            estimated_fee_usd: None,
            gas_fee_usd: None,
            settlement_seconds: None,
            transaction_to: Some("$.tx.to".to_string()),
            transaction_data: Some("$.tx.data".to_string()),
            transaction_value: Some("$.tx.value".to_string()),
            erc20_approval_spender: None,
            erc20_approval_amount: None,
            error: None,
            approval_transaction_to: None,
            approval_transaction_data: None,
            approval_transaction_value: None,
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            expires_at: None,
            status_id: None,
            status_tx_hash: None,
            ..Default::default()
        };
        let raw = serde_json::json!({
            "tx": { "to": "0xSwap", "data": "0xdeadbeef", "value": "0" }
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::EvmTransaction));
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].to.as_deref(), Some("0xSwap"));
        assert_eq!(plan.actions[0].action_type.as_deref(), Some("swap"));
        assert!(plan.warnings.is_empty());
    }

    #[test]
    fn test_normalize_execution_plan_approval_and_swap() {
        let mapping = ResponseMappingConfig {
            output_amount: None,
            estimated_fee_bps: None,
            estimated_fee_usd: None,
            gas_fee_usd: None,
            settlement_seconds: None,
            transaction_to: Some("$.tx.to".to_string()),
            transaction_data: Some("$.tx.data".to_string()),
            transaction_value: Some("$.tx.value".to_string()),
            erc20_approval_spender: None,
            erc20_approval_amount: None,
            error: None,
            approval_transaction_to: Some("$.approval.to".to_string()),
            approval_transaction_data: Some("$.approval.data".to_string()),
            approval_transaction_value: Some("$.approval.value".to_string()),
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            expires_at: None,
            status_id: None,
            status_tx_hash: None,
            ..Default::default()
        };
        let raw = serde_json::json!({
            "tx": { "to": "0xSwap", "data": "0xdeadbeef", "value": "0" },
            "approval": { "to": "0xToken", "data": "0x095ea7b3", "value": "0" }
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(
            plan.kind,
            ExecutionKind::EvmApprovalThenTransaction
        ));
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(plan.actions[0].action_type.as_deref(), Some("approval"));
        assert_eq!(plan.actions[1].action_type.as_deref(), Some("swap"));
    }

    #[test]
    fn test_normalize_execution_plan_erc20_approval_only() {
        let mapping = ResponseMappingConfig {
            output_amount: None,
            estimated_fee_bps: None,
            estimated_fee_usd: None,
            gas_fee_usd: None,
            settlement_seconds: None,
            transaction_to: None,
            transaction_data: None,
            transaction_value: None,
            erc20_approval_spender: Some("$.spender".to_string()),
            erc20_approval_amount: Some("$.approvalAmount".to_string()),
            error: None,
            approval_transaction_to: None,
            approval_transaction_data: None,
            approval_transaction_value: None,
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            expires_at: None,
            status_id: None,
            status_tx_hash: None,
            ..Default::default()
        };
        let raw = serde_json::json!({
            "spender": "0x1111111254EEB25477B68fb85Ed929f73A960582",
            "approvalAmount": "1000000"
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string()),
        );
        assert!(matches!(plan.kind, ExecutionKind::EvmApprovalOnly));
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].action_type.as_deref(), Some("approval"));
        assert_eq!(
            plan.actions[0].to.as_deref(),
            Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")
        );
        assert!(
            plan.actions[0]
                .data
                .as_ref()
                .unwrap()
                .starts_with("0x095ea7b3")
        );
        assert_eq!(plan.warnings.len(), 1);
        assert!(
            plan.warnings
                .first()
                .unwrap()
                .contains("approval is required")
        );
    }

    #[test]
    fn test_approval_only_plan_kind_warning_normalized_fallback() {
        let mapping = ResponseMappingConfig {
            output_amount: None,
            estimated_fee_bps: None,
            estimated_fee_usd: None,
            gas_fee_usd: None,
            settlement_seconds: None,
            transaction_to: None,
            transaction_data: None,
            transaction_value: None,
            erc20_approval_spender: None,
            erc20_approval_amount: None,
            error: None,
            approval_transaction_to: Some("$.approval.to".to_string()),
            approval_transaction_data: Some("$.approval.data".to_string()),
            approval_transaction_value: Some("$.approval.value".to_string()),
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            expires_at: None,
            status_id: None,
            status_tx_hash: None,
            ..Default::default()
        };
        let raw = serde_json::json!({
            "approval": { "to": "0xToken", "data": "0x095ea7b3", "value": "0" }
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string()),
        );

        assert!(
            matches!(plan.kind, ExecutionKind::EvmApprovalOnly),
            "expected EvmApprovalOnly for approval-only, got {:?}",
            plan.kind
        );
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].action_type.as_deref(), Some("approval"));
        assert_eq!(plan.actions[0].to.as_deref(), Some("0xToken"));

        assert_eq!(plan.warnings.len(), 1);
        assert!(plan.warnings[0].contains("approval is required"));
        assert!(plan.warnings[0].contains("re-run"));

        let evm_action = find_first_evm_action(&plan.actions);
        assert!(
            evm_action.is_none(),
            "find_first_evm_action should return None for approval-only actions"
        );
    }

    #[test]
    fn test_normalize_execution_plan_deposit() {
        let mapping = ResponseMappingConfig {
            output_amount: None,
            estimated_fee_bps: None,
            estimated_fee_usd: None,
            gas_fee_usd: None,
            settlement_seconds: None,
            transaction_to: None,
            transaction_data: None,
            transaction_value: None,
            erc20_approval_spender: None,
            erc20_approval_amount: None,
            error: None,
            approval_transaction_to: None,
            approval_transaction_data: None,
            approval_transaction_value: None,
            deposit_address: Some("$.depositAddress".to_string()),
            memo: None,
            serialized_transaction: None,
            expires_at: Some("$.expiry".to_string()),
            status_id: Some("$.orderId".to_string()),
            status_tx_hash: None,
            ..Default::default()
        };
        let raw = serde_json::json!({
            "depositAddress": "bc1qdepositaddress",
            "expiry": "2026-12-31T23:59:59Z",
            "orderId": "order_123"
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::NativeDeposit));
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            plan.actions[0].deposit_address.as_deref(),
            Some("bc1qdepositaddress")
        );
        assert_eq!(plan.expires_at.as_deref(), Some("2026-12-31T23:59:59Z"));
        assert!(plan.status.is_some());
        assert_eq!(
            plan.status.as_ref().unwrap().id.as_deref(),
            Some("order_123")
        );
    }

    #[test]
    fn test_normalize_execution_plan_deposit_with_memo() {
        let mapping = ResponseMappingConfig {
            output_amount: None,
            estimated_fee_bps: None,
            estimated_fee_usd: None,
            gas_fee_usd: None,
            settlement_seconds: None,
            transaction_to: None,
            transaction_data: None,
            transaction_value: None,
            erc20_approval_spender: None,
            erc20_approval_amount: None,
            error: None,
            approval_transaction_to: None,
            approval_transaction_data: None,
            approval_transaction_value: None,
            deposit_address: Some("$.address".to_string()),
            memo: Some("$.memo".to_string()),
            serialized_transaction: None,
            expires_at: None,
            status_id: None,
            status_tx_hash: None,
            ..Default::default()
        };
        let raw = serde_json::json!({
            "address": "thor1inboundaddress",
            "memo": "SWAP:BTC.BTC:bc1dest:12345"
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::NativeMemoDeposit));
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(
            plan.actions[0].memo.as_deref(),
            Some("SWAP:BTC.BTC:bc1dest:12345")
        );
    }

    #[test]
    fn test_normalize_execution_plan_unsupported() {
        let mapping = ResponseMappingConfig {
            output_amount: None,
            estimated_fee_bps: None,
            estimated_fee_usd: None,
            gas_fee_usd: None,
            settlement_seconds: None,
            transaction_to: None,
            transaction_data: None,
            transaction_value: None,
            erc20_approval_spender: None,
            erc20_approval_amount: None,
            error: None,
            approval_transaction_to: None,
            approval_transaction_data: None,
            approval_transaction_value: None,
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            expires_at: None,
            status_id: None,
            status_tx_hash: None,
            ..Default::default()
        };
        let raw = serde_json::json!({"message": "nothing useful"});
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::Unsupported));
        assert!(plan.actions.is_empty());
        assert!(!plan.warnings.is_empty());
    }

    #[test]
    fn test_normalize_execution_plan_serialized_solana_tx() {
        let config = GulfConfig {
            chains: vec![crate::config::ChainConfig {
                caip2_id: "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp".to_string(),
                display_name: "Solana".to_string(),
                vm: "solana".to_string(),
                aliases: vec![],
                numeric_chain_id: None,
                native_symbol: Some("SOL".to_string()),
                rpc_endpoints: vec![],
                provider_references: BTreeMap::new(),
                explorer_url: None,
            }],
            ..make_config()
        };
        let quote = QuoteRequest {
            from_chain: "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp".to_string(),
            ..make_quote("usdc")
        };
        let mapping = ResponseMappingConfig {
            output_amount: None,
            estimated_fee_bps: None,
            estimated_fee_usd: None,
            gas_fee_usd: None,
            settlement_seconds: None,
            transaction_to: None,
            transaction_data: None,
            transaction_value: None,
            erc20_approval_spender: None,
            erc20_approval_amount: None,
            error: None,
            approval_transaction_to: None,
            approval_transaction_data: None,
            approval_transaction_value: None,
            deposit_address: None,
            memo: None,
            serialized_transaction: Some("$.tx".to_string()),
            expires_at: None,
            status_id: None,
            status_tx_hash: None,
            ..Default::default()
        };
        let raw = serde_json::json!({
            "tx": "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA..."
        });
        let plan =
            normalize_execution_plan(&mapping, &raw, &make_provider(), &quote, &config, None);
        assert!(matches!(plan.kind, ExecutionKind::SolanaTransaction));
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.actions[0].serialized_transaction.is_some());
    }

    #[test]
    fn test_normalize_execution_plan_serialized_tron_tx() {
        let config = GulfConfig {
            chains: vec![crate::config::ChainConfig {
                caip2_id: "tron:0x2b6653dc".to_string(),
                display_name: "Tron".to_string(),
                vm: "tron".to_string(),
                aliases: vec![],
                numeric_chain_id: None,
                native_symbol: Some("TRX".to_string()),
                rpc_endpoints: vec![],
                provider_references: BTreeMap::new(),
                explorer_url: None,
            }],
            ..make_config()
        };
        let quote = QuoteRequest {
            from_chain: "tron:0x2b6653dc".to_string(),
            ..make_quote("usdc")
        };
        let mapping = ResponseMappingConfig {
            output_amount: None,
            estimated_fee_bps: None,
            estimated_fee_usd: None,
            gas_fee_usd: None,
            settlement_seconds: None,
            transaction_to: None,
            transaction_data: None,
            transaction_value: None,
            erc20_approval_spender: None,
            erc20_approval_amount: None,
            error: None,
            approval_transaction_to: None,
            approval_transaction_data: None,
            approval_transaction_value: None,
            deposit_address: None,
            memo: None,
            serialized_transaction: Some("$.signedTx".to_string()),
            expires_at: None,
            status_id: None,
            status_tx_hash: None,
            ..Default::default()
        };
        let raw = serde_json::json!({
            "signedTx": "0a02..."
        });
        let plan =
            normalize_execution_plan(&mapping, &raw, &make_provider(), &quote, &config, None);
        assert!(matches!(plan.kind, ExecutionKind::TronTransaction));
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.actions[0].serialized_transaction.is_some());
    }

    #[test]
    fn test_normalize_execution_plan_serialized_unknown_vm_defaults_to_serialized_transaction() {
        let config = GulfConfig {
            chains: vec![crate::config::ChainConfig {
                caip2_id: "cosmos:cosmoshub-4".to_string(),
                display_name: "Cosmos".to_string(),
                vm: "cosmos".to_string(),
                aliases: vec![],
                numeric_chain_id: None,
                native_symbol: Some("ATOM".to_string()),
                rpc_endpoints: vec![],
                provider_references: BTreeMap::new(),
                explorer_url: None,
            }],
            ..make_config()
        };
        let quote = QuoteRequest {
            from_chain: "cosmos:cosmoshub-4".to_string(),
            ..make_quote("usdc")
        };
        let mapping = ResponseMappingConfig {
            output_amount: None,
            estimated_fee_bps: None,
            estimated_fee_usd: None,
            gas_fee_usd: None,
            settlement_seconds: None,
            transaction_to: None,
            transaction_data: None,
            transaction_value: None,
            erc20_approval_spender: None,
            erc20_approval_amount: None,
            error: None,
            approval_transaction_to: None,
            approval_transaction_data: None,
            approval_transaction_value: None,
            deposit_address: None,
            memo: None,
            serialized_transaction: Some("$.rawTx".to_string()),
            expires_at: None,
            status_id: None,
            status_tx_hash: None,
            ..Default::default()
        };
        let raw = serde_json::json!({
            "rawTx": "base64encoded..."
        });
        let plan =
            normalize_execution_plan(&mapping, &raw, &make_provider(), &quote, &config, None);
        assert!(matches!(plan.kind, ExecutionKind::SerializedTransaction));
    }

    #[test]
    fn test_normalize_execution_plan_with_status_handle() {
        let mapping = ResponseMappingConfig {
            output_amount: None,
            estimated_fee_bps: None,
            estimated_fee_usd: None,
            gas_fee_usd: None,
            settlement_seconds: None,
            transaction_to: Some("$.tx.to".to_string()),
            transaction_data: Some("$.tx.data".to_string()),
            transaction_value: Some("$.tx.value".to_string()),
            erc20_approval_spender: None,
            erc20_approval_amount: None,
            error: None,
            approval_transaction_to: None,
            approval_transaction_data: None,
            approval_transaction_value: None,
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            expires_at: None,
            status_id: Some("$.orderId".to_string()),
            status_tx_hash: Some("$.txHash".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "tx": { "to": "0xSwap", "data": "0xdeadbeef", "value": "0" },
            "orderId": "order_456",
            "txHash": "0xabcdef..."
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::EvmTransaction));
        let status = plan.status.unwrap();
        assert_eq!(status.id.as_deref(), Some("order_456"));
        assert_eq!(status.tx_hash.as_deref(), Some("0xabcdef..."));
        assert_eq!(status.provider_id, "test_provider");
    }

    #[test]
    fn test_normalize_execution_plan_configured_step_array() {
        let mapping = ResponseMappingConfig {
            output_amount: Some("$.result.details.destTokenAmount".to_string()),
            actions: Some("$.result.steps".to_string()),
            action_type: Some("action".to_string()),
            action_to: Some("tx.to".to_string()),
            action_data: Some("tx.data".to_string()),
            action_value: Some("tx.value".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "result": {
                "details": { "destTokenAmount": "990000" },
                "steps": [
                    { "action": "approve", "tx": { "to": "0xToken", "data": "0x095ea7b3", "value": "0" } },
                    { "action": "bridge", "tx": { "to": "0xRouter", "data": "0xabcdef", "value": "0" } }
                ]
            }
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(
            plan.kind,
            ExecutionKind::EvmApprovalThenTransaction
        ));
        assert_eq!(plan.actions.len(), 2);
        assert_eq!(plan.actions[0].action_type.as_deref(), Some("approve"));
        assert_eq!(plan.actions[1].to.as_deref(), Some("0xRouter"));
    }

    #[test]
    fn test_forced_execution_kind_evm_approval_only() {
        let mapping = ResponseMappingConfig {
            execution_kind: Some("evm_approval_only".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({});
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::EvmApprovalOnly));
    }

    #[test]
    fn test_forced_execution_kind_serialized_transaction() {
        let mapping = ResponseMappingConfig {
            execution_kind: Some("serialized_transaction".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({});
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::SerializedTransaction));
    }

    #[test]
    fn test_forced_execution_kind_typed_signature_request() {
        let mapping = ResponseMappingConfig {
            execution_kind: Some("typed_signature_request".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({});
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::TypedSignatureRequest));
    }

    #[test]
    fn test_configured_typed_data_action_produces_typed_signature_request() {
        let mapping = ResponseMappingConfig {
            actions: Some("$.steps".to_string()),
            action_type: Some("type".to_string()),
            action_typed_data: Some("signData".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "steps": [
                {
                    "type": "sign",
                    "signData": { "types": {}, "domain": {}, "message": {} }
                }
            ]
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::TypedSignatureRequest));
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.actions[0].typed_data.is_some());
        assert_eq!(plan.actions[0].action_type.as_deref(), Some("sign"));
    }

    #[test]
    fn test_configured_intent_action_produces_intent_order() {
        let mapping = ResponseMappingConfig {
            actions: Some("$.steps".to_string()),
            action_type: Some("type".to_string()),
            action_intent: Some("intent".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "steps": [
                {
                    "type": "intent",
                    "intent": { "action": "swap", "token": "USDC" }
                }
            ]
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::IntentOrder));
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.actions[0].intent.is_some());
        assert_eq!(plan.actions[0].action_type.as_deref(), Some("intent"));
    }

    #[test]
    fn test_configured_single_approval_action_produces_evm_approval_only() {
        let mapping = ResponseMappingConfig {
            actions: Some("$.steps".to_string()),
            action_type: Some("type".to_string()),
            action_to: Some("tx.to".to_string()),
            action_data: Some("tx.data".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "steps": [
                {
                    "type": "approve",
                    "tx": { "to": "0xToken", "data": "0x095ea7b3" }
                }
            ]
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::EvmApprovalOnly));
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].action_type.as_deref(), Some("approve"));
    }

    // ---------------------------------------------------------------------------
    // Top-level typed_data / message / intent / submission tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_top_level_typed_data_produces_typed_signature_request() {
        let mapping = ResponseMappingConfig {
            typed_data: Some("$.signingRequest".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "signingRequest": { "types": {}, "domain": {}, "message": {}, "primaryType": "Permit" }
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(
            matches!(plan.kind, ExecutionKind::TypedSignatureRequest),
            "expected TypedSignatureRequest, got {:?}",
            plan.kind
        );
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.actions[0].typed_data.is_some());
        assert_eq!(plan.actions[0].action_type.as_deref(), Some("sign"));
    }

    #[test]
    fn test_top_level_message_produces_typed_signature_request() {
        let mapping = ResponseMappingConfig {
            message: Some("$.signingRequest.message".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "signingRequest": { "message": "0xdeadbeef...", "hash": "0xabcdef..." }
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(
            matches!(plan.kind, ExecutionKind::TypedSignatureRequest),
            "expected TypedSignatureRequest, got {:?}",
            plan.kind
        );
        assert_eq!(plan.actions.len(), 1);
        assert_eq!(plan.actions[0].message.as_deref(), Some("0xdeadbeef..."));
    }

    #[test]
    fn test_top_level_intent_produces_intent_order() {
        let mapping = ResponseMappingConfig {
            intent: Some("$.intent".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "intent": { "action": "swap", "token": "USDC", "chain": "ethereum" }
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(
            matches!(plan.kind, ExecutionKind::IntentOrder),
            "expected IntentOrder, got {:?}",
            plan.kind
        );
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.actions[0].intent.is_some());
    }

    #[test]
    fn test_top_level_typed_data_with_submission() {
        let mapping = ResponseMappingConfig {
            typed_data: Some("$.signingRequest".to_string()),
            message: Some("$.signingRequest.message".to_string()),
            submission: Some("$.encoded".to_string()),
            status_id: Some("$.encoded".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "encoded": "a1b2c3",
            "signingRequest": { "message": "sign this", "hash": "0xabcdef" },
            "tx": { "to": "0xBridge", "data": "0xdeadbeef", "value": "0" }
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(
            matches!(plan.kind, ExecutionKind::TypedSignatureRequest),
            "expected TypedSignatureRequest, got {:?}",
            plan.kind
        );
        assert_eq!(plan.actions.len(), 1);
        assert!(plan.actions[0].typed_data.is_some());
        assert!(plan.actions[0].submission.is_some());
        assert_eq!(
            plan.actions[0].submission.as_ref().and_then(|s| s.as_str()),
            Some("a1b2c3")
        );
        assert!(plan.status.is_some());
        assert_eq!(plan.status.as_ref().unwrap().id.as_deref(), Some("a1b2c3"));
    }

    #[test]
    fn test_forced_execution_kind_intent_order() {
        let mapping = ResponseMappingConfig {
            execution_kind: Some("intent_order".to_string()),
            intent: Some("$.intent".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "intent": { "action": "swap" }
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            None,
        );
        assert!(matches!(plan.kind, ExecutionKind::IntentOrder));
        assert_eq!(plan.actions.len(), 1);
    }

    #[test]
    fn test_openocean_prepare_erc20_approval_and_swap() {
        // Simulates OpenOcean V4 /swap response with erc20_approval_spender= "data.to"
        // (the router address). Execution kind should be EvmApprovalThenTransaction.
        let mapping = ResponseMappingConfig {
            output_amount: Some("data.outAmount".to_string()),
            transaction_to: Some("data.to".to_string()),
            transaction_data: Some("data.data".to_string()),
            transaction_value: Some("data.value".to_string()),
            erc20_approval_spender: Some("data.to".to_string()),
            ..Default::default()
        };
        let raw = serde_json::json!({
            "data": {
                "to": "0x6352a56caadC4F1E25CD6c75970Fa768A3304e64",
                "data": "0x7c0252000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000f4240",
                "value": "0",
                "outAmount": "2450000000000000"
            }
        });
        let plan = normalize_execution_plan(
            &mapping,
            &raw,
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48".to_string()),
        );
        assert!(
            matches!(plan.kind, ExecutionKind::EvmApprovalThenTransaction),
            "expected EvmApprovalThenTransaction, got {:?}",
            plan.kind
        );
        assert_eq!(plan.actions.len(), 2);
        // First action: ERC20 approve
        assert_eq!(plan.actions[0].action_type.as_deref(), Some("approval"));
        assert_eq!(
            plan.actions[0].to.as_deref(),
            Some("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")
        );
        assert_eq!(
            plan.actions[0].spender.as_deref(),
            Some("0x6352a56caadC4F1E25CD6c75970Fa768A3304e64")
        );
        assert_eq!(plan.actions[0].amount.as_deref(), Some("1000000"));
        assert!(
            plan.actions[0]
                .data
                .as_ref()
                .unwrap()
                .starts_with("0x095ea7b3"),
            "approval data should be ERC20 approve selector"
        );
        // Second action: swap
        assert_eq!(plan.actions[1].action_type.as_deref(), Some("swap"));
        assert_eq!(
            plan.actions[1].to.as_deref(),
            Some("0x6352a56caadC4F1E25CD6c75970Fa768A3304e64")
        );
        assert!(plan.warnings.is_empty());
    }
}
