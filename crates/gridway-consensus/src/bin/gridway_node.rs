//! Gridway Node — the main validator binary.
//!
//! Wires together all components following Alto's validator.rs pattern:
//! - commonware-runtime (tokio) for async execution
//! - commonware-p2p (authenticated) for networking
//! - commonware-broadcast (buffered) for message dissemination
//! - commonware-consensus (simplex) for BFT consensus
//! - gridway-baseapp for WASM microkernel execution

use gridway_baseapp::{Account, BaseApp};
use gridway_client::Keystore;
use gridway_consensus::{
    application::GridwayApp,
    config::{GenesisConfig, NodeConfig, Peers},
    engine,
    mempool::MempoolError,
    types::{PublicKey, EPOCH, NAMESPACE},
};

use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use clap::{Arg, Command};
use commonware_codec::{Decode, DecodeExt};
use commonware_consensus::{marshal, types::ViewDelta};
use commonware_cryptography::{
    bls12381::primitives::{group, sharing::Sharing, variant::MinSig},
    ed25519::PrivateKey,
    Signer,
};
use commonware_p2p::{authenticated::discovery as authenticated, Ingress, Manager};
use commonware_runtime::{tokio, Metrics, RayonPoolSpawner, Runner};
use commonware_utils::{from_hex_formatted, ordered::Set, union_unique, NZUsize, NZU32};
use futures::future::try_join_all;
use governor::Quota;
use serde::Serialize;
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    num::NonZeroU32,
    path::PathBuf,
    str::FromStr,
    sync::Arc,
    time::Duration,
};
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info, Level};

const PENDING_CHANNEL: u64 = 0;
const RECOVERED_CHANNEL: u64 = 1;
const RESOLVER_CHANNEL: u64 = 2;
const BROADCASTER_CHANNEL: u64 = 3;
const MARSHAL_CHANNEL: u64 = 4;

const LEADER_TIMEOUT: Duration = Duration::from_secs(1);
const NOTARIZATION_TIMEOUT: Duration = Duration::from_secs(2);
const NULLIFY_RETRY: Duration = Duration::from_secs(10);
const ACTIVITY_TIMEOUT: ViewDelta = ViewDelta::new(256);
const SKIP_TIMEOUT: ViewDelta = ViewDelta::new(32);
const FETCH_TIMEOUT: Duration = Duration::from_secs(2);
const FETCH_CONCURRENT: usize = 4;
const MAX_MESSAGE_SIZE: u32 = 1024 * 1024;
const MAX_FETCH_COUNT: usize = 16;
const MAX_FETCH_SIZE: usize = 512 * 1024;
const BLOCKS_FREEZER_TABLE_INITIAL_SIZE: u32 = 2u32.pow(21); // 100MB
const FINALIZED_FREEZER_TABLE_INITIAL_SIZE: u32 = 2u32.pow(21); // 100MB

// ============================================================================
// Genesis state loading
// ============================================================================

/// Apply genesis state from a GenesisConfig to a BaseApp.
///
/// Sets accounts and balances for all genesis accounts, then commits
/// to produce the initial state root. Returns the genesis state root hash.
fn apply_genesis(baseapp: &mut BaseApp, genesis: &GenesisConfig) -> Result<[u8; 32], String> {
    for account in &genesis.accounts {
        baseapp
            .set_account(
                &account.address,
                &Account {
                    public_key: account.public_key_hex.clone(),
                    sequence: 0,
                },
            )
            .map_err(|e| format!("set_account for {}: {e}", account.address))?;

        for balance in &account.balances {
            baseapp
                .set_balance(&account.address, &balance.denom, balance.amount)
                .map_err(|e| {
                    format!("set_balance for {} {}: {e}", account.address, balance.denom)
                })?;
        }
    }

    let root = baseapp
        .commit()
        .map_err(|e| format!("genesis commit: {e}"))?;
    Ok(root)
}

// ============================================================================
// HTTP API — axum-based server
// ============================================================================

/// JSON response for successful tx submission.
#[derive(Serialize)]
struct SubmitTxResponse {
    status: String,
    tx_hash: String,
}

/// JSON response for balance queries.
#[derive(Serialize)]
struct BalanceResponse {
    address: String,
    denom: String,
    balance: u64,
}

/// JSON response for account queries.
#[derive(Serialize)]
struct AccountResponse {
    address: String,
    public_key: String,
    sequence: u64,
}

/// JSON response for node status.
#[derive(Serialize)]
struct StatusResponse {
    chain_id: String,
    state_root: String,
    pending_tx_count: usize,
}

/// JSON response for health check.
#[derive(Serialize)]
struct HealthResponse {
    status: String,
}

/// JSON error response — unified format for all errors.
#[derive(Serialize)]
struct ErrorResponse {
    error: String,
}

/// Convenience: build an error JSON response with a status code.
fn error_response(status: StatusCode, msg: impl Into<String>) -> Response {
    (status, Json(ErrorResponse { error: msg.into() })).into_response()
}

/// POST /tx — submit a transaction.
// TODO: Add per-IP rate limiting here to prevent mempool spam attacks.
// Without rate limiting, an attacker can flood the mempool with transactions.
async fn handle_submit_tx(State(state): State<Arc<AppState>>, body: axum::body::Bytes) -> Response {
    let app = &state.app;
    if body.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "empty body");
    }

    // Validate JSON structure
    let tx_json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(_) => return error_response(StatusCode::BAD_REQUEST, "invalid JSON"),
    };

    // Validate required TX fields before inserting into mempool
    if !tx_json.get("body").is_some_and(|v| v.is_object()) {
        return error_response(StatusCode::BAD_REQUEST, "missing or invalid 'body' field");
    }
    if !tx_json.get("signature").is_some_and(|v| v.is_string()) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "missing or invalid 'signature' field",
        );
    }
    if !tx_json.get("public_key").is_some_and(|v| v.is_string()) {
        return error_response(
            StatusCode::BAD_REQUEST,
            "missing or invalid 'public_key' field",
        );
    }

    match app.submit_tx(body.to_vec()) {
        Ok(tx_hash) => (
            StatusCode::OK,
            Json(SubmitTxResponse {
                status: "submitted".to_string(),
                tx_hash,
            }),
        )
            .into_response(),
        Err(MempoolError::TxTooLarge { size, max }) => error_response(
            StatusCode::BAD_REQUEST,
            format!("transaction too large: {} bytes (max {})", size, max),
        ),
        Err(MempoolError::DuplicateTx { tx_hash }) => error_response(
            StatusCode::BAD_REQUEST,
            format!("duplicate transaction: {}", tx_hash),
        ),
        Err(MempoolError::MempoolFull { reason }) => error_response(
            StatusCode::TOO_MANY_REQUESTS,
            format!("mempool full: {}", reason),
        ),
        Err(MempoolError::LockPoisoned) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal error: mempool lock poisoned",
        ),
    }
}

/// GET /balance/:address/:denom — query balance.
async fn handle_balance_query(
    State(state): State<Arc<AppState>>,
    Path((address, denom)): Path<(String, String)>,
) -> Response {
    let app = &state.app;
    let balance = match app.baseapp().read() {
        Ok(baseapp) => baseapp.get_balance(&address, &denom).unwrap_or(0),
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal: read lock poisoned",
            )
        }
    };

    (
        StatusCode::OK,
        Json(BalanceResponse {
            address,
            denom,
            balance,
        }),
    )
        .into_response()
}

/// GET /account/:address — query account info.
async fn handle_account_query(
    State(state): State<Arc<AppState>>,
    Path(address): Path<String>,
) -> Response {
    let app = &state.app;
    let account = match app.baseapp().read() {
        Ok(baseapp) => baseapp.get_account(&address),
        Err(_) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal: read lock poisoned",
            )
        }
    };

    match account {
        Some(acct) => (
            StatusCode::OK,
            Json(AccountResponse {
                address,
                public_key: acct.public_key,
                sequence: acct.sequence,
            }),
        )
            .into_response(),
        None => error_response(
            StatusCode::NOT_FOUND,
            format!("account not found: {}", address),
        ),
    }
}

/// GET /status — node status.
async fn handle_status(State(state): State<Arc<AppState>>) -> Response {
    let app = &state.app;
    match app.baseapp().read() {
        Ok(baseapp) => {
            let root = hex::encode(baseapp.last_state_root());
            let chain_id = app.chain_id().to_string();
            let pending_tx_count = app.pending_tx_count();
            (
                StatusCode::OK,
                Json(StatusResponse {
                    chain_id,
                    state_root: root,
                    pending_tx_count,
                }),
            )
                .into_response()
        }
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "read lock failed"),
    }
}

/// Maximum number of state entries before the /snapshot endpoint refuses
/// to return inline JSON and suggests file-based export instead.
const SNAPSHOT_MAX_ENTRIES: usize = 10_000;

/// Shared state for the HTTP API, including optional API token for auth.
#[derive(Clone)]
struct AppState {
    app: Arc<GridwayApp>,
    api_token: Option<String>,
}

/// GET /snapshot — full state snapshot (authenticated, size-limited).
///
/// Requires `Authorization: Bearer <token>` if `api_token` is configured.
/// Returns 413 if state has more than SNAPSHOT_MAX_ENTRIES entries.
async fn handle_snapshot(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    // Check bearer token authentication
    if let Some(ref expected_token) = state.api_token {
        let authorized = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|token| token == expected_token.as_str())
            .unwrap_or(false);
        if !authorized {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "missing or invalid Authorization: Bearer <token>",
            );
        }
    }

    match state.app.baseapp().read() {
        Ok(baseapp) => match baseapp.export_snapshot() {
            Ok(snapshot) => {
                // Size limit check
                if snapshot.entries.len() > SNAPSHOT_MAX_ENTRIES {
                    return error_response(
                        StatusCode::PAYLOAD_TOO_LARGE,
                        format!(
                            "state has {} entries (limit {}). Use file-based snapshot export instead (--snapshot flag on node start or export to file).",
                            snapshot.entries.len(),
                            SNAPSHOT_MAX_ENTRIES,
                        ),
                    );
                }
                match serde_json::to_string(&snapshot) {
                    Ok(json) => (
                        StatusCode::OK,
                        [(axum::http::header::CONTENT_TYPE, "application/json")],
                        json,
                    )
                        .into_response(),
                    Err(e) => {
                        error_response(StatusCode::INTERNAL_SERVER_ERROR, format!("serialize: {e}"))
                    }
                }
            }
            Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, format!("export: {e}")),
        },
        Err(_) => error_response(StatusCode::INTERNAL_SERVER_ERROR, "read lock failed"),
    }
}

/// GET /health — health check.
async fn handle_health() -> impl IntoResponse {
    Json(HealthResponse {
        status: "ok".to_string(),
    })
}

/// Build the axum Router with all endpoints.
// TODO: Add HTTP rate limiting (e.g., tower::limit::RateLimitLayer or governor)
// to prevent abuse. Especially important for POST /tx to mitigate mempool spam.
// Consider per-IP rate limits with a shared HashMap<IpAddr, (Instant, u32)> in AppState.
fn build_router(app: GridwayApp, api_token: Option<String>) -> Router {
    let app_state = Arc::new(AppState {
        app: Arc::new(app),
        api_token,
    });

    // SECURITY: CORS allows any origin for public blockchain query endpoints (GET).
    // Methods are restricted to GET/POST only (no DELETE, PUT, PATCH).
    // Headers restricted to Content-Type and Authorization.
    // For production deployments with sensitive endpoints (e.g., /snapshot),
    // consider using a reverse proxy with stricter CORS policies.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
        .allow_headers([
            axum::http::header::CONTENT_TYPE,
            axum::http::header::AUTHORIZATION,
        ]);

    Router::new()
        .route("/tx", post(handle_submit_tx))
        .route("/balance/{address}/{denom}", get(handle_balance_query))
        .route("/account/{address}", get(handle_account_query))
        .route("/status", get(handle_status))
        .route("/snapshot", get(handle_snapshot))
        .route("/health", get(handle_health))
        .layer(cors)
        .with_state(app_state)
}

/// Start the HTTP API server on a separate OS thread with its own tokio runtime.
///
/// This avoids conflicts with commonware's internal tokio runtime.
fn start_http_server(addr: SocketAddr, app: GridwayApp, api_token: Option<String>) {
    std::thread::spawn(move || {
        let rt = match ::tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("Failed to create tokio runtime for HTTP server: {}", e);
                return;
            }
        };

        rt.block_on(async move {
            let router = build_router(app, api_token);

            let listener = match ::tokio::net::TcpListener::bind(addr).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Failed to bind HTTP server on {}: {}", addr, e);
                    return;
                }
            };

            println!("HTTP API listening on http://{}", addr);
            println!("  POST /tx                        — submit transaction");
            println!("  GET  /balance/{{address}}/{{denom}}  — query balance");
            println!("  GET  /account/{{address}}           — query account info");
            println!("  GET  /status                       — node status & state root");
            println!("  GET  /snapshot                     — full state snapshot (JSON)");
            println!("  GET  /health                       — health check");

            if let Err(e) = axum::serve(listener, router).await {
                eprintln!("HTTP server error: {}", e);
            }
        });
    });
}

// ============================================================================
// Main
// ============================================================================

fn main() {
    // Parse arguments
    let matches = Command::new("gridway-node")
        .about("Validator for a gridway chain.")
        .arg(Arg::new("peers").long("peers").required(true))
        .arg(Arg::new("config").long("config").required(true))
        .arg(
            Arg::new("genesis")
                .long("genesis")
                .required(true)
                .help("Path to genesis.yaml containing initial chain state"),
        )
        .arg(
            Arg::new("snapshot")
                .long("snapshot")
                .help("Path or URL to state snapshot JSON file for fast bootstrap"),
        )
        .get_matches();

    // Load peers file
    let peers_file = match matches.get_one::<String>("peers") {
        Some(f) => f,
        None => {
            eprintln!("Missing required argument: --peers");
            std::process::exit(1);
        }
    };
    let peers_content = match std::fs::read_to_string(peers_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Could not read peers file '{}': {}", peers_file, e);
            std::process::exit(1);
        }
    };
    let peers: Peers = match serde_yaml::from_str(&peers_content) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Could not parse peers file: {}", e);
            std::process::exit(1);
        }
    };
    let peer_map: std::collections::HashMap<PublicKey, SocketAddr> = peers
        .addresses
        .into_iter()
        .map(|peer| {
            let key = match from_hex_formatted(&peer.0) {
                Some(k) => k,
                None => {
                    eprintln!("Could not parse peer key '{}'", peer.0);
                    std::process::exit(1);
                }
            };
            let key = match PublicKey::decode(key.as_ref()) {
                Ok(k) => k,
                Err(e) => {
                    eprintln!("Peer key is invalid '{}': {}", peer.0, e);
                    std::process::exit(1);
                }
            };
            (key, peer.1)
        })
        .collect();

    // Load config
    let config_file = match matches.get_one::<String>("config") {
        Some(f) => f,
        None => {
            eprintln!("Missing required argument: --config");
            std::process::exit(1);
        }
    };
    let config_content = match std::fs::read_to_string(config_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Could not read config file '{}': {}", config_file, e);
            std::process::exit(1);
        }
    };
    let config: NodeConfig = match serde_yaml::from_str(&config_content) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Could not parse config file: {}", e);
            std::process::exit(1);
        }
    };
    // Load private key: prefer keystore if configured, fall back to plaintext hex
    let signer = if let Some(ref ks_path) = config.keystore_path {
        let keystore = Keystore::new(Some(std::path::PathBuf::from(ks_path)));
        let key_name = &config.keystore_key_name;

        // Read password from GRIDWAY_KEYSTORE_PASSWORD env var or prompt
        let password = match std::env::var("GRIDWAY_KEYSTORE_PASSWORD") {
            Ok(pw) => pw,
            Err(_) => {
                eprintln!("GRIDWAY_KEYSTORE_PASSWORD environment variable not set.");
                eprintln!("Set it to the keystore password for key '{}'.", key_name);
                std::process::exit(1);
            }
        };

        match keystore.load_key(key_name, &password) {
            Ok(key_bytes) => match PrivateKey::decode(key_bytes.as_ref()) {
                Ok(s) => {
                    eprintln!(
                        "Loaded private key from keystore: {}/{}.json",
                        ks_path, key_name
                    );
                    s
                }
                Err(e) => {
                    eprintln!(
                        "Keystore key '{}' is not a valid ed25519 private key: {}",
                        key_name, e
                    );
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!(
                    "Failed to load key '{}' from keystore at '{}': {}",
                    key_name, ks_path, e
                );
                std::process::exit(1);
            }
        }
    } else {
        // Fall back to plaintext private key from config
        let key = match from_hex_formatted(&config.private_key) {
            Some(k) => k,
            None => {
                eprintln!("Could not parse private key");
                std::process::exit(1);
            }
        };
        match PrivateKey::decode(key.as_ref()) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Private key is invalid: {}", e);
                std::process::exit(1);
            }
        }
    };
    let public_key = signer.public_key();

    // Load genesis config
    let genesis_file = match matches.get_one::<String>("genesis") {
        Some(f) => f,
        None => {
            eprintln!("Missing required argument: --genesis");
            std::process::exit(1);
        }
    };
    let genesis_content = match std::fs::read_to_string(genesis_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Could not read genesis file '{}': {}", genesis_file, e);
            std::process::exit(1);
        }
    };
    let genesis: GenesisConfig = match serde_yaml::from_str(&genesis_content) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Could not parse genesis file: {}", e);
            std::process::exit(1);
        }
    };

    // Capture tx_port and snapshot path before config is moved into the async block
    let tx_port = config.tx_port;
    let snapshot_path = matches.get_one::<String>("snapshot").cloned();

    // Initialize runtime
    let cfg = tokio::Config::default()
        .with_tcp_nodelay(Some(true))
        .with_worker_threads(config.worker_threads)
        .with_storage_directory(PathBuf::from(&config.directory))
        .with_catch_panics(false);
    let executor = tokio::Runner::new(cfg);

    // Start runtime
    executor.start(|context| async move {
        // Configure telemetry
        let log_level = match Level::from_str(&config.log_level) {
            Ok(l) => l,
            Err(e) => {
                error!("Invalid log level '{}': {}", config.log_level, e);
                return;
            }
        };
        tokio::telemetry::init(
            context.with_label("telemetry"),
            tokio::telemetry::Logging {
                level: log_level,
                json: false,
            },
            Some(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                config.metrics_port,
            )),
            None,
        );

        // Prepare peers
        let peer_keys: Vec<PublicKey> = peer_map.keys().cloned().collect();
        let peers_u32 = peer_keys.len() as u32;

        // Build bootstrapper list
        let mut bootstrappers = Vec::new();
        for bootstrapper_hex in &config.bootstrappers {
            let key = match from_hex_formatted(bootstrapper_hex) {
                Some(k) => k,
                None => {
                    error!("Could not parse bootstrapper key '{}'", bootstrapper_hex);
                    return;
                }
            };
            let key = match PublicKey::decode(key.as_ref()) {
                Ok(k) => k,
                Err(e) => {
                    error!("Bootstrapper key is invalid '{}': {}", bootstrapper_hex, e);
                    return;
                }
            };
            let socket = match peer_map.get(&key) {
                Some(s) => s,
                None => {
                    error!("Could not find bootstrapper in peers: {}", bootstrapper_hex);
                    return;
                }
            };
            bootstrappers.push((key, Ingress::Socket(*socket)));
        }

        let ip = match peer_map.get(&public_key) {
            Some(addr) => addr.ip(),
            None => {
                error!("Could not find self in peers");
                return;
            }
        };
        info!(peers = peer_keys.len(), "loaded peers");

        // Parse BLS keys
        let share = match from_hex_formatted(&config.share) {
            Some(s) => s,
            None => {
                error!("Could not parse share hex");
                return;
            }
        };
        let share = match group::Share::decode(share.as_ref()) {
            Ok(s) => s,
            Err(e) => {
                error!("Share is invalid: {}", e);
                return;
            }
        };
        let polynomial = match from_hex_formatted(&config.polynomial) {
            Some(p) => p,
            None => {
                error!("Could not parse polynomial hex");
                return;
            }
        };
        let polynomial =
            match Sharing::<MinSig>::decode_cfg(polynomial.as_ref(), &NZU32!(peers_u32)) {
                Ok(p) => p,
                Err(e) => {
                    error!("Polynomial is invalid: {}", e);
                    return;
                }
            };
        let identity = polynomial.public();
        info!(
            ?public_key,
            ?identity,
            ?ip,
            port = config.port,
            chain_id = %genesis.chain_id,
            "loaded config"
        );

        // ====================================================================
        // Create BaseApp with persistent state (if directory configured)
        // ====================================================================
        let state_db_path = PathBuf::from(&config.directory).join("state");
        let mut baseapp = match BaseApp::with_persistence("gridway".to_string(), &state_db_path) {
            Ok(b) => {
                info!(path = %state_db_path.display(), "created BaseApp with sled persistence");
                b
            }
            Err(e) => {
                error!(
                    "Failed to create persistent BaseApp at {}: {}",
                    state_db_path.display(),
                    e
                );
                info!("Falling back to in-memory BaseApp");
                match BaseApp::new("gridway".to_string()) {
                    Ok(b) => b,
                    Err(e2) => {
                        error!("Failed to create in-memory BaseApp: {}", e2);
                        return;
                    }
                }
            }
        };

        // Check if state was loaded from disk
        let loaded_from_disk = {
            let store = baseapp.global_store().get_store();
            let result = match store.lock() {
                Ok(store) => store.has_persistence() && store.version() > 0,
                Err(e) => {
                    error!("Failed to lock store: {}", e);
                    false
                }
            };
            result
        };

        if loaded_from_disk {
            let state_root = baseapp.last_state_root();
            let version = {
                let store = baseapp.global_store().get_store();
                let v = match store.lock() {
                    Ok(store) => store.version(),
                    Err(e) => {
                        error!("Failed to lock store for version: {}", e);
                        0
                    }
                };
                v
            };
            info!(
                state_root = hex::encode(state_root),
                version = version,
                "loaded persisted state from disk"
            );
        }

        // Only apply genesis state on fresh start — skip if state was loaded from disk
        // to avoid resetting balances/sequences to genesis values.
        if !loaded_from_disk {
            let genesis_root = match apply_genesis(&mut baseapp, &genesis) {
                Ok(r) => r,
                Err(e) => {
                    error!("Failed to apply genesis state: {}", e);
                    return;
                }
            };

            info!(
                state_root = hex::encode(genesis_root),
                accounts = genesis.accounts.len(),
                chain_id = %genesis.chain_id,
                "genesis state committed (fresh start)"
            );
        } else {
            info!(
                chain_id = %genesis.chain_id,
                "skipping genesis — state loaded from disk"
            );
        }

        // If a snapshot was provided, import it (overrides genesis state)
        let skip_replay = if let Some(ref snap_path) = snapshot_path {
            info!(path = %snap_path, "loading state snapshot for fast bootstrap");
            // TODO: Replace tokio::process::Command("curl") with reqwest when added as a dependency.
            // Using async subprocess to avoid blocking the runtime during snapshot download.
            let snapshot_data =
                if snap_path.starts_with("http://") || snap_path.starts_with("https://") {
                    match ::tokio::process::Command::new("curl")
                        .args(["-s", "-f", "--max-time", "300", snap_path])
                        .output()
                        .await
                    {
                        Ok(o) if o.status.success() => o.stdout,
                        Ok(o) => {
                            error!(
                                "Failed to download snapshot: curl failed with status {}",
                                o.status
                            );
                            return;
                        }
                        Err(e) => {
                            error!("Failed to download snapshot: curl error: {}", e);
                            return;
                        }
                    }
                } else {
                    match std::fs::read(snap_path) {
                        Ok(data) => data,
                        Err(e) => {
                            error!("Failed to read snapshot file '{}': {}", snap_path, e);
                            return;
                        }
                    }
                };

            let snapshot: gridway_store::merkle::StateSnapshot =
                match serde_json::from_slice(&snapshot_data) {
                    Ok(s) => s,
                    Err(e) => {
                        error!("Failed to parse snapshot JSON: {}", e);
                        return;
                    }
                };
            info!(
                entries = snapshot.entries.len(),
                version = snapshot.version,
                root_hash = hex::encode(&snapshot.root_hash),
                "importing snapshot"
            );
            if let Err(e) = baseapp.import_snapshot(&snapshot) {
                error!("Failed to import snapshot: {}", e);
                return;
            }
            info!(
                state_root = hex::encode(baseapp.last_state_root()),
                "snapshot imported successfully"
            );
            true
        } else {
            false
        };

        // Create the application wrapper with chain_id from genesis
        let gridway_app = GridwayApp::new(baseapp, genesis.chain_id.clone());

        // ====================================================================
        // Start HTTP API server (if tx_port configured)
        // ====================================================================
        if tx_port > 0 {
            let http_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), tx_port);
            start_http_server(http_addr, gridway_app.clone(), config.api_token.clone());
        }

        // ====================================================================
        // Configure network
        // ====================================================================
        let p2p_namespace = union_unique(NAMESPACE, b"_P2P");
        let mut p2p_cfg = if config.local {
            authenticated::Config::local(
                signer.clone(),
                &p2p_namespace,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), config.port),
                SocketAddr::new(ip, config.port),
                bootstrappers,
                MAX_MESSAGE_SIZE,
            )
        } else {
            authenticated::Config::recommended(
                signer.clone(),
                &p2p_namespace,
                SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), config.port),
                SocketAddr::new(ip, config.port),
                bootstrappers,
                MAX_MESSAGE_SIZE,
            )
        };
        p2p_cfg.mailbox_size = config.mailbox_size;

        // Start p2p
        let (mut network, mut oracle) =
            authenticated::Network::new(context.with_label("network"), p2p_cfg);

        // Provide authorized peers
        let participants: Set<PublicKey> = Set::from_iter_dedup(peer_keys);
        oracle.update(EPOCH.get(), participants.clone()).await;

        // Register channels
        let pending_limit = Quota::per_second(NonZeroU32::new(128).unwrap());
        let pending = network.register(PENDING_CHANNEL, pending_limit, config.message_backlog);

        let recovered_limit = Quota::per_second(NonZeroU32::new(128).unwrap());
        let recovered =
            network.register(RECOVERED_CHANNEL, recovered_limit, config.message_backlog);

        let resolver_limit = Quota::per_second(NonZeroU32::new(128).unwrap());
        let resolver = network.register(RESOLVER_CHANNEL, resolver_limit, config.message_backlog);

        let broadcaster_limit = Quota::per_second(NonZeroU32::new(8).unwrap());
        let broadcaster = network.register(
            BROADCASTER_CHANNEL,
            broadcaster_limit,
            config.message_backlog,
        );

        let marshal_quota = Quota::per_second(NonZeroU32::new(8).unwrap());
        let marshal_channel =
            network.register(MARSHAL_CHANNEL, marshal_quota, config.message_backlog);

        // Start network
        let p2p = network.start();

        // Create parallel strategy
        let strategy = match context.create_strategy(NZUsize!(config.signature_threads)) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to create parallel strategy: {:?}", e);
                return;
            }
        };

        // Create engine (pass the pre-configured GridwayApp)
        let engine_cfg = engine::Config {
            blocker: oracle.clone(),
            partition_prefix: "engine".to_string(),
            blocks_freezer_table_initial_size: BLOCKS_FREEZER_TABLE_INITIAL_SIZE,
            finalized_freezer_table_initial_size: FINALIZED_FREEZER_TABLE_INITIAL_SIZE,
            me: public_key.clone(),
            participants,
            mailbox_size: config.mailbox_size,
            deque_size: config.deque_size,
            leader_timeout: LEADER_TIMEOUT,
            notarization_timeout: NOTARIZATION_TIMEOUT,
            nullify_retry: NULLIFY_RETRY,
            activity_timeout: ACTIVITY_TIMEOUT,
            skip_timeout: SKIP_TIMEOUT,
            fetch_timeout: FETCH_TIMEOUT,
            max_fetch_count: MAX_FETCH_COUNT,
            max_fetch_size: MAX_FETCH_SIZE,
            fetch_concurrent: FETCH_CONCURRENT,
            fetch_rate_per_peer: resolver_limit,
            polynomial,
            share,
            strategy,
            skip_replay,
        };
        let engine = match engine::Engine::new(
            context.with_label("engine"),
            engine_cfg,
            gridway_app,
        )
        .await
        {
            Ok(e) => e,
            Err(e) => {
                error!("Failed to create consensus engine: {}", e);
                return;
            }
        };

        // Create marshal resolver
        let marshal_resolver_cfg = marshal::resolver::p2p::Config {
            public_key: public_key.clone(),
            manager: oracle.clone(),
            blocker: oracle,
            mailbox_size: config.mailbox_size,
            initial: Duration::from_secs(1),
            timeout: Duration::from_secs(2),
            fetch_retry_timeout: Duration::from_millis(100),
            priority_requests: false,
            priority_responses: false,
        };
        let marshal_resolver =
            marshal::resolver::p2p::init(&context, marshal_resolver_cfg, marshal_channel);

        // Start engine
        let engine_handle =
            engine.start(pending, recovered, resolver, broadcaster, marshal_resolver);

        info!("Gridway node started");
        info!("  Chain ID: {}", genesis.chain_id);
        info!("  Consensus: commonware-consensus (simplex BFT)");
        info!("  Networking: commonware-p2p (authenticated)");
        info!("  Execution: gridway-baseapp (WASM microkernel + native bank)");
        info!("  TX Auth: ed25519 signatures with sequence numbers");
        if tx_port > 0 {
            info!("  HTTP API: http://0.0.0.0:{}", tx_port);
        }

        // Wait for any task to error
        if let Err(e) = try_join_all(vec![p2p, engine_handle]).await {
            error!(?e, "task failed");
        }
    });
}
