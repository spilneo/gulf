use crate::config::{ChainConfig, RpcEndpointConfig};
use anyhow::{Context, Result, bail};
use reqwest::Client;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ResolvedRpcEndpoint {
    pub name: String,
    pub url: String,
    pub priority: u16,
    pub timeout_ms: Option<u64>,
}

pub fn resolve_rpc_endpoints(chain: &ChainConfig) -> Vec<ResolvedRpcEndpoint> {
    let mut endpoints = chain
        .rpc_endpoints
        .iter()
        .filter(|endpoint| endpoint.enabled)
        .filter_map(resolve_rpc_endpoint)
        .collect::<Vec<_>>();

    endpoints.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| a.name.cmp(&b.name))
    });
    endpoints
}

pub async fn call_json_rpc_with_failover(
    client: &Client,
    chain: &ChainConfig,
    payload: &Value,
) -> Result<Value> {
    let endpoints = resolve_rpc_endpoints(chain);
    if endpoints.is_empty() {
        bail!("chain {} has no enabled RPC endpoints", chain.caip2_id);
    }

    let mut last_error = None;
    for endpoint in endpoints {
        let mut request = client.post(&endpoint.url).json(payload);
        if let Some(timeout_ms) = endpoint.timeout_ms {
            request = request.timeout(Duration::from_millis(timeout_ms));
        }

        match request.send().await {
            Ok(response) => match response.error_for_status() {
                Ok(response) => return Ok(response.json().await?),
                Err(error) => last_error = Some(error.into()),
            },
            Err(error) => last_error = Some(error.into()),
        }
    }

    Err(last_error.context("all configured RPC endpoints failed")?)
}

fn resolve_rpc_endpoint(endpoint: &RpcEndpointConfig) -> Option<ResolvedRpcEndpoint> {
    let url = endpoint.url.clone().or_else(|| {
        endpoint
            .url_env
            .as_ref()
            .and_then(|key| std::env::var(key).ok())
    })?;

    Some(ResolvedRpcEndpoint {
        name: endpoint.name.clone(),
        url,
        priority: endpoint.priority,
        timeout_ms: endpoint.timeout_ms,
    })
}
