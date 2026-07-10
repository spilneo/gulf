use crate::{
    config::ProviderConfig,
    model::{ExecutionAction, ExecutionKind, ExecutionPlan, QuoteRequest, StatusHandle},
};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use super::super::{EndpointCall, ProviderRegistry};

// ---------------------------------------------------------------------------
// Meson chain CAIP-2 → Meson short name mapping
// ---------------------------------------------------------------------------

fn caip2_to_meson_chain(caip2: &str) -> Option<&'static str> {
    match caip2 {
        "eip155:8453" => Some("base"),
        "eip155:42161" => Some("arb"),
        "eip155:1" => Some("eth"),
        "eip155:10" => Some("op"),
        "eip155:137" => Some("matic"),
        "eip155:43114" => Some("avax"),
        "eip155:56" => Some("bsc"),
        "eip155:59144" => Some("linea"),
        "eip155:324" => Some("zksync"),
        "eip155:100" => Some("gnosis"),
        _ => None,
    }
}

/// Meson token short name from the Gulf token symbol/id.
fn token_to_meson_name(token_id: &str) -> Option<&'static str> {
    match token_id {
        "USDC" => Some("usdc"),
        "USDT" => Some("usdt"),
        "ETH" | "ethereum-eth" | "base-eth" | "arbitrum-eth" => Some("eth"),
        "WETH" | "base-weth" | "ethereum-weth" => Some("weth"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Meson API constants
// ---------------------------------------------------------------------------

const MESON_BASE_URL: &str = "https://relayer.meson.fi";
const MESON_CONTRACT: &str = "0x25aB3Efd52e6470681CE037cD546Dc60726948D3";

fn meson_amount(raw_amount: &str, decimals: u8) -> Result<String> {
    super::super::amount::amount_decimal(raw_amount, decimals)
        .context("Meson amount must be a non-negative integer in source token base units")
}

// ---------------------------------------------------------------------------
// Meson API request / response structures
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct MesonPriceRequest {
    from: String,
    to: String,
    amount: String,
    #[serde(rename = "fromAddress")]
    from_address: String,
    recipient: String,
}

#[derive(Debug, Deserialize)]
struct MesonPriceResponse {
    result: Option<MesonPriceResult>,
}

#[derive(Debug, Deserialize)]
struct MesonPriceResult {
    price: Option<String>,
    fee: Option<String>,
}

#[derive(Debug, Serialize)]
struct MesonSwapRequest {
    from: String,
    to: String,
    amount: String,
    #[serde(rename = "fromAddress")]
    from_address: String,
    recipient: String,
}

#[derive(Debug, Deserialize)]
struct MesonSwapResponse {
    result: Option<MesonSwapResult>,
}

#[derive(Debug, Deserialize)]
struct MesonSwapResult {
    tx: Option<MesonTx>,
    encoded: Option<String>,
    #[serde(rename = "swapId")]
    swap_id: Option<String>,
    #[serde(rename = "signingRequest")]
    signing_request: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MesonTx {
    to: Option<String>,
    data: Option<String>,
    value: Option<String>,
}

// ---------------------------------------------------------------------------
// ProviderRegistry extension — Meson
// ---------------------------------------------------------------------------

impl ProviderRegistry {
    /// Prepare a Meson swap (quote + tx).
    pub(crate) async fn prepare_meson(
        &self,
        provider: &ProviderConfig,
        quote: &QuoteRequest,
    ) -> Result<EndpointCall> {
        // ---- chain mapping ------------------------------------------------
        let chain_in = caip2_to_meson_chain(&quote.from_chain).context(format!(
            "unsupported Meson source chain: {}",
            quote.from_chain
        ))?;
        let chain_out = caip2_to_meson_chain(&quote.to_chain).context(format!(
            "unsupported Meson destination chain: {}",
            quote.to_chain
        ))?;

        // ---- token mapping ------------------------------------------------
        let token_in = token_to_meson_name(&quote.from_token)
            .or_else(|| {
                // Try to resolve from token config provider_symbols
                let chain = self.config.find_chain(&quote.from_chain)?;
                let token = self.config.find_token(&quote.from_token, &chain.caip2_id)?;
                token.provider_symbols.get("meson").map(|s| s.as_str())
            })
            .context(format!(
                "unsupported Meson source token: {}",
                quote.from_token
            ))?;

        let token_out = token_to_meson_name(&quote.to_token)
            .or_else(|| {
                let chain = self.config.find_chain(&quote.to_chain)?;
                let token = self.config.find_token(&quote.to_token, &chain.caip2_id)?;
                token.provider_symbols.get("meson").map(|s| s.as_str())
            })
            .context(format!(
                "unsupported Meson destination token: {}",
                quote.to_token
            ))?;

        let from_str = format!("{chain_in}:{token_in}");
        let to_str = format!("{chain_out}:{token_out}");

        let recipient = quote
            .recipient
            .clone()
            .unwrap_or_else(|| quote.sender.clone());

        // ---- resolve source token address ---------------------------------
        let from_chain_cfg = self
            .config
            .find_chain(&quote.from_chain)
            .context("source chain not found in config")?;
        let from_token_cfg = self
            .config
            .find_token(&quote.from_token, &from_chain_cfg.caip2_id)
            .context("source token not found in config")?;
        let token_address = from_token_cfg
            .address
            .clone()
            .context("source token address not found in config")?;
        let amount = meson_amount(&quote.amount, from_token_cfg.decimals)?;

        // ---- step 1: price (quote) ----------------------------------------
        let price_req = MesonPriceRequest {
            from: from_str.clone(),
            to: to_str.clone(),
            amount: amount.clone(),
            from_address: quote.sender.clone(),
            recipient: recipient.clone(),
        };

        let price_response: MesonPriceResponse = self
            .client
            .post(format!("{MESON_BASE_URL}/api/v1/price"))
            .json(&price_req)
            .send()
            .await
            .context("Meson price request failed")?
            .json()
            .await
            .context("failed to parse Meson price response")?;

        let price_data = price_response
            .result
            .context("Meson price response missing result field")?;

        // ---- step 2: swap -------------------------------------------------
        let swap_req = MesonSwapRequest {
            from: from_str.clone(),
            to: to_str.clone(),
            amount: amount.clone(),
            from_address: quote.sender.clone(),
            recipient: recipient.clone(),
        };

        let swap_http_response = self
            .client
            .post(format!("{MESON_BASE_URL}/api/v1/swap"))
            .json(&swap_req)
            .send()
            .await
            .context("Meson swap request failed")?;

        let swap_status = swap_http_response.status();
        let swap_body = swap_http_response
            .text()
            .await
            .context("failed to read Meson swap response body")?;

        // If the API returned a non-success status, try to extract structured error info.
        // Meson returns errors like { "error": { "code": 65, "message": "amount-over-balance",
        //   "data": { "balance": "...", "swapData": {...} } } }.
        // Do NOT treat errored swapData as executable — report as provider minimum/infrastructure.
        if !swap_status.is_success() {
            if let Ok(raw_json) = serde_json::from_str::<serde_json::Value>(&swap_body)
                && let Some(error_obj) = raw_json.get("error")
            {
                let code = error_obj.get("code").and_then(|v| v.as_u64()).unwrap_or(0);
                let message = error_obj
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                if let Some(error_data) = error_obj.get("data") {
                    let balance = error_data
                        .get("balance")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let has_swap_data = error_data.get("swapData").is_some();
                    if has_swap_data {
                        bail!(
                            "Meson swap error code={code} message={message}: amount exceeds \
                                 balance (available={balance}). The API returned swapData in the \
                                 error payload — this is not a valid executable swap. Treat this \
                                 as blocked_provider_minimum or blocked_provider_infrastructure."
                        );
                    }
                    bail!(
                        "Meson swap error code={code} message={message}: balance={balance} \
                             (provider infrastructure / minimum)"
                    );
                }
                bail!(
                    "Meson swap HTTP {status} error code={code} message={message}",
                    status = swap_status.as_u16()
                );
            }
            // Fallback: unknown error body
            let truncated = swap_body.chars().take(500).collect::<String>();
            bail!("Meson swap HTTP {}: {}", swap_status.as_u16(), truncated);
        }

        // Parse successful response
        let raw_json: serde_json::Value =
            serde_json::from_str(&swap_body).context("failed to parse Meson swap response")?;

        // If the body has an `error` key but HTTP was 200, treat as no-route/minimum
        if let Some(error_obj) = raw_json.get("error") {
            let code = error_obj.get("code").and_then(|v| v.as_u64()).unwrap_or(0);
            let message = error_obj
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            if let Some(error_data) = error_obj.get("data") {
                let balance = error_data
                    .get("balance")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");
                if error_data.get("swapData").is_some() {
                    bail!(
                        "Meson swap returned error code={code} message={message}: amount exceeds \
                         balance (available={balance}). swapData was present in the error payload \
                         — this is NOT a valid executable swap."
                    );
                }
                bail!("Meson swap returned error code={code} message={message}: balance={balance}");
            }
            bail!("Meson swap returned error code={code} message={message}");
        }

        let swap_response: MesonSwapResponse = serde_json::from_value(raw_json)
            .context("failed to parse Meson swap response struct")?;

        let swap_data = swap_response
            .result
            .context("Meson swap response missing result field")?;

        // ---- build tx(s) --------------------------------------------------
        let tx = swap_data
            .tx
            .as_ref()
            .context("Meson swap response missing tx field")?;
        let tx_to = tx.to.clone().context("Meson swap tx missing 'to' field")?;
        let tx_data = tx.data.clone().unwrap_or_else(|| "0x".to_string());
        let tx_value = tx.value.clone().unwrap_or_else(|| "0".to_string());

        // ---- status id ----------------------------------------------------
        let encoded_id = swap_data
            .encoded
            .clone()
            .or_else(|| swap_data.swap_id.clone())
            .unwrap_or_default();
        let status_endpoint = if encoded_id.is_empty() {
            None
        } else {
            Some(format!("{MESON_BASE_URL}/api/v1/swap/{encoded_id}"))
        };

        // ---- warnings -----------------------------------------------------
        let mut warnings =
            vec!["ensure the user holds enough native gas token on the source chain".to_string()];
        if swap_data.signing_request.is_some() {
            warnings.push(
                "Meson returned a signingRequest; the route may need off-chain signature \
                 submission instead of an on-chain tx"
                    .to_string(),
            );
        }

        // ---- approval action ----------------------------------------------
        let approval_amount = quote.amount.clone();
        let approval_calldata =
            super::super::execution::erc20_approve_calldata(MESON_CONTRACT, &approval_amount)
                .context("failed to encode ERC-20 approval calldata")?;

        let approval_action = ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type: Some("approval".to_string()),
            description: Some("Approve token for Meson bridge contract".to_string()),
            to: Some(token_address.clone()),
            data: Some(approval_calldata),
            value: Some("0".to_string()),
            token: Some(token_address.clone()),
            spender: Some(MESON_CONTRACT.to_string()),
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

        // ---- bridge action ------------------------------------------------
        let bridge_action = ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type: Some("bridge".to_string()),
            description: Some("Meson bridge transaction".to_string()),
            to: Some(tx_to),
            data: Some(tx_data),
            value: Some(tx_value),
            token: Some(token_address.clone()),
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

        // ---- execution plan -----------------------------------------------
        let plan = ExecutionPlan {
            kind: ExecutionKind::EvmApprovalThenTransaction,
            actions: vec![approval_action, bridge_action],
            status: Some(StatusHandle {
                provider_id: provider.id.clone(),
                status_endpoint,
                id: Some(encoded_id),
                tx_hash: None,
            }),
            expires_at: None,
            warnings,
        };

        // ---- raw metadata ------------------------------------------------
        let raw_metadata = serde_json::json!({
            "from": from_str,
            "to": to_str,
            "amount": quote.amount,
            "fromAddress": quote.sender,
            "recipient": recipient,
            "encoded": swap_data.encoded,
            "swapId": swap_data.swap_id,
            "tx": {
                "to": tx.to.clone(),
                "data": tx.data.clone(),
                "value": tx.value.clone(),
            },
            "quotePrice": price_data.price,
            "quoteFee": price_data.fee,
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

    // ---- chain mapping ----------------------------------------------------

    #[test]
    fn test_caip2_to_meson_chain_known() {
        assert_eq!(caip2_to_meson_chain("eip155:8453"), Some("base"));
        assert_eq!(caip2_to_meson_chain("eip155:42161"), Some("arb"));
        assert_eq!(caip2_to_meson_chain("eip155:1"), Some("eth"));
        assert_eq!(caip2_to_meson_chain("eip155:10"), Some("op"));
        assert_eq!(caip2_to_meson_chain("eip155:137"), Some("matic"));
        assert_eq!(caip2_to_meson_chain("eip155:43114"), Some("avax"));
        assert_eq!(caip2_to_meson_chain("eip155:56"), Some("bsc"));
        assert_eq!(caip2_to_meson_chain("eip155:59144"), Some("linea"));
        assert_eq!(caip2_to_meson_chain("eip155:324"), Some("zksync"));
        assert_eq!(caip2_to_meson_chain("eip155:100"), Some("gnosis"));
    }

    #[test]
    fn test_caip2_to_meson_chain_unknown() {
        assert_eq!(caip2_to_meson_chain("eip155:99999"), None);
        assert_eq!(
            caip2_to_meson_chain("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"),
            None
        );
        assert_eq!(caip2_to_meson_chain(""), None);
    }

    // ---- token mapping ----------------------------------------------------

    #[test]
    fn test_token_to_meson_name_known() {
        assert_eq!(token_to_meson_name("USDC"), Some("usdc"));
        assert_eq!(token_to_meson_name("USDT"), Some("usdt"));
        assert_eq!(token_to_meson_name("ethereum-eth"), Some("eth"));
        assert_eq!(token_to_meson_name("base-weth"), Some("weth"));
    }

    #[test]
    fn test_token_to_meson_name_unknown() {
        assert_eq!(token_to_meson_name("DAI"), None);
        assert_eq!(token_to_meson_name("WBTC"), None);
        assert_eq!(token_to_meson_name(""), None);
    }

    #[test]
    fn test_meson_amount_uses_human_decimal_units() {
        assert_eq!(meson_amount("1000000", 6).unwrap(), "1");
        assert_eq!(meson_amount("1234567", 6).unwrap(), "1.234567");
        assert!(meson_amount("1.0", 6).is_err());
    }

    // ---- provider identification ------------------------------------------

    #[test]
    fn test_is_meson_provider() {
        assert!(super::super::is_meson_provider("meson"));
        assert!(!super::super::is_meson_provider("across-swap-api"));
        assert!(!super::super::is_meson_provider("rhino-fi"));
        assert!(!super::super::is_meson_provider(""));
    }
}
