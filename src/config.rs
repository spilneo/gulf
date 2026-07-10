use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GulfConfig {
    pub project: Option<ProjectConfig>,
    pub developer: DeveloperConfig,
    pub safety: Option<SafetyConfig>,
    pub routing: Option<RoutingConfig>,
    pub server: Option<ServerConfig>,
    #[serde(default)]
    pub chains: Vec<ChainConfig>,
    #[serde(default)]
    pub tokens: Vec<TokenConfig>,
    #[serde(default)]
    pub providers: Vec<ProviderConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    pub rust_version: String,
    pub edition: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperConfig {
    pub default_commission_bps: f64,
    #[serde(default)]
    pub recipients: BTreeMap<String, String>,
    #[serde(default)]
    pub b2b: B2bFeeConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct B2bFeeConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub default_partner_fee_bps: f64,
    pub max_partner_fee_bps: Option<f64>,
    #[serde(default)]
    pub require_partner_recipient: bool,
    pub settlement: Option<String>,
    #[serde(default)]
    pub partners: BTreeMap<String, B2bPartnerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct B2bPartnerConfig {
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub default_fee_bps: Option<f64>,
    pub max_fee_bps: Option<f64>,
    pub recipient: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub default_listen: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyConfig {
    pub max_commission_bps: Option<f64>,
    #[serde(default)]
    pub require_live_quote_for_swap: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingConfig {
    #[serde(default = "RoutingConfig::default_sort")]
    pub sort: Vec<String>,
    #[serde(default)]
    pub require_chain_registry_match: bool,
    #[serde(default)]
    pub require_token_registry_match: bool,
}

impl RoutingConfig {
    fn default_sort() -> Vec<String> {
        vec![
            "total_configured_bps".to_owned(),
            "settlement_seconds".to_owned(),
            "provider_name".to_owned(),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainConfig {
    pub caip2_id: String,
    pub display_name: String,
    pub vm: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    pub numeric_chain_id: Option<u64>,
    pub native_symbol: Option<String>,
    #[serde(default)]
    pub rpc_endpoints: Vec<RpcEndpointConfig>,
    #[serde(default)]
    pub provider_references: BTreeMap<String, String>,
    pub explorer_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcEndpointConfig {
    pub name: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub url: Option<String>,
    pub url_env: Option<String>,
    #[serde(default)]
    pub priority: u16,
    pub timeout_ms: Option<u64>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenConfig {
    pub id: String,
    pub chain_caip2_id: String,
    pub symbol: String,
    pub decimals: u8,
    pub address: Option<String>,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub provider_symbols: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub id: String,
    pub name: String,
    #[serde(default = "enabled")]
    pub enabled: bool,
    pub service_type: ServiceType,
    #[serde(default)]
    pub supported_chains: Vec<String>,
    #[serde(default)]
    pub supported_tokens: Vec<String>,
    pub fee: FeeConfig,
    pub commission: CommissionConfig,
    #[serde(default)]
    pub docs: Vec<String>,
    pub http: Option<HttpConfig>,
    pub blockchain: Option<BlockchainConfig>,
    pub notes: Option<String>,
}

/// Public, non-sensitive provider metadata.
///
/// Request templates and authentication configuration deliberately remain
/// internal to `ProviderConfig` and are never included here.
#[derive(Debug, Clone, Serialize)]
pub struct ProviderSummary {
    pub id: String,
    pub name: String,
    pub service_type: ServiceType,
    pub enabled: bool,
    pub fee: FeeConfig,
    pub commission: CommissionConfig,
    pub supported_chains: Vec<String>,
    pub supported_tokens: Vec<String>,
    pub docs: Vec<String>,
    pub notes: Option<String>,
}

impl From<&ProviderConfig> for ProviderSummary {
    fn from(provider: &ProviderConfig) -> Self {
        Self {
            id: provider.id.clone(),
            name: provider.name.clone(),
            service_type: provider.service_type.clone(),
            enabled: provider.enabled,
            fee: provider.fee.clone(),
            commission: provider.commission.clone(),
            supported_chains: provider.supported_chains.clone(),
            supported_tokens: provider.supported_tokens.clone(),
            docs: provider.docs.clone(),
            notes: provider.notes.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceType {
    DirectBridge,
    MetaAggregator,
    DexAggregator,
    NativeSwap,
    ContractOnly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeConfig {
    pub unit: FeeUnit,
    #[serde(default)]
    pub dynamic: bool,
    pub estimated_bps: Option<f64>,
    pub flat_usd: Option<f64>,
    pub settlement_seconds: Option<u64>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeeUnit {
    Bps,
    Percent,
    FlatUsd,
    Dynamic,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommissionConfig {
    #[serde(default)]
    pub supported: bool,
    #[serde(default = "CommissionConfig::default_strategy")]
    pub strategy: String,
    pub default_bps: Option<f64>,
    pub max_bps: Option<f64>,
    #[serde(default)]
    pub parameters: BTreeMap<String, String>,
    pub notes: Option<String>,
}

impl CommissionConfig {
    fn default_strategy() -> String {
        "none".to_owned()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpConfig {
    pub base_url: String,
    #[serde(default)]
    pub default_headers: BTreeMap<String, String>,
    #[serde(default)]
    pub default_query: BTreeMap<String, String>,
    pub auth: Option<AuthConfig>,
    #[serde(default)]
    pub requests: BTreeMap<String, EndpointConfig>,
    pub prepare_flow: Option<HttpFlowConfig>,
    pub quote: Option<EndpointConfig>,
    pub prepare: Option<EndpointConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpFlowConfig {
    pub steps: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthConfig {
    pub header: Option<String>,
    pub value: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub query: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointConfig {
    #[serde(default = "EndpointConfig::default_method")]
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub query: BTreeMap<String, String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body_json: Option<serde_json::Value>,
    #[serde(default)]
    pub body_form: BTreeMap<String, String>,
    pub response: Option<ResponseMappingConfig>,
    pub timeout_ms: Option<u64>,
    pub retry_attempts: Option<u8>,
}

impl EndpointConfig {
    fn default_method() -> String {
        "GET".to_owned()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ResponseMappingConfig {
    pub execution_kind: Option<String>,
    pub output_amount: Option<String>,
    pub estimated_fee_bps: Option<String>,
    pub estimated_fee_usd: Option<String>,
    pub gas_fee_usd: Option<String>,
    pub settlement_seconds: Option<String>,
    pub transaction_to: Option<String>,
    pub transaction_data: Option<String>,
    pub transaction_value: Option<String>,
    pub erc20_approval_spender: Option<String>,
    pub erc20_approval_amount: Option<String>,
    pub error: Option<String>,
    pub approval_transaction_to: Option<String>,
    pub approval_transaction_data: Option<String>,
    pub approval_transaction_value: Option<String>,
    pub deposit_address: Option<String>,
    pub memo: Option<String>,
    pub serialized_transaction: Option<String>,
    pub expires_at: Option<String>,
    pub status_id: Option<String>,
    pub status_endpoint: Option<String>,
    pub status_tx_hash: Option<String>,
    pub actions: Option<String>,
    pub action_type: Option<String>,
    pub action_description: Option<String>,
    pub action_to: Option<String>,
    pub action_data: Option<String>,
    pub action_value: Option<String>,
    pub action_token: Option<String>,
    pub action_spender: Option<String>,
    pub action_amount: Option<String>,
    pub action_deposit_address: Option<String>,
    pub action_memo: Option<String>,
    pub action_serialized_transaction: Option<String>,
    pub typed_data: Option<String>,
    pub message: Option<String>,
    pub intent: Option<String>,
    pub submission: Option<String>,
    pub action_typed_data: Option<String>,
    pub action_message: Option<String>,
    pub action_intent: Option<String>,
    pub action_submission: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockchainConfig {
    pub vm: String,
    pub chain_caip2_id: Option<String>,
}

impl GulfConfig {
    pub async fn load(source: &str) -> Result<Self> {
        let raw = if Url::parse(source).is_ok_and(|url| matches!(url.scheme(), "http" | "https")) {
            reqwest::get(source)
                .await
                .with_context(|| format!("failed to fetch config from {source}"))?
                .error_for_status()?
                .text()
                .await?
        } else {
            fs::read_to_string(source).with_context(|| format!("failed to read config {source}"))?
        };

        let config: Self = toml::from_str(&raw).context("failed to parse TOML config")?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.providers.is_empty() {
            bail!("at least one provider is required");
        }
        if !(0.0..=10_000.0).contains(&self.developer.default_commission_bps) {
            bail!("developer.default_commission_bps must be between 0 and 10000");
        }
        if !(0.0..=10_000.0).contains(&self.developer.b2b.default_partner_fee_bps) {
            bail!("developer.b2b.default_partner_fee_bps must be between 0 and 10000");
        }
        if let Some(max) = self.developer.b2b.max_partner_fee_bps {
            if !(0.0..=10_000.0).contains(&max) {
                bail!("developer.b2b.max_partner_fee_bps must be between 0 and 10000");
            }
            if self.developer.b2b.default_partner_fee_bps > max {
                bail!(
                    "developer.b2b.default_partner_fee_bps exceeds developer.b2b.max_partner_fee_bps"
                );
            }
        }
        for (partner_id, partner) in &self.developer.b2b.partners {
            if partner_id.trim().is_empty() {
                bail!("developer.b2b partner id cannot be empty");
            }
            if let Some(default_fee) = partner.default_fee_bps
                && !(0.0..=10_000.0).contains(&default_fee)
            {
                bail!(
                    "developer.b2b.partners.{partner_id}.default_fee_bps must be between 0 and 10000"
                );
            }
            if let Some(max) = partner.max_fee_bps {
                if !(0.0..=10_000.0).contains(&max) {
                    bail!(
                        "developer.b2b.partners.{partner_id}.max_fee_bps must be between 0 and 10000"
                    );
                }
                if partner.default_fee_bps.unwrap_or_default() > max {
                    bail!(
                        "developer.b2b.partners.{partner_id}.default_fee_bps exceeds max_fee_bps"
                    );
                }
                if self
                    .developer
                    .b2b
                    .max_partner_fee_bps
                    .is_some_and(|policy_max| max > policy_max)
                {
                    bail!(
                        "developer.b2b.partners.{partner_id}.max_fee_bps exceeds developer.b2b.max_partner_fee_bps"
                    );
                }
            }
        }
        if let Some(safety) = &self.safety {
            if safety
                .max_commission_bps
                .is_some_and(|bps| !(0.0..=10_000.0).contains(&bps))
            {
                bail!("safety.max_commission_bps must be between 0 and 10000");
            }
            if let Some(max) = safety.max_commission_bps
                && self.developer.default_commission_bps > max
            {
                bail!("developer.default_commission_bps exceeds safety.max_commission_bps");
            }
        }

        let mut seen_chain_caip2_ids = BTreeSet::new();
        for chain in &self.chains {
            if !seen_chain_caip2_ids.insert(chain.caip2_id.clone()) {
                bail!("duplicate chain {}", chain.caip2_id);
            }
        }

        let mut seen_token_registry_ids = BTreeSet::new();
        for token in &self.tokens {
            if !seen_token_registry_ids.insert(token.id.clone()) {
                bail!("duplicate token {}", token.id);
            }
        }

        let mut provider_ids = BTreeSet::new();
        for provider in &self.providers {
            if provider.id.trim().is_empty() {
                bail!("provider id cannot be empty");
            }
            if !provider_ids.insert(provider.id.clone()) {
                bail!("duplicate provider {}", provider.id);
            }

            let provider_json = serde_json::to_string(provider).unwrap_or_default();
            validate_env_templates(&provider_json, &provider.id)?;

            if provider.supported_chains.is_empty() {
                bail!("provider {} must configure supported_chains", provider.id);
            }
            for chain_id in &provider.supported_chains {
                if !is_valid_caip2_id(chain_id) {
                    bail!(
                        "provider {} references invalid CAIP-2 chain {}",
                        provider.id,
                        chain_id
                    );
                }
                if self.find_chain(chain_id).is_none() {
                    bail!(
                        "provider {} references unknown chain {}",
                        provider.id,
                        chain_id
                    );
                }
            }
            for token_id in &provider.supported_tokens {
                if !seen_token_registry_ids.contains(token_id) {
                    bail!(
                        "provider {} references unknown token {}",
                        provider.id,
                        token_id
                    );
                }
            }
            if provider.commission.supported {
                if let Some(max) = provider.commission.max_bps
                    && provider.commission.default_bps.unwrap_or_default() > max
                {
                    bail!(
                        "provider {} commission default exceeds max commission",
                        provider.id
                    );
                }
                if let Some(safety_max) = self
                    .safety
                    .as_ref()
                    .and_then(|safety| safety.max_commission_bps)
                    && provider.commission.default_bps.unwrap_or_default() > safety_max
                {
                    bail!(
                        "provider {} commission default exceeds safety.max_commission_bps",
                        provider.id
                    );
                }
            }
        }

        for chain in &self.chains {
            if !is_valid_caip2_id(&chain.caip2_id) {
                bail!("chain {} is not a valid CAIP-2 identifier", chain.caip2_id);
            }
            for rpc in &chain.rpc_endpoints {
                if rpc.url.is_none() && rpc.url_env.is_none() {
                    bail!(
                        "chain {} rpc endpoint {} must set url or url_env",
                        chain.caip2_id,
                        rpc.name
                    );
                }
                if let Some(name) = &rpc.url_env
                    && !allowed_config_env_vars().contains(&name.as_str())
                {
                    bail!(
                        "chain {} rpc endpoint {} references unsupported environment variable {}",
                        chain.caip2_id,
                        rpc.name,
                        name
                    );
                }
            }
        }

        for token in &self.tokens {
            if token.id.trim().is_empty() {
                bail!("token id cannot be empty");
            }
            if self.find_chain(&token.chain_caip2_id).is_none() {
                bail!(
                    "token {} references unknown chain {}",
                    token.id,
                    token.chain_caip2_id
                );
            }
        }

        Ok(())
    }

    pub fn find_chain(&self, value: &str) -> Option<&ChainConfig> {
        self.chains.iter().find(|chain| {
            chain.caip2_id == value
                || chain.display_name.eq_ignore_ascii_case(value)
                || chain
                    .aliases
                    .iter()
                    .any(|alias| alias.eq_ignore_ascii_case(value))
                || chain.numeric_chain_id.is_some_and(|chain_id| {
                    value == format!("eip155:{chain_id}") || chain_id.to_string() == value
                })
        })
    }

    pub fn find_token(&self, value: &str, chain_caip2_id: &str) -> Option<&TokenConfig> {
        self.tokens.iter().find(|token| {
            token.chain_caip2_id == chain_caip2_id
                && (token.id.eq_ignore_ascii_case(value)
                    || token.symbol.eq_ignore_ascii_case(value)
                    || token
                        .address
                        .as_ref()
                        .is_some_and(|address| address.eq_ignore_ascii_case(value))
                    || token
                        .aliases
                        .iter()
                        .any(|alias| alias.eq_ignore_ascii_case(value)))
        })
    }
}

/// Environment variables that configuration templates may read.
///
/// This is intentionally finite: configuration must never become a mechanism
/// for exposing arbitrary process environment values to provider requests.
pub fn allowed_template_env_vars() -> &'static [&'static str] {
    &[
        "GULF_ACROSS_API_KEY",
        "GULF_ACROSS_INTEGRATOR_ID",
        "GULF_BITCOIN_FEE_RECIPIENT",
        "GULF_CHAINFLIP_BROKER_API_KEY",
        "GULF_CHAINFLIP_BROKER_API_URL",
        "GULF_CHANGENOW_API_KEY",
        "GULF_CIRCLE_API_KEY",
        "GULF_CIRCLE_IRIS_API_URL",
        "GULF_CIRCLE_IRIS_SANDBOX_API_URL",
        "GULF_ECO_DAPP_ID",
        "GULF_E2E_EVM_FEE_RECIPIENT",
        "GULF_EVM_FEE_RECIPIENT",
        "GULF_LIFI_API_KEY",
        "GULF_MAYAN_API_KEY",
        "GULF_ODOS_API_KEY",
        "GULF_ORBITER_API_KEY",
        "GULF_RANGO_API_KEY",
        "GULF_RHINO_API_KEY",
        "GULF_RUBIC_API_KEY",
        "GULF_SOCKET_AFFILIATE_ID",
        "GULF_SOCKET_API_KEY",
        "GULF_SOLANA_FEE_RECIPIENT",
        "GULF_SQUID_INTEGRATOR_ID",
        "GULF_THORNAME_FEE_RECIPIENT",
        "GULF_ZEROX_API_KEY",
    ]
}

fn allowed_config_env_vars() -> &'static [&'static str] {
    &[
        "GULF_APTOS_RPC_URL",
        "GULF_ARBITRUM_RPC_URL",
        "GULF_AVALANCHE_RPC_URL",
        "GULF_BASE_RPC_URL",
        "GULF_BASE_SEPOLIA_RPC_URL",
        "GULF_BITCOIN_RPC_URL",
        "GULF_BNB_RPC_URL",
        "GULF_CELO_RPC_URL",
        "GULF_CELO_SEPOLIA_RPC_URL",
        "GULF_COSMOS_RPC_URL",
        "GULF_ETHEREUM_RPC_URL",
        "GULF_GNOSIS_RPC_URL",
        "GULF_INK_RPC_URL",
        "GULF_LINEA_RPC_URL",
        "GULF_LITECOIN_RPC_URL",
        "GULF_MANTLE_RPC_URL",
        "GULF_NEAR_RPC_URL",
        "GULF_OPTIMISM_RPC_URL",
        "GULF_OSMOSIS_RPC_URL",
        "GULF_POLKADOT_RPC_URL",
        "GULF_POLYGON_RPC_URL",
        "GULF_SEPOLIA_RPC_URL",
        "GULF_SOLANA_RPC_URL",
        "GULF_SONIC_RPC_URL",
        "GULF_STARKNET_RPC_URL",
        "GULF_STELLAR_RPC_URL",
        "GULF_SUI_RPC_URL",
        "GULF_THORCHAIN_RPC_URL",
        "GULF_TRON_RPC_URL",
        "GULF_UNICHAIN_RPC_URL",
        "GULF_WORLDCHAIN_RPC_URL",
        "GULF_ZKSYNC_RPC_URL",
    ]
}

fn validate_env_templates(value: &str, provider_id: &str) -> Result<()> {
    let mut remainder = value;
    while let Some(start) = remainder.find("${env.") {
        let candidate = &remainder[start + 6..];
        let Some(end) = candidate.find('}') else {
            bail!("provider {provider_id} contains an unterminated environment template");
        };
        let name = &candidate[..end];
        if !allowed_template_env_vars().contains(&name) {
            bail!("provider {provider_id} references unsupported environment variable {name}");
        }
        remainder = &candidate[end + 1..];
    }
    Ok(())
}

fn enabled() -> bool {
    true
}

fn is_valid_caip2_id(value: &str) -> bool {
    let Some((namespace, reference)) = value.split_once(':') else {
        return false;
    };
    (3..=8).contains(&namespace.len())
        && (1..=32).contains(&reference.len())
        && namespace
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        && reference
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_valid_provider(id: &str, name: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.to_string(),
            name: name.to_string(),
            enabled: true,
            service_type: ServiceType::DexAggregator,
            supported_chains: vec!["eip155:1".to_string()],
            supported_tokens: vec![],
            fee: FeeConfig {
                unit: FeeUnit::Bps,
                dynamic: false,
                estimated_bps: Some(0.0),
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

    fn minimal_valid_config(providers: Vec<ProviderConfig>) -> GulfConfig {
        GulfConfig {
            project: None,
            developer: DeveloperConfig {
                default_commission_bps: 0.0,
                recipients: BTreeMap::new(),
                b2b: Default::default(),
            },
            safety: None,
            routing: None,
            server: None,
            chains: vec![ChainConfig {
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
            tokens: vec![],
            providers,
        }
    }

    #[test]
    fn rejects_disallowed_env_template_in_provider() {
        let mut provider = minimal_valid_provider("acme", "Acme");
        provider.http = Some(HttpConfig {
            base_url: "https://api.example.com".to_string(),
            default_headers: BTreeMap::from([(
                "x-api-key".to_string(),
                "${env.ODOS_API_KEY}".to_string(),
            )]),
            default_query: BTreeMap::new(),
            auth: None,
            requests: BTreeMap::new(),
            prepare_flow: None,
            quote: None,
            prepare: None,
        });
        let config = minimal_valid_config(vec![provider]);
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("ODOS_API_KEY"),
            "expected disallowed env rejection, got: {err}"
        );
    }

    #[test]
    fn accepts_modern_provider_id_velora_market() {
        let config = minimal_valid_config(vec![minimal_valid_provider(
            "velora-market",
            "Velora Market",
        )]);
        config
            .validate()
            .expect("modern provider should be accepted");
    }

    #[test]
    fn provider_summary_does_not_serialize_request_configuration() {
        let mut provider = minimal_valid_provider("acme", "Acme");
        provider.http = Some(HttpConfig {
            base_url: "https://user:secret@example.test".to_string(),
            default_headers: BTreeMap::from([("authorization".to_string(), "secret".to_string())]),
            default_query: BTreeMap::new(),
            auth: None,
            requests: BTreeMap::new(),
            prepare_flow: None,
            quote: None,
            prepare: None,
        });
        let serialized = serde_json::to_string(&ProviderSummary::from(&provider)).unwrap();
        assert!(!serialized.contains("http"));
        assert!(!serialized.contains("secret"));
        assert!(!serialized.contains("authorization"));
    }

    #[test]
    fn accepts_modern_provider_id_socket_v3() {
        let config =
            minimal_valid_config(vec![minimal_valid_provider("socket-v3", "Socket Swap V3")]);
        config
            .validate()
            .expect("modern provider should be accepted");
    }

    #[test]
    fn rejects_disallowed_env_rango_in_provider() {
        let mut provider = minimal_valid_provider("acme", "Acme");
        provider.http = Some(HttpConfig {
            base_url: "https://api.example.com".to_string(),
            default_headers: BTreeMap::new(),
            default_query: BTreeMap::new(),
            auth: Some(AuthConfig {
                header: None,
                value: None,
                headers: BTreeMap::new(),
                query: BTreeMap::from([("apiKey".to_string(), "${env.RANGO_API_KEY}".to_string())]),
            }),
            requests: BTreeMap::new(),
            prepare_flow: None,
            quote: None,
            prepare: None,
        });
        let config = minimal_valid_config(vec![provider]);
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("RANGO_API_KEY"),
            "expected disallowed env rejection, got: {err}"
        );
    }

    #[test]
    fn rejects_disallowed_env_rhino_fi_jwt_in_provider() {
        let mut provider = minimal_valid_provider("acme", "Acme");
        provider.notes = Some("Gated on ${env.RHINO_FI_JWT} for the bridge auth flow".to_string());
        let config = minimal_valid_config(vec![provider]);
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains("RHINO_FI_JWT"),
            "expected disallowed env rejection, got: {err}"
        );
    }

    #[test]
    fn accepts_literal_provider_notes() {
        let mut provider = minimal_valid_provider("acme", "Acme");
        let disallowed_key = concat!("ECO", "_API", "_KEY");
        let disallowed_url = concat!("ECO", "_API", "_URL");
        provider.notes = Some(format!(
            "Eco gated on {disallowed_key} and {disallowed_url}"
        ));
        let config = minimal_valid_config(vec![provider]);
        config.validate().expect("literal notes are not templates");
    }

    #[test]
    fn rejects_eco_credential_in_body_env_template() {
        // Eco credential env template in body should be rejected.
        let mut provider = minimal_valid_provider("acme", "Acme");
        provider.http = Some(HttpConfig {
            base_url: "https://quotes.eco.com".to_string(),
            default_headers: BTreeMap::new(),
            default_query: BTreeMap::new(),
            auth: None,
            requests: BTreeMap::from([(
                "quote".to_string(),
                EndpointConfig {
                    method: "POST".to_string(),
                    path: "/api/v3/quotes/single".to_string(),
                    query: BTreeMap::new(),
                    headers: BTreeMap::new(),
                    body_json: Some(serde_json::json!({
                        "dAppID": "gulf",
                        "fromChain": "1",
                        "toChain": "8453",
                        "apiKey": concat!("${env.GULF_ECO", "_API", "_KEY}"),
                    })),
                    body_form: BTreeMap::new(),
                    response: None,
                    timeout_ms: None,
                    retry_attempts: None,
                },
            )]),
            prepare_flow: None,
            quote: None,
            prepare: None,
        });
        let config = minimal_valid_config(vec![provider]);
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains(concat!("ECO", "_API", "_KEY")),
            "expected disallowed Eco API key env rejection, got: {err}"
        );
    }

    #[test]
    fn accepts_literal_provider_auth_value() {
        let mut provider = minimal_valid_provider("acme", "Acme");
        provider.http = Some(HttpConfig {
            base_url: "https://quotes.eco.com".to_string(),
            default_headers: BTreeMap::new(),
            default_query: BTreeMap::new(),
            auth: Some(AuthConfig {
                header: Some("x-eco-api-key".to_string()),
                value: Some(concat!("ECO", "_API", "_KEY").to_string()),
                headers: BTreeMap::new(),
                query: BTreeMap::new(),
            }),
            requests: BTreeMap::new(),
            prepare_flow: None,
            quote: None,
            prepare: None,
        });
        let config = minimal_valid_config(vec![provider]);
        config
            .validate()
            .expect("literal auth values are not templates");
    }

    #[test]
    fn rejects_eco_auth_env_header_in_provider() {
        let mut provider = minimal_valid_provider("acme", "Acme");
        provider.http = Some(HttpConfig {
            base_url: "https://quotes.eco.com".to_string(),
            default_headers: BTreeMap::from([(
                "x-eco-api-key".to_string(),
                concat!("${env.GULF_ECO", "_API", "_KEY}").to_string(),
            )]),
            default_query: BTreeMap::new(),
            auth: None,
            requests: BTreeMap::new(),
            prepare_flow: None,
            quote: None,
            prepare: None,
        });
        let config = minimal_valid_config(vec![provider]);
        let err = config.validate().unwrap_err();
        assert!(
            err.to_string().contains(concat!("ECO", "_API", "_KEY")),
            "expected Eco credential env header rejection, got: {err}"
        );
    }

    #[test]
    fn accepts_eco_dappid_literal_in_body_no_env_template() {
        // dAppID as a literal value (not an env template) should pass,
        // demonstrating that a non-runtime Eco test fixture can be parsed.
        let mut provider = minimal_valid_provider("eco-test", "Eco Test");
        provider.http = Some(HttpConfig {
            base_url: "https://quotes.eco.com".to_string(),
            default_headers: BTreeMap::new(),
            default_query: BTreeMap::new(),
            auth: None,
            requests: BTreeMap::from([(
                "quote".to_string(),
                EndpointConfig {
                    method: "POST".to_string(),
                    path: "/api/v3/quotes/single".to_string(),
                    query: BTreeMap::new(),
                    headers: BTreeMap::new(),
                    body_json: Some(serde_json::json!({
                        "dAppID": "test-fixture",
                        "fromChain": "1",
                        "toChain": "8453"
                    })),
                    body_form: BTreeMap::new(),
                    response: None,
                    timeout_ms: None,
                    retry_attempts: None,
                },
            )]),
            prepare_flow: None,
            quote: None,
            prepare: None,
        });
        let config = minimal_valid_config(vec![provider]);
        config
            .validate()
            .expect("Eco literal dAppID fixture should be accepted as non-runtime test config");
    }

    #[test]
    fn accepts_eco_dappid_env_template_in_body() {
        // Eco dAppID env template ${env.GULF_ECO_DAPP_ID} is now
        // allowed (non-secret) and should pass validation.
        let mut provider = minimal_valid_provider("eco-routes", "Eco Routes");
        provider.http = Some(HttpConfig {
            base_url: "https://quotes.eco.com".to_string(),
            default_headers: BTreeMap::new(),
            default_query: BTreeMap::new(),
            auth: None,
            requests: BTreeMap::from([(
                "quote".to_string(),
                EndpointConfig {
                    method: "POST".to_string(),
                    path: "/api/v3/quotes/exactIn".to_string(),
                    query: BTreeMap::new(),
                    headers: BTreeMap::new(),
                    body_json: Some(serde_json::json!({
                        "dAppID": concat!("${env.GULF_ECO", "_DAPP", "_ID}"),
                        "intentExecutionTypes": ["SELF_PUBLISH"],
                        "quoteRequest": {
                            "sourceChainID": 8453,
                            "destinationChainID": 42161,
                            "sourceToken": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
                            "destinationToken": "0xaf88d065e77c8cC2239327C5EDb3A432268e5831",
                            "sourceAmount": "100000",
                            "funder": "${sender}",
                            "recipient": "${recipient}",
                            "refundRecipient": "${sender}",
                        }
                    })),
                    body_form: BTreeMap::new(),
                    response: None,
                    timeout_ms: None,
                    retry_attempts: None,
                },
            )]),
            prepare_flow: None,
            quote: None,
            prepare: None,
        });
        let config = minimal_valid_config(vec![provider]);
        config
            .validate()
            .expect("Eco dAppID env template should be accepted");
    }
}
