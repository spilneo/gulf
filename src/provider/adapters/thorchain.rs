use crate::{
    config::ProviderConfig,
    model::{ExecutionAction, ExecutionKind, ExecutionPlan, QuoteRequest, StatusHandle},
};
use anyhow::{Context, Result, bail};

use super::super::{EndpointCall, ProviderRegistry, response::string_value};

impl ProviderRegistry {
    pub(crate) async fn prepare_thorchain(
        &self,
        provider: &ProviderConfig,
        quote: &QuoteRequest,
    ) -> Result<EndpointCall> {
        let call = self
            .call_prepare(provider, quote)
            .await
            .context("THORChain quote/swap prepare failed")?;

        let raw = &call.raw;
        let deposit_addr = raw
            .get("inbound_address")
            .and_then(string_value)
            .filter(|a| !a.trim().is_empty());
        let memo = raw
            .get("memo")
            .and_then(string_value)
            .filter(|m| !m.trim().is_empty());

        if deposit_addr.is_none() {
            bail!("THORChain prepare did not return a valid inbound_address");
        }
        if memo.is_none() {
            bail!("THORChain prepare did not return a valid memo");
        }

        let mut warnings = vec![
            "Use this deposit address and memo only while the quote is fresh; re-run prepare before sending funds."
                .to_string(),
        ];

        if let Some(w) = raw
            .get("warning")
            .and_then(string_value)
            .filter(|s| !s.trim().is_empty())
        {
            warnings.push(w);
        }
        if let Some(n) = raw
            .get("notes")
            .and_then(string_value)
            .filter(|s| !s.trim().is_empty())
        {
            warnings.push(n);
        }

        let expiry = raw
            .get("expiry")
            .and_then(|v| v.as_u64())
            .map(|e| e.to_string());

        let status_endpoint =
            "https://gateway.liquify.com/chain/thorchain_api/thorchain/tx/status/{tx_hash}"
                .to_string();
        let status = Some(StatusHandle {
            provider_id: "thorchain".to_string(),
            status_endpoint: Some(status_endpoint),
            id: None,
            tx_hash: None,
        });

        let execution = ExecutionPlan {
            kind: ExecutionKind::NativeMemoDeposit,
            actions: vec![ExecutionAction {
                chain: Some(quote.from_chain.clone()),
                action_type: Some("deposit".to_string()),
                description: Some("THORChain native deposit with memo".to_string()),
                to: None,
                data: None,
                value: None,
                token: None,
                spender: None,
                amount: Some(quote.amount.clone()),
                deposit_address: deposit_addr.clone(),
                memo: memo.clone(),
                serialized_transaction: None,
                typed_data: None,
                message: None,
                intent: None,
                submission: None,
                raw: raw.clone(),
            }],
            status,
            expires_at: expiry,
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
