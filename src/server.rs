use crate::{
    config::ProviderSummary,
    model::{PartnerFeeRequest, PrepareSwapRequest, QuoteRequest},
    provider::ProviderRegistry,
};
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{
        DefaultBodyLimit, Path, Query, State,
        rejection::{JsonRejection, QueryRejection},
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use serde::Deserialize;
use std::net::SocketAddr;

#[derive(Debug, Deserialize)]
struct QuoteQuery {
    from_chain: String,
    to_chain: String,
    from_token: String,
    to_token: String,
    amount: String,
    sender: String,
    recipient: Option<String>,
    live: Option<bool>,
    partner_id: Option<String>,
    partner_fee_bps: Option<f64>,
    partner_recipient: Option<String>,
}

pub async fn serve(listen: SocketAddr, registry: ProviderRegistry) -> Result<()> {
    let app = router(registry);

    let listener = tokio::net::TcpListener::bind(listen).await?;
    tracing::info!("gulf API listening on http://{listen}");
    axum::serve(listener, app).await?;
    Ok(())
}

pub fn router(registry: ProviderRegistry) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async { Json(serde_json::json!({ "status": "ok" })) }),
        )
        .route("/v1/providers", get(providers))
        .route("/v1/chains", get(chains))
        .route("/v1/tokens", get(tokens))
        .route("/v1/rpc/{chain}", post(rpc_call))
        .route("/v1/quotes", get(quotes).post(quotes_post))
        .route("/v1/swaps/prepare", post(prepare_swap))
        .fallback(not_found)
        .method_not_allowed_fallback(method_not_allowed)
        .layer(DefaultBodyLimit::max(1024 * 1024))
        .with_state(registry)
}

async fn providers(State(registry): State<ProviderRegistry>) -> Json<serde_json::Value> {
    let providers = registry
        .sorted_providers()
        .iter()
        .map(ProviderSummary::from)
        .collect::<Vec<_>>();
    Json(serde_json::json!({ "providers": providers }))
}

async fn chains(State(registry): State<ProviderRegistry>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "chains": registry.chains() }))
}

async fn tokens(State(registry): State<ProviderRegistry>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "tokens": registry.tokens() }))
}

async fn rpc_call(
    State(registry): State<ProviderRegistry>,
    Path(chain): Path<String>,
    payload: Result<Json<serde_json::Value>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Json(payload) = payload.map_err(|_| ApiError)?;
    validate_rpc_request(&payload)?;
    let response = registry.call_rpc(&chain, &payload).await?;
    Ok(Json(response))
}

async fn quotes(
    State(registry): State<ProviderRegistry>,
    query: Result<Query<QuoteQuery>, QueryRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Query(query) = query.map_err(|_| ApiError)?;
    let request = QuoteRequest {
        from_chain: query.from_chain,
        to_chain: query.to_chain,
        from_token: query.from_token,
        to_token: query.to_token,
        amount: query.amount,
        sender: query.sender,
        recipient: query.recipient,
        live: query.live.unwrap_or(false),
        partner_fee: query.partner_id.map(|partner_id| PartnerFeeRequest {
            partner_id,
            fee_bps: query.partner_fee_bps,
            recipient: query.partner_recipient,
            metadata: Default::default(),
        }),
    };
    let quotes = registry.quote_all(&request, false).await?;
    Ok(Json(serde_json::json!({ "quotes": quotes })))
}

async fn quotes_post(
    State(registry): State<ProviderRegistry>,
    request: Result<Json<QuoteRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Json(request) = request.map_err(|_| ApiError)?;
    let quotes = registry.quote_all(&request, false).await?;
    Ok(Json(serde_json::json!({ "quotes": quotes })))
}

async fn prepare_swap(
    State(registry): State<ProviderRegistry>,
    request: Result<Json<PrepareSwapRequest>, JsonRejection>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let Json(request) = request.map_err(|_| ApiError)?;
    let prepared = registry.prepare_swap(&request).await?;
    Ok(Json(serde_json::json!({ "swap": prepared })))
}

struct ApiError;

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(_: E) -> Self {
        Self
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        let status = StatusCode::BAD_REQUEST;
        let body = api_error("bad_request", "request could not be processed");
        (status, body).into_response()
    }
}

async fn not_found() -> impl IntoResponse {
    (
        StatusCode::NOT_FOUND,
        api_error("not_found", "route not found"),
    )
}

async fn method_not_allowed() -> impl IntoResponse {
    (
        StatusCode::METHOD_NOT_ALLOWED,
        api_error("method_not_allowed", "method not allowed"),
    )
}

fn api_error(code: &str, message: &str) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "error": { "code": code, "message": message } }))
}

fn validate_rpc_request(payload: &serde_json::Value) -> Result<(), ApiError> {
    let Some(object) = payload.as_object() else {
        return Err(ApiError);
    };
    let Some(method) = object.get("method").and_then(serde_json::Value::as_str) else {
        return Err(ApiError);
    };
    const ALLOWED_METHODS: &[&str] = &[
        "eth_chainId",
        "eth_blockNumber",
        "eth_getBalance",
        "eth_call",
        "eth_estimateGas",
        "eth_getCode",
        "eth_getTransactionCount",
        "eth_getTransactionByHash",
        "eth_getTransactionReceipt",
        "eth_sendRawTransaction",
    ];
    if ALLOWED_METHODS.contains(&method) {
        Ok(())
    } else {
        Err(ApiError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ChainConfig, CommissionConfig, DeveloperConfig, FeeConfig, FeeUnit, GulfConfig,
        ProviderConfig, ServiceType,
    };
    use axum::{body::Body, http::Request};
    use std::collections::BTreeMap;
    use tower::ServiceExt;

    fn app() -> Router {
        let config = GulfConfig {
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
                caip2_id: "eip155:1".into(),
                display_name: "Ethereum".into(),
                vm: "evm".into(),
                aliases: vec![],
                numeric_chain_id: Some(1),
                native_symbol: Some("ETH".into()),
                rpc_endpoints: vec![],
                provider_references: BTreeMap::new(),
                explorer_url: None,
            }],
            tokens: vec![],
            providers: vec![ProviderConfig {
                id: "test".into(),
                name: "Test".into(),
                enabled: true,
                service_type: ServiceType::DexAggregator,
                supported_chains: vec!["eip155:1".into()],
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
                    strategy: "none".into(),
                    default_bps: None,
                    max_bps: None,
                    parameters: BTreeMap::new(),
                    notes: None,
                },
                docs: vec![],
                http: None,
                blockchain: None,
                notes: None,
            }],
        };
        router(ProviderRegistry::new(config).unwrap())
    }

    async fn assert_bad_request(request: Request<Body>) {
        let response = app().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
            serde_json::json!({"error": {"code": "bad_request", "message": "request could not be processed"}})
        );
    }

    #[tokio::test]
    async fn malformed_json_uses_the_api_error_envelope() {
        assert_bad_request(
            Request::post("/v1/quotes")
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .unwrap(),
        )
        .await;
    }

    #[tokio::test]
    async fn missing_query_field_uses_the_api_error_envelope() {
        assert_bad_request(
            Request::get("/v1/quotes?from_chain=base")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    }

    #[tokio::test]
    async fn unsupported_rpc_method_is_rejected_before_forwarding() {
        assert_bad_request(
            Request::post("/v1/rpc/eip155:1")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"jsonrpc":"2.0","id":1,"method":"eth_sendTransaction","params":[]}"#,
                ))
                .unwrap(),
        )
        .await;
    }

    #[test]
    fn rpc_validation_rejects_batches_and_accepts_raw_transaction_submission() {
        assert!(validate_rpc_request(&serde_json::json!([{"method": "eth_blockNumber"}])).is_err());
        assert!(
            validate_rpc_request(&serde_json::json!({"method": "eth_sendRawTransaction"})).is_ok()
        );
    }
}
