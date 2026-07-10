use crate::{
    config::{ChainConfig, GulfConfig, ProviderConfig, TokenConfig},
    model::QuoteRequest,
};
use std::collections::BTreeMap;

use super::{RevenueShare, amount::amount_decimal, response::string_value};

pub(super) fn render(
    template: &str,
    provider: &ProviderConfig,
    quote: &QuoteRequest,
    config: &GulfConfig,
    revenue: &RevenueShare,
) -> String {
    render_with_steps(template, provider, quote, config, revenue, &BTreeMap::new())
}

pub(super) fn render_with_steps(
    template: &str,
    provider: &ProviderConfig,
    quote: &QuoteRequest,
    config: &GulfConfig,
    revenue: &RevenueShare,
    step_outputs: &BTreeMap<String, serde_json::Value>,
) -> String {
    let mut values = BTreeMap::from([
        ("provider.id".to_owned(), provider.id.clone()),
        ("from_chain".to_owned(), quote.from_chain.clone()),
        ("to_chain".to_owned(), quote.to_chain.clone()),
        ("from_token".to_owned(), quote.from_token.clone()),
        ("to_token".to_owned(), quote.to_token.clone()),
        ("amount".to_owned(), quote.amount.clone()),
        ("amount.raw".to_owned(), quote.amount.clone()),
        ("sender".to_owned(), quote.sender.clone()),
        (
            "recipient".to_owned(),
            quote
                .recipient
                .clone()
                .unwrap_or_else(|| quote.sender.clone()),
        ),
        (
            "commission.bps".to_owned(),
            revenue.integrator_fee_bps.to_string(),
        ),
        (
            "commission.percent".to_owned(),
            (revenue.integrator_fee_bps / 100.0).to_string(),
        ),
        (
            "commission.decimal".to_owned(),
            (revenue.integrator_fee_bps / 10_000.0).to_string(),
        ),
        (
            "owner.commission.bps".to_owned(),
            revenue.owner_fee_bps.to_string(),
        ),
        (
            "owner.commission.percent".to_owned(),
            (revenue.owner_fee_bps / 100.0).to_string(),
        ),
        (
            "owner.commission.decimal".to_owned(),
            (revenue.owner_fee_bps / 10_000.0).to_string(),
        ),
        ("partner.id".to_owned(), revenue.partner_id.clone()),
        (
            "partner.fee_bps".to_owned(),
            revenue.partner_fee_bps.to_string(),
        ),
        (
            "partner.fee_percent".to_owned(),
            (revenue.partner_fee_bps / 100.0).to_string(),
        ),
        (
            "partner.fee_decimal".to_owned(),
            (revenue.partner_fee_bps / 10_000.0).to_string(),
        ),
        (
            "partner.recipient".to_owned(),
            revenue.partner_recipient.clone(),
        ),
        (
            "revenue.total_bps".to_owned(),
            revenue.integrator_fee_bps.to_string(),
        ),
        (
            "revenue.total_percent".to_owned(),
            (revenue.integrator_fee_bps / 100.0).to_string(),
        ),
        (
            "revenue.total_decimal".to_owned(),
            (revenue.integrator_fee_bps / 10_000.0).to_string(),
        ),
        (
            "native_token_address.evm_zero".to_owned(),
            "0x0000000000000000000000000000000000000000".to_owned(),
        ),
        (
            "native_token_address.evm_eeee".to_owned(),
            "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE".to_owned(),
        ),
    ]);

    for (vm, recipient) in &config.developer.recipients {
        values.insert(
            format!("developer.{vm}_recipient"),
            resolve_allowed_env_templates(recipient, |key| std::env::var(key).ok()),
        );
    }

    if let Some(chain) = config.find_chain(&quote.from_chain) {
        add_chain_values(&mut values, "from_chain", chain);
    }
    if let Some(chain) = config.find_chain(&quote.to_chain) {
        add_chain_values(&mut values, "to_chain", chain);
    }
    if let Some(chain) = config.find_chain(&quote.from_chain)
        && let Some(token) = config.find_token(&quote.from_token, &chain.caip2_id)
    {
        add_token_values(&mut values, "from_token", token, &provider.id);
        if let Some(decimal) = amount_decimal(&quote.amount, token.decimals) {
            values.insert("amount.decimal".to_owned(), decimal);
        }
    }
    if let Some(chain) = config.find_chain(&quote.to_chain)
        && let Some(token) = config.find_token(&quote.to_token, &chain.caip2_id)
    {
        add_token_values(&mut values, "to_token", token, &provider.id);
    }

    for key in crate::config::allowed_template_env_vars() {
        if let Ok(value) = std::env::var(key) {
            values.insert(format!("env.{key}"), value);
        }
    }

    for (step, raw) in step_outputs {
        collect_step_values(&mut values, &format!("step.{step}"), raw);
    }

    let mut rendered = template.to_owned();
    for (key, value) in values {
        rendered = rendered.replace(&format!("${{{key}}}"), &value);
    }

    replace_unresolved_env_templates(rendered)
}

pub(super) fn collect_step_values(
    values: &mut BTreeMap<String, String>,
    prefix: &str,
    value: &serde_json::Value,
) {
    if let Some(scalar) = string_value(value) {
        values.insert(prefix.to_string(), scalar);
    }
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                collect_step_values(values, &format!("{prefix}.{key}"), child);
            }
        }
        serde_json::Value::Array(items) => {
            for (index, child) in items.iter().enumerate() {
                collect_step_values(values, &format!("{prefix}.{index}"), child);
            }
        }
        _ => {}
    }
}

pub(super) fn render_optional_header(
    template: &str,
    provider: &ProviderConfig,
    quote: &QuoteRequest,
    config: &GulfConfig,
    revenue: &RevenueShare,
) -> Option<String> {
    let rendered = render(template, provider, quote, config, revenue);
    if rendered.is_empty() {
        None
    } else {
        Some(rendered)
    }
}

pub(super) fn render_optional_param_with_steps(
    template: &str,
    provider: &ProviderConfig,
    quote: &QuoteRequest,
    config: &GulfConfig,
    revenue: &RevenueShare,
    step_outputs: &BTreeMap<String, serde_json::Value>,
) -> Option<String> {
    let rendered = render_with_steps(template, provider, quote, config, revenue, step_outputs);
    if rendered.is_empty() {
        None
    } else {
        Some(rendered)
    }
}

pub(super) fn render_json_with_steps(
    value: &serde_json::Value,
    provider: &ProviderConfig,
    quote: &QuoteRequest,
    config: &GulfConfig,
    revenue: &RevenueShare,
    step_outputs: &BTreeMap<String, serde_json::Value>,
) -> serde_json::Value {
    match value {
        serde_json::Value::String(template) => {
            if let Some(step_value) = step_template_value(template, step_outputs) {
                step_value.clone()
            } else {
                let rendered =
                    render_with_steps(template, provider, quote, config, revenue, step_outputs);
                if rendered.is_empty() && is_exact_template(template) {
                    return serde_json::Value::Null;
                }
                if is_numeric_json_template(template) {
                    rendered
                        .parse::<u64>()
                        .ok()
                        .map(serde_json::Value::from)
                        .or_else(|| {
                            rendered
                                .parse::<f64>()
                                .ok()
                                .and_then(serde_json::Number::from_f64)
                                .map(serde_json::Value::Number)
                        })
                        .unwrap_or(serde_json::Value::String(rendered))
                } else {
                    serde_json::Value::String(rendered)
                }
            }
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .map(|item| {
                    render_json_with_steps(item, provider, quote, config, revenue, step_outputs)
                })
                .collect(),
        ),
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .filter_map(|(key, value)| {
                    let rendered = render_json_with_steps(
                        value,
                        provider,
                        quote,
                        config,
                        revenue,
                        step_outputs,
                    );
                    (!rendered.is_null()).then(|| (key.clone(), rendered))
                })
                .collect(),
        ),
        other => other.clone(),
    }
}

fn is_numeric_json_template(template: &str) -> bool {
    matches!(
        template
            .strip_prefix("${")
            .and_then(|value| value.strip_suffix('}')),
        Some(
            "from_chain.numeric_chain_id"
                | "to_chain.numeric_chain_id"
                | "from_token.decimals"
                | "to_token.decimals"
                | "commission.bps"
                | "revenue.total_bps"
                | "owner.commission.bps"
                | "partner.fee_bps"
        )
    )
}

fn is_exact_template(template: &str) -> bool {
    template.starts_with("${") && template.ends_with('}')
}

fn step_template_value<'a>(
    template: &str,
    step_outputs: &'a BTreeMap<String, serde_json::Value>,
) -> Option<&'a serde_json::Value> {
    let inner = template.strip_prefix("${")?.strip_suffix('}')?;
    let rest = inner.strip_prefix("step.")?;
    let (step, path) = rest.split_once('.')?;
    let raw = step_outputs.get(step)?;
    super::response::json_path(raw, path)
}

fn replace_unresolved_env_templates(mut rendered: String) -> String {
    while let Some(start) = rendered.find("${env.") {
        let Some(relative_end) = rendered[start..].find('}') else {
            break;
        };
        let end = start + relative_end + 1;
        rendered.replace_range(start..end, "");
    }
    rendered
}

/// Resolve only allowlisted environment templates embedded in a configured
/// developer recipient. This prevents nested templates from escaping the
/// recipient substitution pass without exposing arbitrary process variables.
fn resolve_allowed_env_templates<F>(template: &str, lookup: F) -> String
where
    F: Fn(&str) -> Option<String>,
{
    let mut rendered = String::new();
    let mut remainder = template;
    while let Some(start) = remainder.find("${env.") {
        rendered.push_str(&remainder[..start]);
        let candidate = &remainder[start + 6..];
        let Some(end) = candidate.find('}') else {
            rendered.push_str(&remainder[start..]);
            return rendered;
        };
        let name = &candidate[..end];
        if crate::config::allowed_template_env_vars().contains(&name) {
            rendered.push_str(&lookup(name).unwrap_or_default());
        }
        remainder = &candidate[end + 1..];
    }
    rendered.push_str(remainder);
    rendered
}

fn add_chain_values(values: &mut BTreeMap<String, String>, prefix: &str, chain: &ChainConfig) {
    values.insert(format!("{prefix}.caip2_id"), chain.caip2_id.clone());
    values.insert(format!("{prefix}.display_name"), chain.display_name.clone());
    values.insert(format!("{prefix}.vm"), chain.vm.clone());
    if let Some(chain_id) = chain.numeric_chain_id {
        values.insert(format!("{prefix}.numeric_chain_id"), chain_id.to_string());
        // Always-string variant for providers (e.g. Orbiter) that require
        // chain IDs to be JSON strings rather than integers.
        values.insert(
            format!("{prefix}.numeric_chain_id_str"),
            chain_id.to_string(),
        );
    }
    if let Some(symbol) = &chain.native_symbol {
        values.insert(format!("{prefix}.native_symbol"), symbol.clone());
    }
    for (provider, value) in &chain.provider_references {
        values.insert(format!("{prefix}.provider.{provider}"), value.clone());
    }
}

fn add_token_values(
    values: &mut BTreeMap<String, String>,
    prefix: &str,
    token: &TokenConfig,
    provider_id: &str,
) {
    values.insert(format!("{prefix}.id"), token.id.clone());
    values.insert(format!("{prefix}.symbol"), token.symbol.clone());
    values.insert(format!("{prefix}.decimals"), token.decimals.to_string());
    if let Some(address) = &token.address {
        values.insert(format!("{prefix}.address"), address.clone());
    }
    if let Some(symbol) = token.provider_symbols.get(provider_id) {
        values.insert(format!("{prefix}.provider_symbol"), symbol.clone());
    }
    for (provider, symbol) in &token.provider_symbols {
        values.insert(format!("{prefix}.provider.{provider}"), symbol.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CommissionConfig, FeeConfig, FeeUnit, ProviderConfig, ServiceType};
    use crate::{config::GulfConfig, model::QuoteRequest};

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
    fn test_render_json_injects_step_scalar_into_second_step_body() {
        let mut steps = BTreeMap::new();
        steps.insert(
            "quote".to_string(),
            serde_json::json!({ "pathId": "path-123", "outAmounts": ["42"] }),
        );
        let rendered = render_json_with_steps(
            &serde_json::json!({ "userAddr": "${sender}", "pathId": "${step.quote.pathId}" }),
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            &RevenueShare {
                owner_fee_bps: 0.0,
                partner_fee_bps: 0.0,
                integrator_fee_bps: 0.0,
                partner_id: String::new(),
                partner_recipient: String::new(),
            },
            &steps,
        );
        assert_eq!(rendered["pathId"], "path-123");
        assert_eq!(rendered["userAddr"], "0xSender");
    }

    #[test]
    fn test_render_json_converts_raw_amount_to_token_decimal() {
        let rendered = render_json_with_steps(
            &serde_json::json!({ "amount": "${amount.decimal}" }),
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            &RevenueShare {
                owner_fee_bps: 0.0,
                partner_fee_bps: 0.0,
                integrator_fee_bps: 0.0,
                partner_id: String::new(),
                partner_recipient: String::new(),
            },
            &BTreeMap::new(),
        );
        assert_eq!(rendered["amount"], "1");
    }

    #[test]
    fn test_render_json_injects_step_object_without_stringifying() {
        let price_route = serde_json::json!({
            "srcToken": "0xSrc",
            "destToken": "0xDest",
            "srcAmount": "1000000",
            "destAmount": "999000"
        });
        let mut steps = BTreeMap::new();
        steps.insert(
            "price".to_string(),
            serde_json::json!({ "priceRoute": price_route.clone() }),
        );
        let rendered = render_json_with_steps(
            &serde_json::json!({ "priceRoute": "${step.price.priceRoute}", "slippage": 50 }),
            &make_provider(),
            &make_quote("usdc"),
            &make_config(),
            &RevenueShare {
                owner_fee_bps: 0.0,
                partner_fee_bps: 0.0,
                integrator_fee_bps: 0.0,
                partner_id: String::new(),
                partner_recipient: String::new(),
            },
            &steps,
        );
        assert_eq!(rendered["priceRoute"], price_route);
        assert_eq!(rendered["slippage"], 50);
    }

    #[test]
    fn developer_recipient_resolves_allowlisted_env_template() {
        assert_eq!(
            resolve_allowed_env_templates("${env.GULF_E2E_EVM_FEE_RECIPIENT}", |name| {
                (name == "GULF_E2E_EVM_FEE_RECIPIENT")
                    .then(|| "0x0000000000000000000000000000000000000001".to_string())
            }),
            "0x0000000000000000000000000000000000000001"
        );
        assert_eq!(
            resolve_allowed_env_templates("${env.NOT_ALLOWLISTED}", |_| Some("leak".to_string())),
            ""
        );
    }

    #[test]
    fn missing_developer_recipient_env_is_empty_and_omitted_from_json() {
        let mut config = make_config();
        config.developer.recipients.insert(
            "evm".to_string(),
            "${env.GULF_E2E_EVM_FEE_RECIPIENT}".to_string(),
        );
        let revenue = RevenueShare {
            owner_fee_bps: 0.0,
            partner_fee_bps: 0.0,
            integrator_fee_bps: 0.0,
            partner_id: String::new(),
            partner_recipient: String::new(),
        };
        assert_eq!(
            render(
                "${developer.evm_recipient}",
                &make_provider(),
                &make_quote("usdc"),
                &config,
                &revenue
            ),
            ""
        );
        let rendered = render_json_with_steps(
            &serde_json::json!({ "feeRecipient": "${developer.evm_recipient}", "feePercent": "0" }),
            &make_provider(),
            &make_quote("usdc"),
            &config,
            &revenue,
            &BTreeMap::new(),
        );
        assert_eq!(rendered, serde_json::json!({ "feePercent": "0" }));
    }
}
