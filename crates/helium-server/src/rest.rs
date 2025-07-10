//! REST Gateway for Cosmos SDK compatible API
//!
//! This module provides REST endpoints that translate to gRPC calls,
//! following the Cosmos SDK REST API specifications.

use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::Json,
    routing::{get, post},
    Router,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tonic::Request;
use tower_http::{
    cors::{Any, CorsLayer},
    limit::RequestBodyLimitLayer,
    timeout::TimeoutLayer,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};
use tracing::{debug, error, info};

use crate::grpc::services::{AuthQueryService, BankQueryService, TxService as GrpcTxService};
use crate::grpc::{
    auth::{self, Query as AuthQuery},
    bank::{self, Query as BankQuery},
    tx::{self, Service as TxService},
};

/// REST Gateway configuration
#[derive(Clone)]
pub struct RestGatewayConfig {
    /// gRPC endpoint to connect to
    pub grpc_endpoint: String,
    /// Enable request logging
    pub enable_logging: bool,
}

impl Default for RestGatewayConfig {
    fn default() -> Self {
        Self {
            grpc_endpoint: "http://[::1]:9090".to_string(),
            enable_logging: true,
        }
    }
}

/// REST Gateway state
#[derive(Clone)]
pub struct RestGatewayState {
    config: RestGatewayConfig,
    bank_service: Arc<BankQueryService>,
    auth_service: Arc<AuthQueryService>,
    tx_service: Arc<GrpcTxService>,
}

impl RestGatewayState {
    /// Create new REST gateway state with actual gRPC services
    pub async fn new(config: RestGatewayConfig) -> Result<Self, Box<dyn std::error::Error>> {
        // Initialize gRPC services
        let bank_service = Arc::new(BankQueryService::new());
        let auth_service = Arc::new(AuthQueryService::new());
        let tx_service = Arc::new(GrpcTxService::new());

        // Populate with some sample data for testing
        bank_service
            .set_balance("cosmos1test", "stake", 1000000)
            .await;
        bank_service
            .set_balance("cosmos1test", "atom", 500000)
            .await;

        // Add a sample account
        auth_service
            .set_account(crate::grpc::BaseAccount {
                address: "cosmos1test".to_string(),
                pub_key: None,
                account_number: 1,
                sequence: 0,
            })
            .await;

        info!("REST Gateway initialized with real gRPC services");

        Ok(Self {
            config,
            bank_service,
            auth_service,
            tx_service,
        })
    }

    /// Create new REST gateway state with custom services
    pub fn with_services(
        config: RestGatewayConfig,
        bank_service: Arc<BankQueryService>,
        auth_service: Arc<AuthQueryService>,
        tx_service: Arc<GrpcTxService>,
    ) -> Self {
        Self {
            config,
            bank_service,
            auth_service,
            tx_service,
        }
    }
}

/// Error response structure
#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub code: u16,
}

/// Pagination parameters
#[derive(Deserialize)]
pub struct PaginationParams {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub count_total: Option<bool>,
}

impl Default for PaginationParams {
    fn default() -> Self {
        Self {
            limit: Some(100),
            offset: Some(0),
            count_total: Some(false),
        }
    }
}

// Bank REST endpoints

/// Balance response
#[derive(Serialize, Deserialize)]
pub struct BalanceResponse {
    pub balance: Coin,
}

/// Coin structure
#[derive(Serialize, Deserialize)]
pub struct Coin {
    pub denom: String,
    pub amount: String,
}

/// All balances response
#[derive(Serialize, Deserialize)]
pub struct AllBalancesResponse {
    pub balances: Vec<Coin>,
    pub pagination: Option<PageResponse>,
}

/// Pagination response
#[derive(Serialize, Deserialize)]
pub struct PageResponse {
    pub next_key: Option<String>,
    pub total: Option<String>,
}

/// Get balance for a specific denom
async fn get_balance(
    Path((address, denom)): Path<(String, String)>,
    headers: HeaderMap,
    State(state): State<Arc<RestGatewayState>>,
) -> Result<Json<BalanceResponse>, (StatusCode, Json<ErrorResponse>)> {
    if state.config.enable_logging {
        debug!("REST: Getting balance for {} denom {}", address, denom);
    }

    // Check for x-cosmos-block-height header
    let block_height = headers
        .get("x-cosmos-block-height")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.parse::<i64>().ok());

    if let Some(height) = block_height {
        debug!("REST: Using historical height {}", height);
    }

    // Call the gRPC bank service
    let mut request = Request::new(bank::QueryBalanceRequest {
        address: address.clone(),
        denom: denom.clone(),
    });

    // Add block height to gRPC metadata if provided
    if let Some(height) = block_height {
        request.metadata_mut().insert(
            "x-cosmos-block-height",
            height.to_string().parse().unwrap(),
        );
    }

    match state.bank_service.as_ref().balance(request).await {
        Ok(response) => {
            let grpc_response = response.into_inner();
            let balance = grpc_response
                .balance
                .map(|coin| Coin {
                    denom: coin.denom,
                    amount: coin.amount,
                })
                .unwrap_or_else(|| Coin {
                    denom,
                    amount: "0".to_string(),
                });

            Ok(Json(BalanceResponse { balance }))
        }
        Err(e) => {
            error!("gRPC balance query failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Balance query failed: {e}"),
                    code: 500,
                }),
            ))
        }
    }
}

/// Get all balances for an address
async fn get_all_balances(
    Path(address): Path<String>,
    Query(pagination): Query<PaginationParams>,
    headers: HeaderMap,
    State(state): State<Arc<RestGatewayState>>,
) -> Result<Json<AllBalancesResponse>, (StatusCode, Json<ErrorResponse>)> {
    if state.config.enable_logging {
        debug!("REST: Getting all balances for {}", address);
    }

    // Check for x-cosmos-block-height header
    let block_height = headers
        .get("x-cosmos-block-height")
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.parse::<i64>().ok());

    if let Some(height) = block_height {
        debug!("REST: Using historical height {}", height);
    }

    // Convert REST pagination to gRPC pagination
    let grpc_pagination = pagination.limit.map(|limit| crate::grpc::PageRequest {
        key: vec![],
        offset: pagination.offset.unwrap_or(0),
        limit,
        count_total: pagination.count_total.unwrap_or(false),
        reverse: false,
    });

    // Call the gRPC bank service
    let mut request = Request::new(bank::QueryAllBalancesRequest {
        address: address.clone(),
        pagination: grpc_pagination,
    });

    // Add block height to gRPC metadata if provided
    if let Some(height) = block_height {
        request.metadata_mut().insert(
            "x-cosmos-block-height",
            height.to_string().parse().unwrap(),
        );
    }

    match state.bank_service.as_ref().all_balances(request).await {
        Ok(response) => {
            let grpc_response = response.into_inner();
            let balances: Vec<Coin> = grpc_response
                .balances
                .into_iter()
                .map(|coin| Coin {
                    denom: coin.denom,
                    amount: coin.amount,
                })
                .collect();

            let pagination_response = grpc_response.pagination.map(|page| PageResponse {
                next_key: if page.next_key.is_empty() {
                    None
                } else {
                    Some(base64::engine::general_purpose::STANDARD.encode(&page.next_key))
                },
                total: Some(page.total.to_string()),
            });

            Ok(Json(AllBalancesResponse {
                balances,
                pagination: pagination_response,
            }))
        }
        Err(e) => {
            error!("gRPC all balances query failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("All balances query failed: {e}"),
                    code: 500,
                }),
            ))
        }
    }
}

// Auth REST endpoints

/// Account response
#[derive(Serialize, Deserialize)]
pub struct AccountResponse {
    pub account: AccountInfo,
}

/// Account info
#[derive(Serialize, Deserialize)]
pub struct AccountInfo {
    pub address: String,
    pub pub_key: Option<String>,
    pub account_number: String,
    pub sequence: String,
}

/// Get account information
async fn get_account(
    Path(address): Path<String>,
    State(state): State<Arc<RestGatewayState>>,
) -> Result<Json<AccountResponse>, (StatusCode, Json<ErrorResponse>)> {
    if state.config.enable_logging {
        debug!("REST: Getting account info for {}", address);
    }

    // Call the gRPC auth service
    let request = Request::new(auth::QueryAccountRequest {
        address: address.clone(),
    });

    match state.auth_service.as_ref().account(request).await {
        Ok(response) => {
            let grpc_response = response.into_inner();
            let account_info = if let Some(account) = grpc_response.account {
                AccountInfo {
                    address: account.address,
                    pub_key: account
                        .pub_key
                        .map(|pk| base64::engine::general_purpose::STANDARD.encode(&pk.value)),
                    account_number: account.account_number.to_string(),
                    sequence: account.sequence.to_string(),
                }
            } else {
                // Return default account if not found (as per Cosmos SDK behavior)
                AccountInfo {
                    address,
                    pub_key: None,
                    account_number: "0".to_string(),
                    sequence: "0".to_string(),
                }
            };

            Ok(Json(AccountResponse {
                account: account_info,
            }))
        }
        Err(e) => {
            error!("gRPC account query failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Account query failed: {e}"),
                    code: 500,
                }),
            ))
        }
    }
}

// Transaction REST endpoints

/// Broadcast request
#[derive(Deserialize)]
pub struct BroadcastRequest {
    pub tx_bytes: String,
    pub mode: String,
}

/// Broadcast response
#[derive(Serialize)]
pub struct BroadcastResponse {
    pub tx_response: TxResponse,
}

/// Transaction response
#[derive(Serialize)]
pub struct TxResponse {
    pub height: String,
    pub txhash: String,
    pub code: u32,
    pub data: String,
    pub raw_log: String,
    pub logs: Vec<TxLog>,
    pub gas_wanted: String,
    pub gas_used: String,
}

/// Transaction log entry
#[derive(Serialize)]
pub struct TxLog {
    pub msg_index: u32,
    pub log: String,
    pub events: Vec<TxEvent>,
}

/// Transaction event
#[derive(Serialize)]
pub struct TxEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub attributes: Vec<EventAttribute>,
}

/// Event attribute
#[derive(Serialize)]
pub struct EventAttribute {
    pub key: String,
    pub value: String,
}

/// Broadcast transaction
async fn broadcast_tx(
    State(state): State<Arc<RestGatewayState>>,
    Json(req): Json<BroadcastRequest>,
) -> Result<Json<BroadcastResponse>, (StatusCode, Json<ErrorResponse>)> {
    if state.config.enable_logging {
        debug!("REST: Broadcasting transaction with mode {}", req.mode);
    }

    // Validate and decode base64 tx bytes
    let tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&req.tx_bytes)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Invalid base64 tx_bytes: {e}"),
                    code: 400,
                }),
            )
        })?;

    // Convert REST broadcast mode to gRPC broadcast mode
    let grpc_mode = match req.mode.as_str() {
        "BROADCAST_MODE_SYNC" => crate::grpc::BroadcastMode::Sync,
        "BROADCAST_MODE_ASYNC" => crate::grpc::BroadcastMode::Async,
        "BROADCAST_MODE_COMMIT" => crate::grpc::BroadcastMode::Commit,
        _ => crate::grpc::BroadcastMode::Sync, // Default to sync
    };

    // Call the gRPC tx service
    let grpc_request = Request::new(tx::BroadcastTxRequest {
        tx_bytes,
        mode: grpc_mode,
    });

    match state.tx_service.as_ref().broadcast_tx(grpc_request).await {
        Ok(response) => {
            let grpc_response = response.into_inner();
            let tx_response = if let Some(tx_resp) = grpc_response.tx_response {
                TxResponse {
                    height: tx_resp.height.to_string(),
                    txhash: tx_resp.txhash,
                    code: tx_resp.code,
                    data: tx_resp.data,
                    raw_log: tx_resp.raw_log,
                    logs: tx_resp
                        .logs
                        .into_iter()
                        .map(|log| TxLog {
                            msg_index: log.msg_index,
                            log: log.log,
                            events: log
                                .events
                                .into_iter()
                                .map(|event| TxEvent {
                                    event_type: event.type_,
                                    attributes: event
                                        .attributes
                                        .into_iter()
                                        .map(|attr| EventAttribute {
                                            key: attr.key,
                                            value: attr.value,
                                        })
                                        .collect(),
                                })
                                .collect(),
                        })
                        .collect(),
                    gas_wanted: tx_resp.gas_wanted.to_string(),
                    gas_used: tx_resp.gas_used.to_string(),
                }
            } else {
                return Err((
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: "Empty transaction response from gRPC service".to_string(),
                        code: 500,
                    }),
                ));
            };

            Ok(Json(BroadcastResponse { tx_response }))
        }
        Err(e) => {
            error!("gRPC broadcast transaction failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Transaction broadcast failed: {e}"),
                    code: 500,
                }),
            ))
        }
    }
}

/// Simulate transaction request
#[derive(Deserialize)]
pub struct SimulateRequest {
    pub tx_bytes: String,
}

/// Simulate response
#[derive(Serialize)]
pub struct SimulateResponse {
    pub gas_info: GasInfo,
    pub result: SimulateResult,
}

/// Gas information
#[derive(Serialize)]
pub struct GasInfo {
    pub gas_wanted: String,
    pub gas_used: String,
}

/// Simulation result
#[derive(Serialize)]
pub struct SimulateResult {
    pub data: String,
    pub log: String,
    pub events: Vec<TxEvent>,
}

/// Simulate transaction
async fn simulate_tx(
    State(state): State<Arc<RestGatewayState>>,
    Json(req): Json<SimulateRequest>,
) -> Result<Json<SimulateResponse>, (StatusCode, Json<ErrorResponse>)> {
    if state.config.enable_logging {
        debug!("REST: Simulating transaction");
    }

    // Validate and decode base64 tx bytes
    let tx_bytes = base64::engine::general_purpose::STANDARD
        .decode(&req.tx_bytes)
        .map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: format!("Invalid base64 tx_bytes: {e}"),
                    code: 400,
                }),
            )
        })?;

    // Call the gRPC tx service for simulation
    let grpc_request = Request::new(tx::SimulateRequest { tx_bytes });

    match state.tx_service.as_ref().simulate(grpc_request).await {
        Ok(response) => {
            let grpc_response = response.into_inner();

            let gas_info = grpc_response
                .gas_info
                .map(|gi| GasInfo {
                    gas_wanted: gi.gas_wanted.to_string(),
                    gas_used: gi.gas_used.to_string(),
                })
                .unwrap_or_else(|| GasInfo {
                    gas_wanted: "0".to_string(),
                    gas_used: "0".to_string(),
                });

            let result = grpc_response
                .result
                .map(|res| SimulateResult {
                    data: base64::engine::general_purpose::STANDARD.encode(&res.data),
                    log: res.log,
                    events: res
                        .events
                        .into_iter()
                        .map(|event| TxEvent {
                            event_type: event.type_,
                            attributes: event
                                .attributes
                                .into_iter()
                                .map(|attr| EventAttribute {
                                    key: attr.key,
                                    value: attr.value,
                                })
                                .collect(),
                        })
                        .collect(),
                })
                .unwrap_or_else(|| SimulateResult {
                    data: "".to_string(),
                    log: "simulation completed".to_string(),
                    events: vec![],
                });

            Ok(Json(SimulateResponse { gas_info, result }))
        }
        Err(e) => {
            error!("gRPC transaction simulation failed: {}", e);
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: format!("Transaction simulation failed: {e}"),
                    code: 500,
                }),
            ))
        }
    }
}

/// Create REST router with comprehensive middleware
pub fn create_rest_router(state: Arc<RestGatewayState>) -> Router {
    use axum::http::Method;

    // CORS configuration
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers(Any)
        .expose_headers([
            "content-type".parse().unwrap(),
            "grpc-status".parse().unwrap(),
            "grpc-message".parse().unwrap(),
        ])
        .max_age(Duration::from_secs(86400)); // 24 hours

    Router::new()
        // Bank endpoints - balance queries
        .route(
            "/cosmos/bank/v1beta1/balances/:address",
            get(get_all_balances),
        )
        .route(
            "/cosmos/bank/v1beta1/balances/:address/:denom",
            get(get_balance),
        )
        // Auth endpoints - account queries
        .route("/cosmos/auth/v1beta1/accounts/:address", get(get_account))
        // Transaction endpoints - broadcasting and simulation
        .route("/cosmos/tx/v1beta1/txs", post(broadcast_tx))
        .route("/cosmos/tx/v1beta1/simulate", post(simulate_tx))
        // Health and status endpoints
        .route("/health", get(health_check))
        .route("/status", get(status_check))
        // Apply middleware layers in correct order (outermost to innermost)
        .layer(TimeoutLayer::new(Duration::from_secs(30)))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::new().level(tracing::Level::INFO))
                .on_response(DefaultOnResponse::new().level(tracing::Level::INFO)),
        )
        .layer(cors)
        .layer(RequestBodyLimitLayer::new(1024 * 1024))
        .with_state(state)
}

/// Health check endpoint
async fn health_check() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "helium-rest-gateway",
        "version": "0.1.0"
    }))
}

/// Status check endpoint with service information
async fn status_check(State(state): State<Arc<RestGatewayState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "running",
        "service": "helium-rest-gateway",
        "version": "0.1.0",
        "grpc_endpoint": state.config.grpc_endpoint,
        "logging_enabled": state.config.enable_logging,
        "uptime": "operational",
        "services": {
            "bank": "connected",
            "auth": "connected",
            "tx": "connected"
        }
    }))
}

/// REST Gateway server
pub struct RestGateway {
    #[allow(dead_code)]
    config: RestGatewayConfig,
    router: Router,
}

impl RestGateway {
    /// Create new REST gateway
    pub async fn new(config: RestGatewayConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let state = Arc::new(RestGatewayState::new(config.clone()).await?);
        let router = create_rest_router(state);

        Ok(Self { config, router })
    }

    /// Get the router for integration with existing server
    pub fn router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Method, Request, StatusCode},
    };
    use tower::ServiceExt; // for `oneshot` and `ready`

    #[test]
    fn test_rest_gateway_config() {
        let config = RestGatewayConfig::default();
        assert_eq!(config.grpc_endpoint, "http://[::1]:9090");
        assert!(config.enable_logging);
    }

    #[test]
    fn test_pagination_params_default() {
        let params = PaginationParams::default();
        assert_eq!(params.limit, Some(100));
        assert_eq!(params.offset, Some(0));
        assert_eq!(params.count_total, Some(false));
    }

    #[tokio::test]
    async fn test_rest_gateway_initialization() {
        let config = RestGatewayConfig::default();
        let state = RestGatewayState::new(config).await.unwrap();

        // Test that services are properly initialized
        assert!(state
            .bank_service
            .as_ref()
            .balance(tonic::Request::new(bank::QueryBalanceRequest {
                address: "cosmos1test".to_string(),
                denom: "stake".to_string(),
            }))
            .await
            .is_ok());
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let config = RestGatewayConfig::default();
        let state = Arc::new(RestGatewayState::new(config).await.unwrap());
        let app = create_rest_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_balance_endpoint_with_grpc() {
        let config = RestGatewayConfig::default();
        let state = Arc::new(RestGatewayState::new(config).await.unwrap());
        let app = create_rest_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/cosmos/bank/v1beta1/balances/cosmos1test/stake")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let balance_response: BalanceResponse = serde_json::from_slice(&body).unwrap();

        // Should return the balance we set up in initialization
        assert_eq!(balance_response.balance.denom, "stake");
        assert_eq!(balance_response.balance.amount, "1000000");
    }

    #[tokio::test]
    async fn test_account_endpoint_with_grpc() {
        let config = RestGatewayConfig::default();
        let state = Arc::new(RestGatewayState::new(config).await.unwrap());
        let app = create_rest_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/cosmos/auth/v1beta1/accounts/cosmos1test")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let account_response: AccountResponse = serde_json::from_slice(&body).unwrap();

        // Should return the account we set up in initialization
        assert_eq!(account_response.account.address, "cosmos1test");
        assert_eq!(account_response.account.account_number, "1");
        assert_eq!(account_response.account.sequence, "0");
    }

    #[tokio::test]
    async fn test_cors_headers() {
        let config = RestGatewayConfig::default();
        let state = Arc::new(RestGatewayState::new(config).await.unwrap());
        let app = create_rest_router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/cosmos/bank/v1beta1/balances/cosmos1test")
                    .header("Origin", "https://example.com")
                    .header("Access-Control-Request-Method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let headers = response.headers();
        assert!(headers.contains_key("access-control-allow-origin"));
        assert!(headers.contains_key("access-control-allow-methods"));
    }
}
