use crate::{config::ResponseMappingConfig, model::NormalizedQuote, model::NormalizedTransaction};

pub(super) fn json_path<'a>(
    value: &'a serde_json::Value,
    path: &str,
) -> Option<&'a serde_json::Value> {
    if path.is_empty() || path == "$" {
        return Some(value);
    }
    let mut current = value;
    let trimmed = path.strip_prefix("$.").unwrap_or(path);
    if trimmed.is_empty() {
        return Some(value);
    }
    for part in trimmed.split('.') {
        if part.is_empty() {
            continue;
        }
        current = if let Ok(index) = part.parse::<usize>() {
            current.get(index)?
        } else {
            current.get(part)?
        };
    }
    Some(current)
}

pub(super) fn string_value(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(value) => Some(value.clone()),
        serde_json::Value::Number(value) => Some(value.to_string()),
        serde_json::Value::Bool(value) => Some(value.to_string()),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => {
            serde_json::to_string(value).ok()
        }
        _ => None,
    }
}

pub(super) fn f64_value(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(value) => value.as_f64(),
        serde_json::Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

pub(super) fn u64_value(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(value) => value.as_u64(),
        serde_json::Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

pub(super) fn normalize_quote(
    mapping: &ResponseMappingConfig,
    raw: &serde_json::Value,
) -> NormalizedQuote {
    NormalizedQuote {
        output_amount: mapping
            .output_amount
            .as_deref()
            .and_then(|path| json_path(raw, path))
            .and_then(string_value),
        estimated_fee_bps: mapping
            .estimated_fee_bps
            .as_deref()
            .and_then(|path| json_path(raw, path))
            .and_then(f64_value),
        estimated_fee_usd: mapping
            .estimated_fee_usd
            .as_deref()
            .and_then(|path| json_path(raw, path))
            .and_then(f64_value),
        gas_fee_usd: mapping
            .gas_fee_usd
            .as_deref()
            .and_then(|path| json_path(raw, path))
            .and_then(f64_value),
        settlement_seconds: mapping
            .settlement_seconds
            .as_deref()
            .and_then(|path| json_path(raw, path))
            .and_then(u64_value),
        error: mapping
            .error
            .as_deref()
            .and_then(|path| json_path(raw, path))
            .and_then(string_value),
    }
}

pub(super) fn normalize_transaction(
    mapping: &ResponseMappingConfig,
    raw: &serde_json::Value,
    _approval_token_address: Option<String>,
) -> NormalizedTransaction {
    NormalizedTransaction {
        to: mapping
            .transaction_to
            .as_deref()
            .and_then(|path| json_path(raw, path))
            .and_then(string_value),
        data: mapping
            .transaction_data
            .as_deref()
            .and_then(|path| json_path(raw, path))
            .and_then(string_value),
        value: mapping
            .transaction_value
            .as_deref()
            .and_then(|path| json_path(raw, path))
            .and_then(string_value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---------------------------------------------------------------------------
    // json_path root / empty / nested tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_json_path_root_dollar() {
        let raw = serde_json::json!({"tx": {"to": "0xSwap"}});
        let result = json_path(&raw, "$");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), &raw);
    }

    #[test]
    fn test_json_path_root_empty() {
        let raw = serde_json::json!({"tx": {"to": "0xSwap"}});
        let result = json_path(&raw, "");
        assert!(result.is_some());
        assert_eq!(result.unwrap(), &raw);
    }

    #[test]
    fn test_json_path_dollar_prefixed_nested() {
        let raw = serde_json::json!({"tx": {"to": "0xSwap"}});
        let result = json_path(&raw, "$.tx.to");
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_str(), Some("0xSwap"));
    }

    #[test]
    fn test_json_path_no_prefix_nested() {
        let raw = serde_json::json!({"tx": {"to": "0xSwap"}});
        let result = json_path(&raw, "tx.to");
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_str(), Some("0xSwap"));
    }

    #[test]
    fn test_json_path_numeric_index() {
        let raw = serde_json::json!({"steps": [{"to": "0xSwap"}, {"to": "0xBridge"}]});
        let result = json_path(&raw, "steps.1.to");
        assert!(result.is_some());
        assert_eq!(result.unwrap().as_str(), Some("0xBridge"));
    }

    #[test]
    fn test_json_path_nonexistent() {
        let raw = serde_json::json!({"tx": {"to": "0xSwap"}});
        assert!(json_path(&raw, "tx.nonexistent").is_none());
    }

    // ---------------------------------------------------------------------------
    // string_value tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_string_value_object_serializes_to_json() {
        let value = serde_json::json!({"to": "0xSwap", "data": "0xdeadbeef"});
        let result = string_value(&value);
        assert!(result.is_some());
        let parsed: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(parsed.get("to").unwrap().as_str(), Some("0xSwap"));
    }

    #[test]
    fn test_string_value_array_serializes_to_json() {
        let value = serde_json::json!([1, 2, 3]);
        let result = string_value(&value);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "[1,2,3]");
    }

    #[test]
    fn test_string_value_null_returns_none() {
        let value = serde_json::Value::Null;
        assert!(string_value(&value).is_none());
    }

    // ---------------------------------------------------------------------------
    // json_path root with string_value for serialized_transaction scenario
    // ---------------------------------------------------------------------------

    #[test]
    fn test_root_json_path_string_value_for_evm_response() {
        let raw = serde_json::json!({"from": "0xSender", "to": "0xTarget", "value": "0", "data": "0xabcdef"});
        let root = json_path(&raw, "$").unwrap();
        let as_str = string_value(root);
        assert!(as_str.is_some());
        // The root object should be serialized to JSON
        assert!(as_str.unwrap().contains("0xTarget"));
    }

    #[test]
    fn test_root_path_maps_entire_response_for_serialized_transaction() {
        let raw = serde_json::json!("AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA...");
        let root = json_path(&raw, "$").unwrap();
        assert_eq!(
            string_value(root).as_deref(),
            Some("AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA...")
        );
    }
}
