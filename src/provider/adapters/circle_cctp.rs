use crate::{
    config::ProviderConfig,
    model::{ExecutionAction, ExecutionKind, ExecutionPlan, QuoteRequest, StatusHandle},
};
use alloy::primitives::{Address, B256, Bytes, U256};
use alloy::sol_types::SolCall;
use anyhow::{Context, Result, bail};
use std::str::FromStr;

use super::super::{EndpointCall, ProviderRegistry};

alloy::sol! {
    #[allow(missing_docs)]
    function depositForBurn(
        uint256 amount,
        uint32 destinationDomain,
        bytes32 mintRecipient,
        address burnToken,
        bytes32 destinationCaller,
        uint256 maxFee,
        uint32 minFinalityThreshold
    );

    #[allow(missing_docs)]
    function depositForBurnWithHook(
        uint256 amount,
        uint32 destinationDomain,
        bytes32 mintRecipient,
        address burnToken,
        bytes32 destinationCaller,
        uint256 maxFee,
        uint32 minFinalityThreshold,
        bytes hookData
    );
}

/// Hook data for Circle CCTP V2 Forwarding Service.
/// Format: "cctp-forward" (12 bytes) + version 0 (uint32) + additional-hook-data length 0 (uint32)
/// + zero-padding to the 32-byte static hook used by Circle examples.
///
/// See: <https://developers.circle.com/cctp/concepts/forwarding-service>
const CCTP_FORWARD_HOOK_DATA_HEX: &str =
    "0x636374702d666f72776172640000000000000000000000000000000000000000";

const CCTP_FORWARD_HOOK_DATA: [u8; 32] = [
    0x63, 0x63, 0x74, 0x70, 0x2d, 0x66, 0x6f, 0x72, 0x77, 0x61, 0x72, 0x64, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];

impl ProviderRegistry {
    pub(crate) async fn prepare_circle_cctp(
        &self,
        provider: &ProviderConfig,
        quote: &QuoteRequest,
    ) -> Result<EndpointCall> {
        let from_chain = self
            .config
            .find_chain(&quote.from_chain)
            .context("from_chain not found")?;
        let to_chain = self
            .config
            .find_chain(&quote.to_chain)
            .context("to_chain not found")?;

        let token_messenger = from_chain
            .provider_references
            .get("circle_token_messenger_v2")
            .context("circle_token_messenger_v2 not configured for from_chain")?
            .clone();
        let message_transmitter = from_chain
            .provider_references
            .get("circle_message_transmitter_v2")
            .context("circle_message_transmitter_v2 not configured for from_chain")?
            .clone();

        let source_domain_str = from_chain
            .provider_references
            .get("circle")
            .context("circle domain not configured for from_chain")?;
        let source_domain: u32 = source_domain_str
            .parse()
            .context("invalid circle source domain")?;

        let dest_domain_str = to_chain
            .provider_references
            .get("circle")
            .context("circle domain not configured for to_chain")?;
        let dest_domain: u32 = dest_domain_str
            .parse()
            .context("invalid circle destination domain")?;

        let burn_token_addr = self
            .config
            .find_token(&quote.from_token, &from_chain.caip2_id)
            .and_then(|t| t.address.clone())
            .context("source token address not found")?;

        let amount: U256 = quote
            .amount
            .parse::<u128>()
            .map(U256::from)
            .context("invalid amount")?;

        let recipient_addr = quote
            .recipient
            .clone()
            .unwrap_or_else(|| quote.sender.clone());
        let mint_recipient_str = recipient_to_bytes32(&recipient_addr)
            .context("invalid recipient address for bytes32 encoding")?;
        let mint_recipient =
            B256::from_str(&mint_recipient_str).context("invalid recipient bytes32")?;

        let (mode, min_finality_threshold, needs_fee_lookup, is_sandbox) =
            match provider.id.as_str() {
                "circle-cctp-standard" => ("standard", 2000u32, false, false),
                "circle-cctp-fast-v2" => ("fast", 1000u32, true, false),
                "circle-cctp-sandbox-fees" => ("sandbox", 1000u32, true, true),
                other => bail!("unknown Circle CCTP provider: {other}"),
            };

        let mut max_fee: U256;
        let mut forward_fee_raw: Option<u64> = None;
        let mut protocol_fee_raw: Option<u64> = None;

        if needs_fee_lookup {
            let fee_endpoint = self.endpoint(provider, "quote");
            let fee_call = self
                .call_endpoint(provider, fee_endpoint, quote)
                .await
                .context("failed to call Circle fee endpoint")?;
            let fee_rows = fee_call
                .raw
                .as_array()
                .context("Circle fee response is not a JSON array")?;

            let matching_row = fee_rows
                .iter()
                .find(|row| {
                    row.get("finalityThreshold")
                        .and_then(|v| v.as_u64())
                        .map(|t| t as u32 == min_finality_threshold)
                        .unwrap_or(false)
                })
                .with_context(|| {
                    format!(
                        "no Circle fee row found for finalityThreshold={min_finality_threshold}"
                    )
                })?;

            let minimum_fee_bps = matching_row
                .get("minimumFee")
                .and_then(crate::provider::response::string_value)
                .and_then(|s| s.parse::<u64>().ok())
                .context("minimumFee not found or invalid in Circle fee response")?;

            let protocol_fee = calculate_cctp_max_fee(&amount, minimum_fee_bps);
            max_fee = protocol_fee;

            if is_sandbox {
                let forward_fee_high_raw = matching_row
                    .get("forwardFee")
                    .and_then(|v| v.get("high"))
                    .and_then(crate::provider::response::string_value)
                    .and_then(|s| s.parse::<u64>().ok())
                    .context(
                        "forwardFee.high not found or invalid in Circle fee response (did you forget ?forward=true?)",
                    )?;
                let forward_fee = U256::from(forward_fee_high_raw);
                max_fee = max_fee
                    .checked_add(forward_fee)
                    .context("maxFee overflow when adding forward fee to protocol fee")?;
                forward_fee_raw = Some(forward_fee_high_raw);
                protocol_fee_raw = Some(protocol_fee.to::<u64>());
            }
        } else {
            max_fee = U256::ZERO;
        };

        let destination_caller = B256::ZERO;

        let burn_token =
            Address::from_str(&burn_token_addr).context("invalid burn token address")?;

        let deposit_calldata = if is_sandbox {
            let hook_data = Bytes::copy_from_slice(&CCTP_FORWARD_HOOK_DATA);
            encode_deposit_for_burn_with_hook_calldata(
                amount,
                dest_domain,
                mint_recipient,
                burn_token,
                destination_caller,
                max_fee,
                min_finality_threshold,
                hook_data,
            )
        } else {
            encode_deposit_for_burn_calldata(
                amount,
                dest_domain,
                mint_recipient,
                burn_token,
                destination_caller,
                max_fee,
                min_finality_threshold,
            )
        };

        let approval_action =
            super::super::execution::erc20_approve_calldata(&token_messenger, &quote.amount).map(
                |data| ExecutionAction {
                    chain: Some(quote.from_chain.clone()),
                    action_type: Some("approval".to_string()),
                    description: Some("Approve USDC for Circle CCTP V2 TokenMessenger".to_string()),
                    to: Some(burn_token_addr.clone()),
                    data: Some(data),
                    value: Some("0".to_string()),
                    token: Some(burn_token_addr.clone()),
                    spender: Some(token_messenger.clone()),
                    amount: Some(quote.amount.clone()),
                    deposit_address: None,
                    memo: None,
                    serialized_transaction: None,
                    typed_data: None,
                    message: None,
                    intent: None,
                    submission: None,
                    raw: serde_json::Value::Null,
                },
            );

        let burn_method_name = if is_sandbox {
            "depositForBurnWithHook"
        } else {
            "depositForBurn"
        };
        let forwarding_note = if is_sandbox {
            " (CCTP V2 Forwarding Service: destination gas not required)"
        } else {
            ""
        };

        let burn_action = ExecutionAction {
            chain: Some(quote.from_chain.clone()),
            action_type: Some("burn".to_string()),
            description: Some(format!(
                "Circle CCTP V2 {mode} {burn_method_name} from domain {source_domain} to domain {dest_domain}{forwarding_note}"
            )),
            to: Some(token_messenger.clone()),
            data: Some(deposit_calldata),
            value: Some("0".to_string()),
            token: Some(burn_token_addr.clone()),
            spender: None,
            amount: Some(quote.amount.clone()),
            deposit_address: None,
            memo: None,
            serialized_transaction: None,
            typed_data: None,
            message: None,
            intent: None,
            submission: None,
            raw: {
                let mut raw = serde_json::json!({
                    "sourceDomain": source_domain,
                    "destinationDomain": dest_domain,
                    "burnToken": burn_token_addr,
                    "mintRecipient": mint_recipient_str,
                    "destinationCaller": format!("{destination_caller:#x}"),
                    "maxFee": max_fee.to_string(),
                    "minFinalityThreshold": min_finality_threshold,
                    "tokenMessenger": token_messenger,
                    "messageTransmitter": message_transmitter,
                    "mode": mode,
                });
                if is_sandbox {
                    let obj = raw.as_object_mut().expect("just constructed above");
                    obj.insert("forwarding".to_string(), serde_json::Value::Bool(true));
                    obj.insert(
                        "hookData".to_string(),
                        serde_json::Value::String(CCTP_FORWARD_HOOK_DATA_HEX.to_string()),
                    );
                    if let Some(fwf) = forward_fee_raw {
                        obj.insert(
                            "forwardFeeRaw".to_string(),
                            serde_json::Value::String(fwf.to_string()),
                        );
                    }
                    if let Some(pf) = protocol_fee_raw {
                        obj.insert(
                            "protocolFeeRaw".to_string(),
                            serde_json::Value::String(pf.to_string()),
                        );
                    }
                }
                raw
            },
        };

        let mut actions: Vec<ExecutionAction> = Vec::new();
        let mut warnings: Vec<String> = Vec::new();

        if let Some(approval) = approval_action {
            warnings.push(
                "allowance is not checked; approval may be unnecessary if USDC is already approved for TokenMessengerV2. Do not broadcast or sign without verifying allowance first."
                    .to_string(),
            );
            actions.push(approval);
        }
        actions.push(burn_action);

        let iris_base = if provider.id == "circle-cctp-sandbox-fees" {
            std::env::var("GULF_CIRCLE_IRIS_SANDBOX_API_URL")
                .unwrap_or_else(|_| "https://iris-api-sandbox.circle.com".to_string())
        } else {
            std::env::var("GULF_CIRCLE_IRIS_API_URL")
                .unwrap_or_else(|_| "https://iris-api.circle.com".to_string())
        };
        let status_endpoint =
            format!("{iris_base}/v2/messages/{source_domain}?transactionHash={{tx_hash}}");

        let execution = ExecutionPlan {
            kind: ExecutionKind::MultiStep,
            actions,
            status: Some(StatusHandle {
                provider_id: provider.id.clone(),
                status_endpoint: Some(status_endpoint),
                id: None,
                tx_hash: None,
            }),
            expires_at: None,
            warnings,
        };

        Ok(EndpointCall {
            raw: serde_json::Value::Null,
            normalized_quote: None,
            normalized_transaction: None,
            normalized_execution_plan: Some(execution),
        })
    }
}

/// Left-pad a 20-byte EVM address to a 32-byte bytes32 value.
/// Strips the optional `0x` prefix; returns the hex-encoded 32-byte string.
pub(super) fn recipient_to_bytes32(address: &str) -> Option<String> {
    let hex = address.strip_prefix("0x").unwrap_or(address);
    if hex.len() != 40 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("0x000000000000000000000000{hex}"))
}

/// Calculate `maxFee` in USDC minor units for CCTP V2 depositForBurn.
///
/// `minimum_fee_bps` is the Circle fee in basis points (1 = 0.01%).
/// The buffer is 1.2x (12000/10000) to account for fee drift.
/// Result is at least 1 if `amount > 0` and `minimum_fee_bps > 0`.
pub(super) fn calculate_cctp_max_fee(amount: &U256, minimum_fee_bps: u64) -> U256 {
    if amount.is_zero() || minimum_fee_bps == 0 {
        return U256::ZERO;
    }
    const BUFFER_NUM: u64 = 12_000;
    const BUFFER_DEN: u64 = 10_000;
    let fee_bps_u128: u128 = minimum_fee_bps as u128;
    let amount_u128: u128 = amount.to::<u128>();
    let fee = (amount_u128
        .saturating_mul(fee_bps_u128)
        .saturating_add(9999))
        / 10_000u128;
    let buffered = (fee
        .saturating_mul(BUFFER_NUM as u128)
        .saturating_add(BUFFER_DEN as u128 - 1))
        / BUFFER_DEN as u128;
    U256::from(buffered.max(1))
}

/// ABI-encode the CCTP V2 `depositForBurn` call data.
fn encode_deposit_for_burn_calldata(
    amount: U256,
    destination_domain: u32,
    mint_recipient: B256,
    burn_token: Address,
    destination_caller: B256,
    max_fee: U256,
    min_finality_threshold: u32,
) -> String {
    let call = depositForBurnCall {
        amount,
        destinationDomain: destination_domain,
        mintRecipient: mint_recipient,
        burnToken: burn_token,
        destinationCaller: destination_caller,
        maxFee: max_fee,
        minFinalityThreshold: min_finality_threshold,
    };
    let encoded = call.abi_encode();
    let hex_str: String = encoded.iter().map(|b| format!("{b:02x}")).collect();
    format!("0x{hex_str}")
}

/// ABI-encode the CCTP V2 `depositForBurnWithHook` call data.
/// Used by the Circle Forwarding Service to relay the mint on the destination chain
/// without requiring the sender to hold destination gas.
#[allow(clippy::too_many_arguments)]
fn encode_deposit_for_burn_with_hook_calldata(
    amount: U256,
    destination_domain: u32,
    mint_recipient: B256,
    burn_token: Address,
    destination_caller: B256,
    max_fee: U256,
    min_finality_threshold: u32,
    hook_data: Bytes,
) -> String {
    let call = depositForBurnWithHookCall {
        amount,
        destinationDomain: destination_domain,
        mintRecipient: mint_recipient,
        burnToken: burn_token,
        destinationCaller: destination_caller,
        maxFee: max_fee,
        minFinalityThreshold: min_finality_threshold,
        hookData: hook_data,
    };
    let encoded = call.abi_encode();
    let hex_str: String = encoded.iter().map(|b| format!("{b:02x}")).collect();
    format!("0x{hex_str}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_recipient_to_bytes32_valid() {
        let result = recipient_to_bytes32("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
        assert_eq!(
            result.as_deref(),
            Some("0x000000000000000000000000d8dA6BF26964aF9D7eEd9e03E53415D37aA96045")
        );
    }

    #[test]
    fn test_recipient_to_bytes32_no_prefix() {
        let result = recipient_to_bytes32("d8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
        assert_eq!(
            result.as_deref(),
            Some("0x000000000000000000000000d8dA6BF26964aF9D7eEd9e03E53415D37aA96045")
        );
    }

    #[test]
    fn test_recipient_to_bytes32_invalid_short() {
        assert_eq!(recipient_to_bytes32("0x123"), None);
    }

    #[test]
    fn test_recipient_to_bytes32_invalid_chars() {
        assert_eq!(
            recipient_to_bytes32("0xZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZZ"),
            None
        );
    }

    #[test]
    fn test_is_circle_cctp_provider() {
        assert!(super::super::is_circle_cctp_provider(
            "circle-cctp-standard"
        ));
        assert!(super::super::is_circle_cctp_provider("circle-cctp-fast-v2"));
        assert!(super::super::is_circle_cctp_provider(
            "circle-cctp-sandbox-fees"
        ));
        assert!(!super::super::is_circle_cctp_provider("across-swap-api"));
        assert!(!super::super::is_circle_cctp_provider(""));
    }

    #[test]
    fn test_calculate_cctp_max_fee_zero_amount() {
        let amount = U256::ZERO;
        assert_eq!(calculate_cctp_max_fee(&amount, 5), U256::ZERO);
    }

    #[test]
    fn test_calculate_cctp_max_fee_zero_bps() {
        let amount = U256::from(1000000u64);
        assert_eq!(calculate_cctp_max_fee(&amount, 0), U256::ZERO);
    }

    #[test]
    fn test_calculate_cctp_max_fee_basic() {
        let amount = U256::from(1_000_000u64);
        let fee = calculate_cctp_max_fee(&amount, 1);
        assert_eq!(fee, U256::from(120u64));
    }

    #[test]
    fn test_calculate_cctp_max_fee_rounds_up() {
        let amount = U256::from(999u64);
        let fee = calculate_cctp_max_fee(&amount, 1);
        assert_eq!(fee, U256::from(2u64));
    }

    #[test]
    fn test_calculate_cctp_max_fee_min_one() {
        let amount = U256::from(1u64);
        let fee = calculate_cctp_max_fee(&amount, 1);
        assert_eq!(fee, U256::from(2u64));
    }

    #[test]
    fn test_deposit_for_burn_calldata_selector() {
        let calldata = encode_deposit_for_burn_calldata(
            U256::from(1000000u64),
            6u32,
            B256::from_str("0x000000000000000000000000d8dA6BF26964aF9D7eEd9e03E53415D37aA96045")
                .unwrap(),
            Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap(),
            B256::ZERO,
            U256::ZERO,
            2000u32,
        );
        assert!(calldata.starts_with("0x"), "calldata should start with 0x");
        assert_eq!(calldata.len(), 2 + 8 + 64 * 7);
        let selector = &calldata[2..10];
        assert!(!selector.is_empty(), "selector must not be empty");
        assert!(
            selector.chars().all(|c| c.is_ascii_hexdigit()),
            "selector must be hex"
        );
    }

    #[test]
    fn test_deposit_for_burn_calldata_contains_dest_domain() {
        let calldata = encode_deposit_for_burn_calldata(
            U256::from(1000000u64),
            6u32,
            B256::ZERO,
            Address::ZERO,
            B256::ZERO,
            U256::ZERO,
            2000u32,
        );
        assert!(
            calldata.contains("0000000000000000000000000000000000000000000000000000000000000006")
        );
    }

    #[test]
    fn test_deposit_for_burn_calldata_contains_amount() {
        let amount = U256::from(1_000_000u64);
        let calldata = encode_deposit_for_burn_calldata(
            amount,
            6u32,
            B256::ZERO,
            Address::ZERO,
            B256::ZERO,
            U256::ZERO,
            2000u32,
        );
        assert!(
            calldata.contains("00000000000000000000000000000000000000000000000000000000000f4240")
        );
    }

    // ---------------------------------------------------------------------------
    // depositForBurnWithHook calldata tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_hook_data_constant_matches_expected_hex() {
        // Verify the byte array matches the hex string constant
        let expected_hex = "636374702d666f72776172640000000000000000000000000000000000000000";
        let hex_from_bytes: String = CCTP_FORWARD_HOOK_DATA
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hex_from_bytes, expected_hex);
    }

    #[test]
    fn test_hook_data_has_correct_prefix() {
        // "cctp-forward" in hex
        assert_eq!(&CCTP_FORWARD_HOOK_DATA[0..12], b"cctp-forward");
        // Version 0
        assert_eq!(CCTP_FORWARD_HOOK_DATA[12], 0x00);
        // Length 0 (2 bytes)
        assert_eq!(CCTP_FORWARD_HOOK_DATA[13], 0x00);
        assert_eq!(CCTP_FORWARD_HOOK_DATA[14], 0x00);
    }

    #[test]
    fn test_deposit_for_burn_with_hook_calldata_selector() {
        let hook_data = Bytes::copy_from_slice(&CCTP_FORWARD_HOOK_DATA);
        let calldata = encode_deposit_for_burn_with_hook_calldata(
            U256::from(1000000u64),
            6u32,
            B256::from_str("0x000000000000000000000000d8dA6BF26964aF9D7eEd9e03E53415D37aA96045")
                .unwrap(),
            Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap(),
            B256::ZERO,
            U256::from(100u64),
            1000u32,
            hook_data,
        );
        assert!(
            calldata.starts_with("0x"),
            "hook calldata should start with 0x"
        );
        let selector = &calldata[2..10];
        assert!(!selector.is_empty(), "selector must not be empty");
        assert!(
            selector.chars().all(|c| c.is_ascii_hexdigit()),
            "selector must be hex"
        );
        // The selector for depositForBurnWithHook should differ from depositForBurn
        let standard_selector = {
            let std_calldata = encode_deposit_for_burn_calldata(
                U256::from(1000000u64),
                6u32,
                B256::from_str(
                    "0x000000000000000000000000d8dA6BF26964aF9D7eEd9e03E53415D37aA96045",
                )
                .unwrap(),
                Address::from_str("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48").unwrap(),
                B256::ZERO,
                U256::from(100u64),
                1000u32,
            );
            std_calldata[2..10].to_string()
        };
        assert_ne!(
            selector, standard_selector,
            "depositForBurnWithHook should have a different selector"
        );
    }

    #[test]
    fn test_deposit_for_burn_with_hook_calldata_contains_hook_data() {
        let hook_data = Bytes::copy_from_slice(&CCTP_FORWARD_HOOK_DATA);
        let calldata = encode_deposit_for_burn_with_hook_calldata(
            U256::from(1000000u64),
            6u32,
            B256::ZERO,
            Address::from_str("0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238").unwrap(),
            B256::ZERO,
            U256::from(1000u64),
            1000u32,
            hook_data,
        );
        // The hook data ABI encoding includes an offset, length (32), and the data.
        // Verify the hook data bytes appear in the calldata somewhere
        let hook_hex = "636374702d666f7277617264";
        assert!(
            calldata.contains(hook_hex),
            "hook calldata should contain the forwarding magic bytes"
        );
    }

    #[test]
    fn test_deposit_for_burn_with_hook_calldata_contains_dest_domain() {
        let hook_data = Bytes::copy_from_slice(&CCTP_FORWARD_HOOK_DATA);
        let calldata = encode_deposit_for_burn_with_hook_calldata(
            U256::from(1000000u64),
            6u32,
            B256::ZERO,
            Address::ZERO,
            B256::ZERO,
            U256::ZERO,
            1000u32,
            hook_data,
        );
        assert!(
            calldata.contains("0000000000000000000000000000000000000000000000000000000000000006")
        );
    }

    #[test]
    fn test_hook_data_constant_length() {
        assert_eq!(CCTP_FORWARD_HOOK_DATA.len(), 32);
        assert_eq!(CCTP_FORWARD_HOOK_DATA_HEX.len(), 66); // "0x" + 64 hex chars
    }
}
