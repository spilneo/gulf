use crate::{
    config::{ChainConfig, GulfConfig, ProviderSummary, TokenConfig},
    model::{
        ExecuteSwapRequest, ExecuteSwapResult, PrepareSwapRequest, PreparedSwap, QuoteRequest,
        QuoteResult,
    },
    provider::ProviderRegistry,
    rpc::ResolvedRpcEndpoint,
    server,
};
use anyhow::Result;
use std::net::SocketAddr;

#[derive(Clone)]
pub struct Gulf {
    registry: ProviderRegistry,
}

impl Gulf {
    pub async fn from_source(source: impl AsRef<str>) -> Result<Self> {
        Self::from_config(GulfConfig::load(source.as_ref()).await?)
    }

    pub fn from_config(config: GulfConfig) -> Result<Self> {
        Ok(Self {
            registry: ProviderRegistry::new(config)?,
        })
    }

    pub fn registry(&self) -> &ProviderRegistry {
        &self.registry
    }

    pub fn providers(&self) -> Vec<ProviderSummary> {
        self.registry
            .sorted_providers()
            .iter()
            .map(ProviderSummary::from)
            .collect()
    }

    pub fn chains(&self) -> Vec<ChainConfig> {
        self.registry.chains()
    }

    pub fn tokens(&self) -> Vec<TokenConfig> {
        self.registry.tokens()
    }

    pub fn rpc_endpoints(&self, chain: &str) -> Result<Vec<ResolvedRpcEndpoint>> {
        self.registry.rpc_endpoints(chain)
    }

    pub async fn call_rpc(
        &self,
        chain: &str,
        payload: &serde_json::Value,
    ) -> Result<serde_json::Value> {
        self.registry.call_rpc(chain, payload).await
    }

    pub async fn quote(&self, request: QuoteRequest) -> Result<Vec<QuoteResult>> {
        self.registry.quote_all(&request, false).await
    }

    pub async fn live_quote(&self, mut request: QuoteRequest) -> Result<Vec<QuoteResult>> {
        request.live = true;
        self.registry.quote_all(&request, true).await
    }

    pub async fn prepare_swap(&self, request: PrepareSwapRequest) -> Result<PreparedSwap> {
        self.registry.prepare_swap(&request).await
    }

    pub async fn execute(&self, request: ExecuteSwapRequest) -> Result<ExecuteSwapResult> {
        let prepared = self
            .registry
            .prepare_swap(&PrepareSwapRequest {
                quote: request.quote,
                provider_id: request.provider_id,
            })
            .await?;
        crate::execution::execute(self.registry.config(), prepared, &request.private_key_env).await
    }

    pub async fn serve(self, listen: SocketAddr) -> Result<()> {
        server::serve(listen, self.registry).await
    }
}
