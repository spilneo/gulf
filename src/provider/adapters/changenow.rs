use crate::{
    config::ProviderConfig,
    model::{ExecutionKind, ExecutionPlan, QuoteRequest},
};
use anyhow::{Context, Result, bail};

use super::super::{EndpointCall, ProviderRegistry};

/// Exported for safety-net unit testing without requiring network or env setup.
pub(super) fn validate_changenow_plan(plan: &ExecutionPlan) -> Result<()> {
    // Must be centralized_deposit
    if !matches!(plan.kind, ExecutionKind::CentralizedDeposit) {
        bail!(
            "ChangeNOW prepare must produce a centralized_deposit execution plan, got {:?}",
            plan.kind
        );
    }

    // Must have at least one action with a non-empty deposit_address
    plan.actions
        .iter()
        .find(|a| {
            a.deposit_address
                .as_deref()
                .filter(|addr| !addr.trim().is_empty())
                .is_some()
        })
        .ok_or_else(|| {
            anyhow::anyhow!("ChangeNOW prepare did not return a valid deposit address")
        })?;

    // Must have a non-empty status id (exchange id)
    let status = plan.status.as_ref();
    let has_status_id = status
        .and_then(|s| s.id.as_deref())
        .filter(|id| !id.trim().is_empty())
        .is_some();
    if !has_status_id {
        bail!("ChangeNOW prepare did not return an exchange id (status_id)");
    }

    Ok(())
}

impl ProviderRegistry {
    pub(crate) async fn prepare_changenow(
        &self,
        provider: &ProviderConfig,
        quote: &QuoteRequest,
    ) -> Result<EndpointCall> {
        // Gate: GULF_CHANGENOW_ENABLED must be exactly "true" (case-insensitive)
        let enabled = std::env::var("GULF_CHANGENOW_ENABLED")
            .ok()
            .filter(|v| v.trim().eq_ignore_ascii_case("true"))
            .is_some();
        if !enabled {
            bail!(
                "ChangeNOW prepare is gated: set GULF_CHANGENOW_ENABLED=true to enable the centralized exchange deposit flow"
            );
        }

        // Gate: GULF_CHANGENOW_API_KEY must be non-empty
        let _api_key = std::env::var("GULF_CHANGENOW_API_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .context(
                "ChangeNOW prepare requires a non-empty GULF_CHANGENOW_API_KEY environment variable",
            )?;

        // Call the configured prepare endpoint (POST /v2/exchange)
        let call = self
            .call_prepare(provider, quote)
            .await
            .context("ChangeNOW exchange create failed")?;

        // Validate the normalized execution plan
        let plan = call.normalized_execution_plan.as_ref().ok_or_else(|| {
            anyhow::anyhow!("ChangeNOW prepare did not produce an execution plan")
        })?;

        validate_changenow_plan(plan)?;

        // Build the execution plan with compliance and freshness warnings
        let mut warnings = vec![
            "CHANGENOW IS A CENTRALIZED EXCHANGE SERVICE: screen all compliance/ToS requirements before depositing funds."
                .to_string(),
            "Custodial deposit — you are sending funds to a ChangeNOW-controlled deposit address."
                .to_string(),
            "Re-run prepare immediately before sending funds; quotes and deposit addresses may expire."
                .to_string(),
            "Gulf does NOT submit or broadcast funds — you must send the deposit manually from your wallet."
                .to_string(),
            "Use only dust/insignificant amounts for testing until you have verified the full exchange lifecycle."
                .to_string(),
        ];
        for w in &plan.warnings {
            if !warnings.contains(w) {
                warnings.push(w.clone());
            }
        }

        let execution = ExecutionPlan {
            kind: ExecutionKind::CentralizedDeposit,
            actions: plan.actions.clone(),
            status: plan.status.clone(),
            expires_at: plan.expires_at.clone(),
            warnings,
        };

        // Suppress any EVM transaction from the standard response normalization
        Ok(EndpointCall {
            raw: call.raw.clone(),
            normalized_quote: None,
            normalized_transaction: None,
            normalized_execution_plan: Some(execution),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ExecutionAction, ExecutionKind, StatusHandle};

    fn make_deposit_plan(
        kind: ExecutionKind,
        deposit_addr: Option<&str>,
        status_id: Option<&str>,
    ) -> ExecutionPlan {
        ExecutionPlan {
            kind,
            actions: vec![ExecutionAction {
                chain: Some("eip155:1".to_string()),
                action_type: Some("deposit".to_string()),
                description: Some("test deposit".to_string()),
                to: None,
                data: None,
                value: None,
                token: None,
                spender: None,
                amount: Some("1000000".to_string()),
                deposit_address: deposit_addr.map(str::to_string),
                memo: None,
                serialized_transaction: None,
                typed_data: None,
                message: None,
                intent: None,
                submission: None,
                raw: serde_json::Value::Null,
            }],
            status: Some(StatusHandle {
                provider_id: "changenow".to_string(),
                status_endpoint: None,
                id: status_id.map(str::to_string),
                tx_hash: None,
            }),
            expires_at: None,
            warnings: vec![],
        }
    }

    #[test]
    fn test_validate_changenow_rejects_non_centralized() {
        let plan = make_deposit_plan(
            ExecutionKind::EvmTransaction,
            Some("0xDeposit"),
            Some("ex_123"),
        );
        let result = validate_changenow_plan(&plan);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("centralized_deposit"),
            "expected centralized_deposit error, got: {err}"
        );
    }

    #[test]
    fn test_validate_changenow_rejects_missing_deposit_address() {
        let plan = make_deposit_plan(ExecutionKind::CentralizedDeposit, None, Some("ex_123"));
        let result = validate_changenow_plan(&plan);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("deposit address"),
            "expected deposit address error, got: {err}"
        );
    }

    #[test]
    fn test_validate_changenow_rejects_empty_deposit_address() {
        let plan = make_deposit_plan(ExecutionKind::CentralizedDeposit, Some(""), Some("ex_123"));
        let result = validate_changenow_plan(&plan);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_changenow_rejects_missing_status_id() {
        let plan = make_deposit_plan(ExecutionKind::CentralizedDeposit, Some("0xDeposit"), None);
        let result = validate_changenow_plan(&plan);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("status_id"),
            "expected status_id error, got: {err}"
        );
    }

    #[test]
    fn test_validate_changenow_rejects_empty_status_id() {
        let plan = make_deposit_plan(
            ExecutionKind::CentralizedDeposit,
            Some("0xDeposit"),
            Some(""),
        );
        let result = validate_changenow_plan(&plan);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_changenow_accepts_valid_plan() {
        let plan = make_deposit_plan(
            ExecutionKind::CentralizedDeposit,
            Some("0xDepositAddr"),
            Some("ex_456"),
        );
        let result = validate_changenow_plan(&plan);
        assert!(result.is_ok(), "valid plan should pass: {:?}", result.err());
    }
}
