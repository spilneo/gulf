mod adapters;
pub mod amount;
pub mod error;
pub mod execution;
pub mod generic_http;
pub mod request;
pub mod response;
pub mod template;

use crate::{
    config::{ChainConfig, EndpointConfig, GulfConfig, ProviderConfig, ServiceType, TokenConfig},
    model::{
        ExecutionKind, ExecutionPlan, NormalizedQuote, NormalizedTransaction, PrepareSwapRequest,
        PreparedSwap, QuoteRequest, QuoteResult,
    },
    rpc,
};
use anyhow::{Context, Result, bail};
use serde::Serialize;

use self::{
    adapters::{
        is_chainflip_broker_provider, is_changenow_provider, is_circle_cctp_provider,
        is_meson_provider, is_rhino_fi_provider, is_squid_provider, is_thorchain_provider,
    },
    execution::{find_first_evm_action, has_transaction},
};

// ---------------------------------------------------------------------------
// Public API (re-exported from submodules as needed)
// ---------------------------------------------------------------------------

/// Serialize a value to a JSON [`serde_json::Value`].
pub fn to_json_value<T: Serialize>(value: T) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(Into::into)
}

// ---------------------------------------------------------------------------
// Shared internal structs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub(super) struct EndpointCall {
    pub(super) raw: serde_json::Value,
    pub(super) normalized_quote: Option<NormalizedQuote>,
    pub(super) normalized_transaction: Option<NormalizedTransaction>,
    pub(super) normalized_execution_plan: Option<ExecutionPlan>,
}

#[derive(Debug, Clone, Default)]
pub(super) struct PartnerFee {
    pub(super) partner_id: Option<String>,
    pub(super) partner_fee_bps: f64,
    pub(super) partner_recipient: String,
}

#[derive(Debug, Clone)]
pub(super) struct RevenueShare {
    pub(super) owner_fee_bps: f64,
    pub(super) partner_fee_bps: f64,
    pub(super) integrator_fee_bps: f64,
    pub(super) partner_id: String,
    pub(super) partner_recipient: String,
}

// ---------------------------------------------------------------------------
// ProviderRegistry
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct ProviderRegistry {
    pub(super) config: GulfConfig,
    pub(super) client: reqwest::Client,
}

impl ProviderRegistry {
    pub fn new(config: GulfConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self {
            config,
            client: reqwest::Client::new(),
        })
    }

    pub fn sorted_providers(&self) -> Vec<ProviderConfig> {
        let mut providers = self
            .config
            .providers
            .iter()
            .filter(|provider| provider.enabled)
            .cloned()
            .collect::<Vec<_>>();

        providers.sort_by(|a, b| {
            let left = a.fee.estimated_bps.unwrap_or(f64::MAX);
            let right = b.fee.estimated_bps.unwrap_or(f64::MAX);
            left.total_cmp(&right).then_with(|| a.name.cmp(&b.name))
        });

        providers
    }

    pub fn chains(&self) -> Vec<ChainConfig> {
        self.config.chains.clone()
    }

    pub fn tokens(&self) -> Vec<TokenConfig> {
        self.config.tokens.clone()
    }

    pub(crate) fn config(&self) -> &GulfConfig {
        &self.config
    }

    pub fn rpc_endpoints(&self, chain: &str) -> Result<Vec<rpc::ResolvedRpcEndpoint>> {
        let chain = self
            .config
            .find_chain(chain)
            .with_context(|| format!("unknown chain {chain}"))?;
        Ok(rpc::resolve_rpc_endpoints(chain))
    }

    pub async fn call_rpc(
        &self,
        chain: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        let chain = self
            .config
            .find_chain(chain)
            .with_context(|| format!("unknown chain {chain}"))?;
        rpc::call_json_rpc_with_failover(&self.client, chain, payload).await
    }

    pub async fn quote_all(
        &self,
        request: &QuoteRequest,
        force_live: bool,
    ) -> Result<Vec<QuoteResult>> {
        let mut results = Vec::new();
        let partner_fee = self.partner_fee(request)?;
        for provider in self.sorted_providers() {
            if !self.supports_route(&provider, request) {
                continue;
            }
            let revenue = self.revenue_for_provider(&provider, &partner_fee);
            if !self.provider_accepts_revenue(&provider, revenue.integrator_fee_bps) {
                continue;
            }

            let (normalized, live_response) = if request.live || force_live {
                quote_response_fields(
                    self.call_endpoint(&provider, self.endpoint(&provider, "quote"), request)
                        .await,
                )
            } else {
                (None, None)
            };

            let estimated_fee_bps = provider.fee.estimated_bps.unwrap_or_default();

            results.push(QuoteResult {
                provider_id: provider.id,
                provider_name: provider.name,
                estimated_fee_bps,
                commission_bps: revenue.integrator_fee_bps,
                protocol_fee_bps: estimated_fee_bps,
                owner_fee_bps: revenue.owner_fee_bps,
                partner_fee_bps: revenue.partner_fee_bps,
                integrator_fee_bps: revenue.integrator_fee_bps,
                total_configured_bps: estimated_fee_bps + revenue.integrator_fee_bps,
                dynamic_fee: provider.fee.dynamic,
                settlement_seconds: provider.fee.settlement_seconds,
                docs: provider.docs,
                normalized,
                live_response,
            });
        }

        results.sort_by(|a, b| {
            a.total_configured_bps
                .total_cmp(&b.total_configured_bps)
                .then_with(|| a.provider_name.cmp(&b.provider_name))
        });
        Ok(results)
    }

    pub async fn prepare_swap(&self, request: &PrepareSwapRequest) -> Result<PreparedSwap> {
        if self
            .config
            .safety
            .as_ref()
            .is_some_and(|safety| safety.require_live_quote_for_swap)
            && !request.quote.live
        {
            bail!("safety.require_live_quote_for_swap requires quote.live=true");
        }
        let provider = match &request.provider_id {
            Some(id) => self
                .sorted_providers()
                .into_iter()
                .find(|provider| provider.id == *id)
                .with_context(|| format!("provider {id} is not enabled or does not exist"))?,
            None => self
                .sorted_providers()
                .into_iter()
                .find(|provider| self.supports_route(provider, &request.quote))
                .context("no enabled provider supports the requested route")?,
        };
        let partner_fee = self.partner_fee(&request.quote)?;
        let revenue = self.revenue_for_provider(&provider, &partner_fee);
        if !self.provider_accepts_revenue(&provider, revenue.integrator_fee_bps) {
            bail!(
                "provider {} does not allow the requested total integrator fee of {} bps",
                provider.id,
                revenue.integrator_fee_bps
            );
        }

        let call = if is_circle_cctp_provider(&provider.id) {
            self.prepare_circle_cctp(&provider, &request.quote)
                .await
                .with_context(|| {
                    format!("provider {} could not prepare a CCTP swap", provider.id)
                })?
        } else if is_thorchain_provider(&provider.id) {
            self.prepare_thorchain(&provider, &request.quote)
                .await
                .with_context(|| {
                    format!(
                        "provider {} could not prepare a THORChain swap",
                        provider.id
                    )
                })?
        } else if is_chainflip_broker_provider(&provider.id) {
            self.prepare_chainflip(&provider, &request.quote)
                .await
                .with_context(|| {
                    format!(
                        "provider {} could not prepare a Chainflip swap",
                        provider.id
                    )
                })?
        } else if is_changenow_provider(&provider.id) {
            self.prepare_changenow(&provider, &request.quote)
                .await
                .with_context(|| {
                    format!(
                        "provider {} could not prepare a ChangeNOW exchange",
                        provider.id
                    )
                })?
        } else if is_rhino_fi_provider(&provider.id) {
            self.prepare_rhino(&provider, &request.quote)
                .await
                .with_context(|| {
                    format!("provider {} could not prepare a Rhino swap", provider.id)
                })?
        } else if is_meson_provider(&provider.id) {
            self.prepare_meson(&provider, &request.quote)
                .await
                .with_context(|| {
                    format!("provider {} could not prepare a Meson swap", provider.id)
                })?
        } else if is_squid_provider(&provider.id) {
            self.prepare_squid(&provider, &request.quote)
                .await
                .with_context(|| {
                    format!("provider {} could not prepare a Squid swap", provider.id)
                })?
        } else {
            self.call_prepare(&provider, &request.quote)
                .await
                .with_context(|| format!("provider {} could not prepare a swap", provider.id))?
        };

        let execution = call
            .normalized_execution_plan
            .unwrap_or_else(|| ExecutionPlan {
                kind: ExecutionKind::Unsupported,
                actions: vec![],
                status: None,
                expires_at: None,
                warnings: vec!["no executable action found".to_string()],
            });

        let normalized_transaction = call
            .normalized_transaction
            .clone()
            .filter(has_transaction)
            .or_else(|| {
                find_first_evm_action(&execution.actions).map(|action| NormalizedTransaction {
                    to: action.to.clone(),
                    data: action.data.clone(),
                    value: action.value.clone(),
                })
            });

        let raw = call.raw.clone();

        Ok(PreparedSwap {
            provider_id: provider.id,
            provider_name: provider.name,
            request: request.quote.clone(),
            normalized_transaction,
            execution,
            transaction: raw.clone(),
            raw,
            docs: provider.docs,
        })
    }

    // -----------------------------------------------------------------------
    // Internal helpers (pub(super) for use by submodules)
    // -----------------------------------------------------------------------

    pub(super) fn endpoint<'a>(
        &self,
        provider: &'a ProviderConfig,
        name: &str,
    ) -> Option<&'a EndpointConfig> {
        provider.http.as_ref().and_then(|http| {
            http.requests.get(name).or(match name {
                "quote" => http.quote.as_ref(),
                "prepare" => http.prepare.as_ref(),
                _ => None,
            })
        })
    }

    pub(super) fn supports_route(&self, provider: &ProviderConfig, request: &QuoteRequest) -> bool {
        let from_chain = self.config.find_chain(&request.from_chain);
        let to_chain = self.config.find_chain(&request.to_chain);

        if self
            .config
            .routing
            .as_ref()
            .is_some_and(|routing| routing.require_chain_registry_match)
            && (from_chain.is_none() || to_chain.is_none())
        {
            return false;
        }

        let from_token = from_chain
            .and_then(|chain| self.config.find_token(&request.from_token, &chain.caip2_id));
        let to_token =
            to_chain.and_then(|chain| self.config.find_token(&request.to_token, &chain.caip2_id));

        if self
            .config
            .routing
            .as_ref()
            .is_some_and(|routing| routing.require_token_registry_match)
            && (from_token.is_none() || to_token.is_none())
        {
            return false;
        }

        if !provider.supported_tokens.is_empty() {
            let Some(from_token) = from_token else {
                return false;
            };
            let Some(to_token) = to_token else {
                return false;
            };
            if !provider
                .supported_tokens
                .iter()
                .any(|token| token.eq_ignore_ascii_case(&from_token.id))
                || !provider
                    .supported_tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case(&to_token.id))
            {
                return false;
            }
        }

        let Some(from_chain) = from_chain else {
            return request::supports_route_by_name(provider, request);
        };
        let Some(to_chain) = to_chain else {
            return request::supports_route_by_name(provider, request);
        };

        if matches!(provider.service_type, ServiceType::DexAggregator)
            && from_chain.caip2_id != to_chain.caip2_id
        {
            return false;
        }

        provider
            .supported_chains
            .iter()
            .any(|chain| chain == &from_chain.caip2_id)
            && provider
                .supported_chains
                .iter()
                .any(|chain| chain == &to_chain.caip2_id)
    }

    pub(super) fn partner_fee(&self, request: &QuoteRequest) -> Result<PartnerFee> {
        let Some(partner) = &request.partner_fee else {
            return Ok(PartnerFee::default());
        };
        if partner.partner_id.trim().is_empty() {
            bail!("partner_fee.partner_id cannot be empty");
        }

        let policy = &self.config.developer.b2b;
        if !policy.enabled {
            bail!("developer.b2b.enabled must be true before partner fees can be requested");
        }
        let partner_config = policy.partners.get(&partner.partner_id);
        if partner_config.is_some_and(|config| !config.enabled) {
            bail!("partner {} is disabled", partner.partner_id);
        }

        let requested_bps = partner
            .fee_bps
            .or_else(|| partner_config.and_then(|config| config.default_fee_bps))
            .unwrap_or(policy.default_partner_fee_bps);
        if !(0.0..=10_000.0).contains(&requested_bps) {
            bail!("partner fee must be between 0 and 10000 bps");
        }

        let max_bps = partner_config
            .and_then(|config| config.max_fee_bps)
            .or(policy.max_partner_fee_bps)
            .unwrap_or(0.0);
        if requested_bps > max_bps {
            bail!(
                "partner fee {} bps exceeds configured maximum {} bps",
                requested_bps,
                max_bps
            );
        }

        let recipient = partner
            .recipient
            .clone()
            .or_else(|| partner_config.and_then(|config| config.recipient.clone()));
        if policy.require_partner_recipient
            && recipient
                .as_ref()
                .is_none_or(|value| value.trim().is_empty())
        {
            bail!("partner fee recipient is required by developer.b2b.require_partner_recipient");
        }

        Ok(PartnerFee {
            partner_id: Some(partner.partner_id.clone()),
            partner_fee_bps: requested_bps,
            partner_recipient: recipient.unwrap_or_default(),
        })
    }

    pub(super) fn revenue_for_provider(
        &self,
        provider: &ProviderConfig,
        partner_fee: &PartnerFee,
    ) -> RevenueShare {
        let owner_fee_bps = if provider.commission.supported {
            provider
                .commission
                .default_bps
                .unwrap_or(self.config.developer.default_commission_bps)
        } else {
            provider.commission.default_bps.unwrap_or_default()
        };
        let integrator_fee_bps = owner_fee_bps + partner_fee.partner_fee_bps;
        RevenueShare {
            owner_fee_bps,
            partner_fee_bps: partner_fee.partner_fee_bps,
            integrator_fee_bps,
            partner_id: partner_fee.partner_id.clone().unwrap_or_default(),
            partner_recipient: partner_fee.partner_recipient.clone(),
        }
    }

    pub(super) fn provider_accepts_revenue(
        &self,
        provider: &ProviderConfig,
        integrator_fee_bps: f64,
    ) -> bool {
        if !provider.commission.supported
            && integrator_fee_bps > provider.commission.default_bps.unwrap_or_default()
        {
            return false;
        }
        if self
            .config
            .safety
            .as_ref()
            .and_then(|safety| safety.max_commission_bps)
            .is_some_and(|max| integrator_fee_bps > max)
        {
            return false;
        }
        provider
            .commission
            .max_bps
            .is_none_or(|max| integrator_fee_bps <= max)
    }
}

/// Convert a quote endpoint result into the public live-quote fields. Endpoint
/// failures are data for an all-provider quote response, rather than a reason
/// to omit a supported provider from that response.
fn quote_response_fields(
    call: Result<EndpointCall>,
) -> (Option<NormalizedQuote>, Option<serde_json::Value>) {
    match call {
        Ok(call) => (call.normalized_quote, Some(call.raw)),
        Err(error) => (
            Some(NormalizedQuote {
                output_amount: None,
                estimated_fee_bps: None,
                estimated_fee_usd: None,
                gas_fee_usd: None,
                settlement_seconds: None,
                error: Some(error::redact_response_body(&error.to_string())),
            }),
            None,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CommissionConfig, FeeConfig, SafetyConfig};

    #[test]
    fn live_endpoint_failure_is_a_normalized_quote_error() {
        let (normalized, live_response) =
            quote_response_fields(Err(anyhow::anyhow!("HTTP 503 unavailable")));

        assert!(live_response.is_none());
        let normalized = normalized.expect("live failures must retain an error quote");
        assert_eq!(normalized.error.as_deref(), Some("HTTP 503 unavailable"));
        assert!(normalized.output_amount.is_none());
    }

    #[test]
    fn offline_quote_fields_remain_empty() {
        let fields: (Option<NormalizedQuote>, Option<serde_json::Value>) = (None, None);
        assert!(fields.0.is_none());
        assert!(fields.1.is_none());
    }

    #[tokio::test]
    async fn live_quote_safety_rejects_offline_swap_preparation() {
        let mut config = GulfConfig {
            project: None,
            developer: crate::config::DeveloperConfig {
                default_commission_bps: 0.0,
                recipients: Default::default(),
                b2b: Default::default(),
            },
            safety: Some(SafetyConfig {
                max_commission_bps: None,
                require_live_quote_for_swap: true,
            }),
            routing: None,
            server: None,
            chains: vec![ChainConfig {
                caip2_id: "eip155:1".into(),
                display_name: "Ethereum".into(),
                vm: "evm".into(),
                aliases: vec![],
                numeric_chain_id: Some(1),
                native_symbol: Some("ETH".into()),
                rpc_endpoints: vec![],
                provider_references: Default::default(),
                explorer_url: None,
            }],
            tokens: vec![],
            providers: vec![],
        };
        config.providers.push(ProviderConfig {
            id: "test".into(),
            name: "Test".into(),
            enabled: true,
            service_type: ServiceType::DexAggregator,
            supported_chains: vec!["eip155:1".into()],
            supported_tokens: vec![],
            fee: FeeConfig {
                unit: crate::config::FeeUnit::Bps,
                dynamic: false,
                estimated_bps: None,
                flat_usd: None,
                settlement_seconds: None,
                notes: None,
            },
            commission: CommissionConfig {
                supported: false,
                strategy: "none".into(),
                default_bps: None,
                max_bps: None,
                parameters: Default::default(),
                notes: None,
            },
            docs: vec![],
            http: None,
            blockchain: None,
            notes: None,
        });
        let registry = ProviderRegistry::new(config).unwrap();
        let error = registry
            .prepare_swap(&PrepareSwapRequest {
                quote: QuoteRequest {
                    from_chain: "eip155:1".into(),
                    to_chain: "eip155:1".into(),
                    from_token: "ETH".into(),
                    to_token: "ETH".into(),
                    amount: "1".into(),
                    sender: "sender".into(),
                    recipient: None,
                    live: false,
                    partner_fee: None,
                },
                provider_id: Some("test".into()),
            })
            .await
            .unwrap_err();
        assert!(error.to_string().contains("require_live_quote_for_swap"));
    }
}
