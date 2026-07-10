/// Squid Router v2 adapter.
///
/// POST https://v2.api.squidrouter.com/v2/route with x-integrator-id header
/// from GULF_SQUID_INTEGRATOR_ID.  The response includes
/// `route.transactionRequest` with the EVM tx action fields and `route.estimate`
/// with quote details.
///
/// Status polling via GET https://v2.api.squidrouter.com/v2/status?transactionId=...
///
/// The x-request-id response header is captured and included in the raw
/// metadata of the execution plan.
use crate::{
    config::ProviderConfig,
    model::{ExecutionAction, ExecutionKind, ExecutionPlan, QuoteRequest, StatusHandle},
};
use anyhow::{Context, Result, bail};
use serde::Deserialize;

use super::super::{EndpointCall, ProviderRegistry};

// ---------------------------------------------------------------------------
// Squid API constants
// ---------------------------------------------------------------------------

const SQUID_V2_BASE: &str = "https://v2.api.squidrouter.com/v2";
const GULF_SQUID_INTEGRATOR_ID: &str = "GULF_SQUID_INTEGRATOR_ID";

// ---------------------------------------------------------------------------
// Squid API request / response structures
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Serialize)]
struct SquidRouteRequest {
    #[serde(rename = "fromChain")]
    from_chain: String,
    #[serde(rename = "toChain")]
    to_chain: String,
    #[serde(rename = "fromToken")]
    from_token: String,
    #[serde(rename = "toToken")]
    to_token: String,
    #[serde(rename = "fromAmount")]
    from_amount: String,
    #[serde(rename = "fromAddress")]
    from_address: String,
    #[serde(rename = "toAddress")]
    to_address: String,
    slippage: f64,
    #[serde(rename = "quoteOnly")]
    quote_only: bool,
    #[serde(rename = "enableExpress")]
    #[serde(default)]
    enable_express: bool,
}

#[derive(Debug, Deserialize)]
struct SquidRouteResponse {
    route: Option<SquidRoute>,
}

#[derive(Debug, Deserialize)]
struct SquidRoute {
    #[serde(rename = "transactionRequest")]
    transaction_request: Option<SquidTransactionRequest>,
    estimate: Option<SquidEstimate>,
    #[serde(rename = "requestId")]
    request_id: Option<String>,
    #[serde(rename = "quoteId")]
    quote_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SquidTransactionRequest {
    #[serde(rename = "target")]
    target: Option<String>,
    data: Option<String>,
    value: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SquidEstimate {
    #[serde(rename = "toAmount")]
    to_amount: Option<String>,
    #[serde(rename = "estimatedRouteDuration")]
    estimated_route_duration: Option<u64>,
    #[serde(rename = "feeCosts")]
    fee_costs: Option<Vec<SquidFeeCost>>,
    #[serde(rename = "fromAmount")]
    from_amount: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SquidFeeCost {
    #[serde(rename = "amountUsd")]
    amount_usd: Option<String>,
}

/// TransactionRequest fields come from `route.transactionRequest`.
/// `target` is the contract address, `data` is the calldata, `value` is the ETH value.
fn extract_tx_fields(
    tx: &SquidTransactionRequest,
) -> (Option<String>, Option<String>, Option<String>) {
    let to_ = tx.target.clone();
    let data = tx.data.clone();
    let value = tx.value.clone().or_else(|| Some("0".to_string()));
    (to_, data, value)
}

// ---------------------------------------------------------------------------
// ProviderRegistry extension — Squid Router
// ---------------------------------------------------------------------------

impl ProviderRegistry {
    /// Prepare a Squid Router v2 swap.
    pub(crate) async fn prepare_squid(
        &self,
        provider: &ProviderConfig,
        quote: &QuoteRequest,
    ) -> Result<EndpointCall> {
        // ---- resolve chain numeric IDs ------------------------------------
        let from_chain_cfg = self
            .config
            .find_chain(&quote.from_chain)
            .context("from_chain not found in config")?;
        let to_chain_cfg = self
            .config
            .find_chain(&quote.to_chain)
            .context("to_chain not found in config")?;

        let from_chain_id_str = from_chain_cfg
            .numeric_chain_id
            .map(|id| id.to_string())
            .context("from_chain numeric_chain_id not configured")?;
        let to_chain_id_str = to_chain_cfg
            .numeric_chain_id
            .map(|id| id.to_string())
            .context("to_chain numeric_chain_id not configured")?;

        // ---- resolve token addresses ---------------------------------------
        let from_token_cfg = self
            .config
            .find_token(&quote.from_token, &from_chain_cfg.caip2_id)
            .context("source token not found in config")?;
        let to_token_cfg = self
            .config
            .find_token(&quote.to_token, &to_chain_cfg.caip2_id)
            .context("destination token not found in config")?;

        let from_token_addr = from_token_cfg
            .address
            .clone()
            .context("source token address not found in config")?;
        let to_token_addr = to_token_cfg
            .address
            .clone()
            .context("destination token address not found in config")?;

        // ---- get integrator ID from env ------------------------------------
        let integrator_id = std::env::var(GULF_SQUID_INTEGRATOR_ID)
            .ok()
            .filter(|v| !v.is_empty());

        let recipient = quote
            .recipient
            .clone()
            .unwrap_or_else(|| quote.sender.clone());

        // ---- build route request -------------------------------------------
        let route_req = SquidRouteRequest {
            from_chain: from_chain_id_str.clone(),
            to_chain: to_chain_id_str.clone(),
            from_token: from_token_addr.clone(),
            to_token: to_token_addr,
            from_amount: quote.amount.clone(),
            from_address: quote.sender.clone(),
            to_address: recipient.clone(),
            slippage: 0.5,
            quote_only: false,
            enable_express: false,
        };

        // ---- call the Squid v2 route endpoint ------------------------------
        let url = format!("{SQUID_V2_BASE}/route");
        let mut request = self
            .client
            .post(&url)
            .header("Accept", "application/json")
            .header("Content-Type", "application/json");

        if let Some(ref id) = integrator_id {
            request = request.header("x-integrator-id", id);
        }

        let http_response = request
            .json(&route_req)
            .send()
            .await
            .with_context(|| format!("Squid route request to {url} failed"))?;

        let http_status = http_response.status();
        let request_id = http_response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        let body = http_response
            .text()
            .await
            .context("failed to read Squid route response body")?;

        if !http_status.is_success() {
            let rid = request_id
                .as_deref()
                .map(|id| format!(" x-request-id={id}"))
                .unwrap_or_default();
            let truncated: String = body.chars().take(500).collect();
            bail!(
                "Squid route HTTP {}{}: {}",
                http_status.as_u16(),
                rid,
                truncated
            );
        }

        // ---- parse response ------------------------------------------------
        let raw_json: serde_json::Value =
            serde_json::from_str(&body).context("failed to parse Squid route response JSON")?;

        let route_resp: SquidRouteResponse = serde_json::from_value(raw_json.clone())
            .context("failed to parse Squid route response struct")?;

        let route = route_resp
            .route
            .context("Squid route response missing 'route' field")?;

        let tx_req = route
            .transaction_request
            .context("Squid route response missing 'transactionRequest' field")?;

        let (tx_to, tx_data, tx_value) = extract_tx_fields(&tx_req);

        let tx_to = tx_to.context("Squid transactionRequest missing target/to field")?;
        let tx_data = tx_data.unwrap_or_else(|| "0x".to_string());

        // ---- extract quote details -----------------------------------------
        let output_amount = route.estimate.as_ref().and_then(|e| e.to_amount.clone());

        let settlement_seconds = route
            .estimate
            .as_ref()
            .and_then(|e| e.estimated_route_duration);

        let fee_usd = route
            .estimate
            .as_ref()
            .and_then(|e| e.fee_costs.as_ref())
            .and_then(|fees| fees.first())
            .and_then(|fee| fee.amount_usd.clone());

        let from_amount = route.estimate.as_ref().and_then(|e| e.from_amount.clone());

        let quote_id = route.quote_id.clone();
        let req_id = route.request_id.clone().or(request_id.clone());

        // ---- build status handle -------------------------------------------
        let status_endpoint = Some(format!("{SQUID_V2_BASE}/status"));

        let status = StatusHandle {
            provider_id: provider.id.clone(),
            status_endpoint,
            id: quote_id.clone(),
            tx_hash: None,
        };

        // ---- approval action ------------------------------------------------
        // Squid requires approval of the token to the router's transactionRequest.target
        let approval_amount = quote.amount.clone();
        let approval_calldata =
            super::super::execution::erc20_approve_calldata(&tx_to, &approval_amount)
                .context("failed to encode ERC-20 approval calldata for Squid")?;

        let approval_action = ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type: Some("approval".to_string()),
            description: Some("Approve token for Squid Router".to_string()),
            to: Some(from_token_addr.clone()),
            data: Some(approval_calldata),
            value: Some("0".to_string()),
            token: Some(from_token_addr.clone()),
            spender: Some(tx_to.clone()),
            amount: Some(approval_amount),
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            typed_data: None,
            message: None,
            intent: None,
            submission: None,
            raw: serde_json::Value::Null,
        };

        // ---- bridge action -------------------------------------------------
        let bridge_action = ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type: Some("bridge".to_string()),
            description: Some("Squid Router bridge transaction".to_string()),
            to: Some(tx_to),
            data: Some(tx_data),
            value: tx_value,
            token: Some(from_token_addr),
            spender: None,
            amount: Some(quote.amount.clone()),
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            typed_data: None,
            message: None,
            intent: None,
            submission: None,
            raw: serde_json::Value::Null,
        };

        // ---- execution plan ------------------------------------------------
        let plan = ExecutionPlan {
            kind: ExecutionKind::EvmApprovalThenTransaction,
            actions: vec![approval_action, bridge_action],
            status: Some(status),
            expires_at: None,
            warnings: vec![
                "ensure the user holds enough native gas token on the source chain".to_string(),
            ],
        };

        // ---- raw metadata --------------------------------------------------
        let raw_metadata = serde_json::json!({
            "fromChain": from_chain_id_str,
            "toChain": to_chain_id_str,
            "fromToken": from_token_cfg.address,
            "toToken": to_token_cfg.address,
            "fromAmount": quote.amount,
            "fromAddress": quote.sender,
            "toAddress": recipient,
            "requestId": req_id,
            "quoteId": quote_id,
            "outputAmount": output_amount,
            "settlementSeconds": settlement_seconds,
            "feeUsd": fee_usd,
            "fromAmountEstimate": from_amount,
            "x_request_id": request_id,
        });

        Ok(EndpointCall {
            raw: raw_metadata,
            normalized_quote: None,
            normalized_transaction: None,
            normalized_execution_plan: Some(plan),
        })
    }
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tx_fields_with_target() {
        let tx = SquidTransactionRequest {
            target: Some("0xTarget".to_string()),
            data: Some("0xdeadbeef".to_string()),
            value: Some("100".to_string()),
        };
        let (to, data, value) = extract_tx_fields(&tx);
        assert_eq!(to.as_deref(), Some("0xTarget"));
        assert_eq!(data.as_deref(), Some("0xdeadbeef"));
        assert_eq!(value.as_deref(), Some("100"));
    }

    #[test]
    fn test_extract_tx_fields_default_value() {
        let tx = SquidTransactionRequest {
            target: Some("0xToTarget".to_string()),
            data: Some("0xbeef".to_string()),
            value: None,
        };
        let (to, data, value) = extract_tx_fields(&tx);
        assert_eq!(to.as_deref(), Some("0xToTarget"));
        assert_eq!(data.as_deref(), Some("0xbeef"));
        assert_eq!(value.as_deref(), Some("0"));
    }

    #[test]
    fn test_extract_tx_fields_all_none() {
        let tx = SquidTransactionRequest {
            target: None,
            data: None,
            value: None,
        };
        let (to, data, value) = extract_tx_fields(&tx);
        assert_eq!(to, None);
        assert_eq!(data, None);
        assert_eq!(value.as_deref(), Some("0"));
    }

    #[test]
    fn test_squid_integrator_id_env_name() {
        assert_eq!(GULF_SQUID_INTEGRATOR_ID, "GULF_SQUID_INTEGRATOR_ID");
    }
}
