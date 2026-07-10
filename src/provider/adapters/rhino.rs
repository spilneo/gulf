use crate::{
    config::ProviderConfig,
    model::{ExecutionAction, ExecutionKind, ExecutionPlan, QuoteRequest, StatusHandle},
};
use alloy::primitives::{Address, U256};
use alloy::sol_types::SolCall;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

use super::super::{EndpointCall, ProviderRegistry};

// ---------------------------------------------------------------------------
// Solidity ABI for the Rhino bridge deposit function
// ---------------------------------------------------------------------------

alloy::sol! {
    #[allow(missing_docs)]
    function depositWithId(address token, uint256 amount, uint256 commitmentId);
}

// ---------------------------------------------------------------------------
// Rhino API response structures
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct RhinoAuthResponse {
    jwt: String,
}

#[derive(Debug, Deserialize)]
struct RhinoChainConfig {
    #[serde(rename = "contractAddress")]
    contract_address: Option<String>,
    /// Map keyed by token identifier (symbol / address); the Rhino API returns
    /// a JSON object, not an array.
    tokens: Option<std::collections::HashMap<String, RhinoTokenInfo>>,
}

#[derive(Debug, Deserialize)]
struct RhinoTokenInfo {
    #[serde(rename = "token")]
    symbol: String,
    address: Option<String>,
    decimals: Option<u8>,
}

#[derive(Debug, Serialize)]
struct RhinoQuoteRequest {
    #[serde(rename = "chainIn")]
    chain_in: String,
    #[serde(rename = "chainOut")]
    chain_out: String,
    amount: String,
    #[serde(rename = "tokenIn")]
    token_in: String,
    #[serde(rename = "tokenOut")]
    token_out: String,
    depositor: String,
    recipient: String,
    #[serde(rename = "amountNative")]
    amount_native: String,
    mode: String,
}

#[derive(Debug, Deserialize)]
struct RhinoQuoteResponse {
    #[serde(rename = "quoteId")]
    quote_id: String,
    #[serde(rename = "payAmount")]
    pay_amount: Option<String>,
    #[serde(rename = "receiveAmount")]
    receive_amount: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RhinoCommitResponse {
    #[serde(rename = "quoteId")]
    quote_id: String,
}

// ---------------------------------------------------------------------------
// Chain CAIP-2 → Rhino chain name mapping
// ---------------------------------------------------------------------------

fn caip2_to_rhino_chain(caip2: &str) -> Option<&'static str> {
    match caip2 {
        "eip155:8453" => Some("BASE"),
        "eip155:42161" => Some("ARBITRUM"),
        "eip155:1" => Some("ETHEREUM"),
        "eip155:10" => Some("OPTIMISM"),
        "eip155:137" => Some("POLYGON"),
        "eip155:324" => Some("ZKSYNC"),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Rhino API helpers
// ---------------------------------------------------------------------------

/// Authenticate with Rhino and return a JWT (valid for 1 hour).
async fn rhino_auth(client: &reqwest::Client, api_key: &str) -> Result<String> {
    let resp = client
        .post("https://api.rhino.fi/authentication/auth/apiKey")
        .json(&serde_json::json!({"apiKey": api_key}))
        .send()
        .await
        .context("Rhino auth request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Rhino auth failed: HTTP {status}: {body}");
    }

    let auth: RhinoAuthResponse = resp
        .json()
        .await
        .context("failed to parse Rhino auth response")?;

    Ok(auth.jwt)
}

/// Fetch public bridge configs (contract addresses / tokens) from Rhino.
async fn rhino_get_configs(
    client: &reqwest::Client,
) -> Result<std::collections::HashMap<String, RhinoChainConfig>> {
    let resp = client
        .get("https://api.rhino.fi/bridge/configs")
        .send()
        .await
        .context("Rhino configs request failed")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        bail!("Rhino configs failed: HTTP {status}: {body}");
    }

    let configs: std::collections::HashMap<String, RhinoChainConfig> = resp
        .json()
        .await
        .context("failed to parse Rhino configs response")?;

    Ok(configs)
}

/// Send an authenticated POST request to the Rhino API.
///
/// Tries `Authorization: <jwt>` first; on 401 retries with
/// `Authorization: Bearer <jwt>` as described in the integration guide.
async fn rhino_authenticated_post<T: serde::de::DeserializeOwned>(
    client: &reqwest::Client,
    jwt: &str,
    url: &str,
    body: Option<&serde_json::Value>,
) -> Result<T> {
    // First attempt — raw JWT in the Authorization header
    let resp = send_rhino_post(client, url, jwt, body)
        .await
        .context("Rhino authenticated request failed")?;

    if resp.status().is_success() {
        return resp
            .json()
            .await
            .context("failed to parse Rhino authenticated response");
    }

    // On 401, retry with "Bearer <jwt>"
    if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
        let bearer = format!("Bearer {jwt}");
        let resp = send_rhino_post(client, url, &bearer, body)
            .await
            .context("Rhino authenticated request failed (retry with Bearer)")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().await.unwrap_or_default();
            bail!("Rhino request failed: HTTP {status}: {body_text}");
        }

        return resp
            .json()
            .await
            .context("failed to parse Rhino authenticated response (Bearer retry)");
    }

    let status = resp.status();
    let body_text = resp.text().await.unwrap_or_default();
    bail!("Rhino request failed: HTTP {status}: {body_text}")
}

/// Low-level helper that builds and sends an authenticated POST request.
async fn send_rhino_post(
    client: &reqwest::Client,
    url: &str,
    auth_value: &str,
    body: Option<&serde_json::Value>,
) -> Result<reqwest::Response, reqwest::Error> {
    let mut req = client.post(url).header("Authorization", auth_value);
    if let Some(b) = body {
        req = req.json(b);
    }
    req.send().await
}

// ---------------------------------------------------------------------------
// Commitment-id parsing
// ---------------------------------------------------------------------------

/// Parse a commitment ID as hex if hex-like (starts with `0x` or contains
/// `a-f`/`A-F`), otherwise as a decimal string.  Returns the raw string on
/// success.
fn parse_commitment_id(id: &str) -> Result<String> {
    if id.trim().is_empty() {
        bail!("commitment ID is empty");
    }
    // Explicit 0x prefix → hex
    if let Some(hex) = id.strip_prefix("0x") {
        if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("commitment ID '{id}' has invalid hex characters after 0x");
        }
        return Ok(id.to_string());
    }
    // Contains a-f / A-F → hex without prefix
    if id.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F')) {
        if !id.chars().all(|c| c.is_ascii_hexdigit()) {
            bail!("commitment ID '{id}' is not valid hex");
        }
        return Ok(id.to_string());
    }
    // All-decimal → decimal
    if !id.chars().all(|c| c.is_ascii_digit()) {
        bail!("cannot parse commitment ID '{id}': not hex-like or decimal");
    }
    Ok(id.to_string())
}

/// Convert a parsed commitment-id string to `U256` for calldata encoding.
fn commitment_id_to_u256(id: &str) -> Result<U256> {
    if let Some(hex) = id.strip_prefix("0x") {
        U256::from_str_radix(hex, 16).context("failed to parse commitment ID as hex U256")
    } else if id.chars().any(|c| matches!(c, 'a'..='f' | 'A'..='F')) {
        U256::from_str_radix(id, 16).context("failed to parse commitment ID as hex U256")
    } else {
        U256::from_str_radix(id, 10).context("failed to parse commitment ID as decimal U256")
    }
}

// ---------------------------------------------------------------------------
// Amount helpers
// ---------------------------------------------------------------------------

/// Convert a Rhino `payAmount` to raw token units.
///
/// If the value contains a decimal point it is interpreted as a human-readable
/// decimal string and converted to the base unit using `decimals`.  Otherwise
/// it is assumed to already be raw and returned as-is.
fn pay_amount_to_raw(pay_amount: &str, decimals: u8) -> String {
    if !pay_amount.contains('.') {
        return pay_amount.to_string();
    }

    let parts: Vec<&str> = pay_amount.split('.').collect();
    let int_part = parts[0];
    let frac_part = if parts.len() > 1 { parts[1] } else { "" };
    let frac_padded = format!("{frac_part:0<width$}", width = decimals as usize);
    let combined = format!("{int_part}{frac_padded}");

    // Strip leading zeros (but keep at least one "0")
    let trimmed = combined.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Encode the `depositWithId(address,uint256,uint256)` calldata.
fn encode_deposit_with_id_calldata(
    token: &str,
    amount: &str,
    commitment_id: &str,
) -> Result<String> {
    let token_addr = Address::from_str(token)
        .with_context(|| format!("invalid token address for depositWithId: {token}"))?;

    let amount_val = amount
        .parse::<u128>()
        .map(U256::from)
        .with_context(|| format!("invalid amount for depositWithId: {amount}"))?;

    let commitment_val = commitment_id_to_u256(commitment_id)?;

    let call = depositWithIdCall {
        token: token_addr,
        amount: amount_val,
        commitmentId: commitment_val,
    };
    let encoded = call.abi_encode();
    let hex_str: String = encoded.iter().map(|b| format!("{b:02x}")).collect();
    Ok(format!("0x{hex_str}"))
}

// ---------------------------------------------------------------------------
// ProviderRegistry extension — Rhino prepare
// ---------------------------------------------------------------------------

impl ProviderRegistry {
    pub(crate) async fn prepare_rhino(
        &self,
        provider: &ProviderConfig,
        quote: &QuoteRequest,
    ) -> Result<EndpointCall> {
        // ---- gate: API key -------------------------------------------------
        let api_key = std::env::var("GULF_RHINO_API_KEY")
            .ok()
            .filter(|v| !v.trim().is_empty())
            .context("Rhino.fi prepare requires the GULF_RHINO_API_KEY environment variable")?;

        // ---- chain mapping ------------------------------------------------
        let chain_in = caip2_to_rhino_chain(&quote.from_chain).context(format!(
            "unsupported Rhino source chain: {}",
            quote.from_chain
        ))?;
        let chain_out = caip2_to_rhino_chain(&quote.to_chain).context(format!(
            "unsupported Rhino destination chain: {}",
            quote.to_chain
        ))?;

        // ---- token info ---------------------------------------------------
        let from_chain_cfg = self
            .config
            .find_chain(&quote.from_chain)
            .context("source chain not found in config")?;
        let from_token_cfg = self
            .config
            .find_token(&quote.from_token, &from_chain_cfg.caip2_id)
            .context("source token not found in config")?;

        // ---- authenticate -------------------------------------------------
        let jwt = rhino_auth(&self.client, &api_key)
            .await
            .context("Rhino authentication failed")?;

        // ---- fetch bridge configs (contract addresses + tokens) -----------
        let configs = rhino_get_configs(&self.client)
            .await
            .context("Rhino configs fetch failed")?;

        let source_config = configs
            .get(chain_in)
            .context(format!("no Rhino config for chain {chain_in}"))?;
        let source_contract = source_config.contract_address.as_deref().context(format!(
            "no contractAddress for chain {chain_in} in Rhino configs"
        ))?;

        // Look up the token in Rhino configs for both address and decimals.
        // tokens is a map — iterate over values to find by symbol.
        let rhino_token = source_config.tokens.as_ref().and_then(|tokens| {
            tokens
                .values()
                .find(|t| t.symbol.eq_ignore_ascii_case(&quote.from_token))
        });

        // Prefer the token address from Rhino configs; fall back to the
        // Gulf token registry.
        let token_address = rhino_token
            .and_then(|t| t.address.as_deref())
            .or(from_token_cfg.address.as_deref())
            .context("token address not found in Rhino configs or Gulf token registry")?;

        // Prefer the decimals from Rhino configs; fall back to the Gulf
        // token registry.
        let decimals = rhino_token
            .and_then(|t| t.decimals)
            .unwrap_or(from_token_cfg.decimals);

        // Convert the raw amount to a human decimal string for the Rhino API
        let amount_decimal = super::super::amount::amount_decimal(&quote.amount, decimals)
            .context("invalid source amount for Rhino")?;

        let recipient = quote
            .recipient
            .clone()
            .unwrap_or_else(|| quote.sender.clone());

        // ---- bridge quote -------------------------------------------------
        let quote_req = RhinoQuoteRequest {
            chain_in: chain_in.to_string(),
            chain_out: chain_out.to_string(),
            amount: amount_decimal,
            token_in: quote.from_token.clone(),
            token_out: quote.to_token.clone(),
            depositor: quote.sender.clone(),
            recipient: recipient.clone(),
            amount_native: "0".to_string(),
            mode: "pay".to_string(),
        };

        let quote_response: RhinoQuoteResponse = rhino_authenticated_post(
            &self.client,
            &jwt,
            "https://api.rhino.fi/bridge/quote/bridge-swap/user",
            Some(&serde_json::to_value(&quote_req)?),
        )
        .await
        .context("Rhino bridge quote request failed")?;

        // ---- commit -------------------------------------------------------
        let commit_url = format!(
            "https://api.rhino.fi/bridge/quote/commit/{}",
            quote_response.quote_id
        );
        let commit_response: RhinoCommitResponse = rhino_authenticated_post(
            &self.client,
            &jwt,
            &commit_url,
            None, // commit endpoint has no body
        )
        .await
        .context("Rhino commit request failed")?;

        // ---- determine deposit amount -------------------------------------
        let deposit_amount_raw = quote_response
            .pay_amount
            .as_deref()
            .map(|pa| pay_amount_to_raw(pa, decimals))
            .unwrap_or_else(|| quote.amount.clone());

        // ---- parse commitment id ------------------------------------------
        let commitment_id = parse_commitment_id(&commit_response.quote_id)
            .context("failed to parse commitment ID from Rhino commit response")?;

        // ---- encode calldata ----------------------------------------------
        let deposit_calldata =
            encode_deposit_with_id_calldata(token_address, &deposit_amount_raw, &commitment_id)
                .context("failed to encode depositWithId calldata")?;

        let approval_calldata =
            super::super::execution::erc20_approve_calldata(source_contract, &deposit_amount_raw)
                .context("failed to encode ERC-20 approval calldata")?;

        // ---- build execution plan -----------------------------------------
        let approval_action = ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type: Some("approval".to_string()),
            description: Some("Approve token for Rhino bridge contract".to_string()),
            to: Some(token_address.to_string()),
            data: Some(approval_calldata),
            value: Some("0".to_string()),
            token: Some(token_address.to_string()),
            spender: Some(source_contract.to_string()),
            amount: Some(deposit_amount_raw.clone()),
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            typed_data: None,
            message: None,
            intent: None,
            submission: None,
            raw: serde_json::Value::Null,
        };

        let deposit_action = ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type: Some("bridge".to_string()),
            description: Some("Rhino depositWithId bridge transaction".to_string()),
            to: Some(source_contract.to_string()),
            data: Some(deposit_calldata),
            value: Some("0".to_string()),
            token: Some(token_address.to_string()),
            spender: None,
            amount: Some(deposit_amount_raw.clone()),
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            typed_data: None,
            message: None,
            intent: None,
            submission: None,
            raw: serde_json::Value::Null,
        };

        let status_endpoint = format!(
            "https://api.rhino.fi/bridge/history/bridge/{}",
            quote_response.quote_id
        );

        let plan = ExecutionPlan {
            kind: ExecutionKind::EvmApprovalThenTransaction,
            actions: vec![approval_action, deposit_action],
            status: Some(StatusHandle {
                provider_id: provider.id.clone(),
                status_endpoint: Some(status_endpoint),
                id: Some(quote_response.quote_id.clone()),
                tx_hash: None,
            }),
            expires_at: None,
            warnings: vec![
                "ensure the user holds enough native gas token on the source chain".to_string(),
            ],
        };

        // ---- raw metadata (includes key fields required by the spec) ------
        let raw_metadata = serde_json::json!({
            "quoteId": quote_response.quote_id,
            "commitmentId": commit_response.quote_id,
            "chainIn": chain_in,
            "chainOut": chain_out,
            "payAmount": deposit_amount_raw,
            "receiveAmount": quote_response.receive_amount.unwrap_or_default(),
            "sourceContract": source_contract,
            "tokenAddress": token_address,
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
    fn test_caip2_to_rhino_chain_known() {
        assert_eq!(caip2_to_rhino_chain("eip155:8453"), Some("BASE"));
        assert_eq!(caip2_to_rhino_chain("eip155:42161"), Some("ARBITRUM"));
        assert_eq!(caip2_to_rhino_chain("eip155:1"), Some("ETHEREUM"));
        assert_eq!(caip2_to_rhino_chain("eip155:10"), Some("OPTIMISM"));
        assert_eq!(caip2_to_rhino_chain("eip155:137"), Some("POLYGON"));
        assert_eq!(caip2_to_rhino_chain("eip155:324"), Some("ZKSYNC"));
    }

    #[test]
    fn test_caip2_to_rhino_chain_unknown() {
        assert_eq!(caip2_to_rhino_chain("eip155:99999"), None);
        assert_eq!(
            caip2_to_rhino_chain("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"),
            None
        );
        assert_eq!(caip2_to_rhino_chain(""), None);
    }

    // ---- commitment-id parsing --------------------------------------------

    #[test]
    fn test_parse_commitment_id_hex_prefixed() {
        let result = parse_commitment_id("0xabcdef").unwrap();
        assert_eq!(result, "0xabcdef");
    }

    #[test]
    fn test_parse_commitment_id_hex_no_prefix() {
        let result = parse_commitment_id("deadbeef").unwrap();
        assert_eq!(result, "deadbeef");
    }

    #[test]
    fn test_parse_commitment_id_decimal() {
        let result = parse_commitment_id("12345").unwrap();
        assert_eq!(result, "12345");
    }

    #[test]
    fn test_parse_commitment_id_mixed_case_hex() {
        let result = parse_commitment_id("0xAbCdEf").unwrap();
        assert_eq!(result, "0xAbCdEf");
    }

    #[test]
    fn test_parse_commitment_id_empty_fails() {
        assert!(parse_commitment_id("").is_err());
        assert!(parse_commitment_id("   ").is_err());
    }

    #[test]
    fn test_parse_commitment_id_garbage_fails() {
        assert!(parse_commitment_id("abc!def").is_err());
        assert!(parse_commitment_id("0xGHIJ").is_err());
    }

    // ---- commitment-id → U256 ---------------------------------------------

    #[test]
    fn test_commitment_id_to_u256_hex_prefixed() {
        let val = commitment_id_to_u256("0x10").unwrap();
        assert_eq!(val, U256::from(16u64));

        let val = commitment_id_to_u256("0x1").unwrap();
        assert_eq!(val, U256::from(1u64));
    }

    #[test]
    fn test_commitment_id_to_u256_hex_no_prefix() {
        let val = commitment_id_to_u256("ff").unwrap();
        assert_eq!(val, U256::from(255u64));
    }

    #[test]
    fn test_commitment_id_to_u256_decimal() {
        let val = commitment_id_to_u256("42").unwrap();
        assert_eq!(val, U256::from(42u64));
    }

    #[test]
    fn test_commitment_id_to_u256_large_hex() {
        let val = commitment_id_to_u256(
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff",
        )
        .unwrap();
        assert_eq!(val, U256::MAX);
    }

    // ---- pay-amount conversion --------------------------------------------

    #[test]
    fn test_pay_amount_to_raw_no_decimal() {
        assert_eq!(pay_amount_to_raw("1000000", 6), "1000000");
        assert_eq!(pay_amount_to_raw("0", 6), "0");
    }

    #[test]
    fn test_pay_amount_to_raw_with_decimal() {
        // 1.0 USDC (6 decimals) → 1000000 raw
        assert_eq!(pay_amount_to_raw("1.0", 6), "1000000");
        // 1.5 USDC → 1500000 raw
        assert_eq!(pay_amount_to_raw("1.5", 6), "1500000");
        // 0.000042 USDC → 42 raw
        assert_eq!(pay_amount_to_raw("0.000042", 6), "42");
        // 100.0 USDC → 100000000 raw
        assert_eq!(pay_amount_to_raw("100.0", 6), "100000000");
    }

    #[test]
    fn test_pay_amount_to_raw_large_decimal() {
        // 1234.567890 USDC → 1234567890 raw
        assert_eq!(pay_amount_to_raw("1234.567890", 6), "1234567890");
    }

    // ---- depositWithId calldata -------------------------------------------

    #[test]
    fn test_deposit_with_id_calldata_selector() {
        let calldata = encode_deposit_with_id_calldata(
            "0x1111111254EEB25477B68fb85Ed929f73A960582",
            "1000000",
            "1",
        )
        .unwrap();
        assert!(calldata.starts_with("0x"), "calldata should start with 0x");
        let selector = &calldata[2..10];
        assert!(!selector.is_empty(), "selector must not be empty");
        assert!(
            selector.chars().all(|c| c.is_ascii_hexdigit()),
            "selector must be hex"
        );
    }

    #[test]
    fn test_deposit_with_id_calldata_contains_token() {
        let calldata = encode_deposit_with_id_calldata(
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            "1000000",
            "1",
        )
        .unwrap();
        // Token address should appear lowercased in the encoded data
        assert!(
            calldata.contains("a0b86991c6218b36c1d19d4a2e9eb0ce3606eb48"),
            "calldata must contain the token address: {calldata}"
        );
    }

    #[test]
    fn test_deposit_with_id_calldata_contains_amount() {
        let calldata = encode_deposit_with_id_calldata(
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            "1000000",
            "1",
        )
        .unwrap();
        // 1000000 = 0xf4240, padded to 32 bytes
        assert!(
            calldata.contains("00000000000000000000000000000000000000000000000000000000000f4240"),
            "calldata must contain the amount: {calldata}"
        );
    }

    #[test]
    fn test_deposit_with_id_calldata_contains_commitment_id_decimal() {
        let calldata = encode_deposit_with_id_calldata(
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            "1000000",
            "42",
        )
        .unwrap();
        // 42 = 0x2a, padded to 32 bytes
        assert!(
            calldata.ends_with("000000000000000000000000000000000000000000000000000000000000002a"),
            "calldata must end with commitment-id 42: {calldata}"
        );
    }

    #[test]
    fn test_deposit_with_id_calldata_contains_commitment_id_hex() {
        let calldata = encode_deposit_with_id_calldata(
            "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
            "1000000",
            "0xff",
        )
        .unwrap();
        assert!(
            calldata.ends_with("00000000000000000000000000000000000000000000000000000000000000ff"),
            "calldata must end with commitment-id 0xff: {calldata}"
        );
    }

    #[test]
    fn test_deposit_with_id_calldata_invalid_token() {
        assert!(encode_deposit_with_id_calldata("0xbad", "100", "1").is_err());
    }

    #[test]
    fn test_deposit_with_id_calldata_invalid_amount() {
        assert!(
            encode_deposit_with_id_calldata(
                "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
                "not_a_number",
                "1"
            )
            .is_err()
        );
    }

    #[test]
    fn test_is_rhino_fi_provider() {
        assert!(super::super::is_rhino_fi_provider("rhino-fi"));
        assert!(!super::super::is_rhino_fi_provider("across-swap-api"));
        assert!(!super::super::is_rhino_fi_provider(""));
    }
}
