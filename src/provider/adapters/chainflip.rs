use crate::{
    config::ProviderConfig,
    model::{ExecutionAction, ExecutionKind, ExecutionPlan, QuoteRequest, StatusHandle},
};
use anyhow::{Context, Result, bail};

use super::super::{EndpointCall, ProviderRegistry, response::string_value};

impl ProviderRegistry {
    pub(crate) async fn prepare_chainflip(
        &self,
        provider: &ProviderConfig,
        quote: &QuoteRequest,
    ) -> Result<EndpointCall> {
        if std::env::var("GULF_CHAINFLIP_BROKER_API_URL")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .is_none()
        {
            bail!(
                "GULF_CHAINFLIP_BROKER_API_URL environment variable is required for Chainflip broker"
            );
        }

        let call = self
            .call_prepare(provider, quote)
            .await
            .context("Chainflip broker prepare failed")?;

        let raw = &call.raw;
        let result = raw.get("result");
        let deposit_addr = result
            .and_then(|r| r.get("address"))
            .and_then(string_value)
            .filter(|a| !a.trim().is_empty());
        let channel_id = result
            .and_then(|r| r.get("channel_id"))
            .and_then(string_value);
        let expires_at = result
            .and_then(|r| r.get("source_chain_expiry_block"))
            .and_then(string_value);

        if deposit_addr.is_none() {
            bail!("Chainflip broker did not return a valid deposit address");
        }

        let warnings = vec![
            "Deposit channel is fresh; do not reuse. No funds are broadcast from Gulf.".to_string(),
        ];

        let status = Some(StatusHandle {
            provider_id: "chainflip-broker".to_string(),
            status_endpoint: None,
            id: channel_id.clone(),
            tx_hash: None,
        });

        let execution = ExecutionPlan {
            kind: ExecutionKind::NativeDeposit,
            actions: vec![ExecutionAction {
                chain: Some(quote.from_chain.clone()),
                action_type: Some("deposit".to_string()),
                description: Some("Chainflip deposit channel".to_string()),
                to: None,
                data: None,
                value: None,
                token: None,
                spender: None,
                amount: Some(quote.amount.clone()),
                deposit_address: deposit_addr.clone(),
                memo: None,
                serialized_transaction: None,
                typed_data: None,
                message: None,
                intent: None,
                submission: None,
                raw: raw.clone(),
            }],
            status,
            expires_at,
            warnings,
        };

        Ok(EndpointCall {
            raw: raw.clone(),
            normalized_quote: None,
            normalized_transaction: None,
            normalized_execution_plan: Some(execution),
        })
    }
}
