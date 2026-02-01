//! Gridway Node — the main validator binary.
//!
//! Wires together all components following Alto's validator.rs pattern:
//! - commonware-runtime (tokio) for async execution
//! - commonware-p2p (authenticated) for networking
//! - commonware-broadcast (buffered) for message dissemination
//! - commonware-consensus (simplex) for BFT consensus
//! - gridway-baseapp for WASM microkernel execution

use gridway_consensus::{
    application::GridwayApp,
    config::{NodeConfig, Peers},
    engine,
    types::{PublicKey, EPOCH, NAMESPACE},
};
use gridway_baseapp::{BaseApp, Account};

use clap::{Arg, Command};
use commonware_codec::{Decode, DecodeExt, Encode};
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
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    num::NonZeroU32,
    path::PathBuf,
    str::FromStr,
    time::Duration,
};
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

/// Fixed seeds for deterministic genesis keypairs (testing only!)
const ALICE_SEED: u64 = 1;
const BOB_SEED: u64 = 2;

// ============================================================================
// HTTP API server for transaction submission and balance queries
// ============================================================================

/// Start a simple HTTP server for TX submission and balance queries.
///
/// Runs in a separate OS thread (std::thread) so it doesn't interfere with
/// the commonware async runtime. Uses blocking I/O — perfectly fine for a
/// demo/testnet API that handles a handful of requests.
fn start_http_server(addr: SocketAddr, app: GridwayApp) {
    std::thread::spawn(move || {
        let listener = match std::net::TcpListener::bind(addr) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("Failed to bind HTTP server on {}: {}", addr, e);
                return;
            }
        };
        // Use println here since tracing may not be set up on this thread
        println!("HTTP API listening on http://{}", addr);
        println!("  POST /tx                        — submit transaction");
        println!("  GET  /balance/{{address}}/{{denom}}  — query balance");
        println!("  GET  /account/{{address}}           — query account info");

        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let app = app.clone();
            std::thread::spawn(move || {
                if let Err(e) = handle_http_request(stream, &app) {
                    eprintln!("HTTP request error: {}", e);
                }
            });
        }
    });
}

fn handle_http_request(
    mut stream: std::net::TcpStream,
    app: &GridwayApp,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;

    let mut reader = BufReader::new(&stream);

    // Read request line
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        let resp = http_response(400, "application/json", r#"{"error":"bad request"}"#);
        stream.write_all(resp.as_bytes())?;
        return Ok(());
    }

    let method = parts[0];
    let path = parts[1];

    // Read headers
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line)?;
        if line.trim().is_empty() {
            break;
        }
        if let Some(val) = line.to_lowercase().strip_prefix("content-length:") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    // Read body
    let body = if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf)?;
        buf
    } else {
        Vec::new()
    };

    // Route request
    let response = match (method, path) {
        ("POST", "/tx") => handle_submit_tx(app, &body),
        ("GET", p) if p.starts_with("/balance/") => handle_balance_query(app, p),
        ("GET", p) if p.starts_with("/account/") => handle_account_query(app, p),
        ("GET", "/health") => http_response(200, "application/json", r#"{"status":"ok"}"#),
        _ => http_response(404, "application/json", r#"{"error":"not found"}"#),
    };

    stream.write_all(response.as_bytes())?;
    Ok(())
}

fn handle_submit_tx(app: &GridwayApp, body: &[u8]) -> String {
    if body.is_empty() {
        return http_response(400, "application/json", r#"{"error":"empty body"}"#);
    }

    // Validate it's at least valid JSON
    if serde_json::from_slice::<serde_json::Value>(body).is_err() {
        return http_response(400, "application/json", r#"{"error":"invalid JSON"}"#);
    }

    app.submit_tx(body.to_vec());
    http_response(200, "application/json", r#"{"status":"submitted"}"#)
}

fn handle_balance_query(app: &GridwayApp, path: &str) -> String {
    let stripped = path.trim_start_matches("/balance/");
    let parts: Vec<&str> = stripped.splitn(2, '/').collect();
    if parts.len() != 2 || parts[0].is_empty() || parts[1].is_empty() {
        return http_response(
            400,
            "application/json",
            r#"{"error":"use /balance/{address}/{denom}"}"#,
        );
    }

    let address = parts[0];
    let denom = parts[1];

    let balance = match app.baseapp().lock() {
        Ok(baseapp) => baseapp.get_balance(address, denom).unwrap_or(0),
        Err(_) => {
            return http_response(
                500,
                "application/json",
                r#"{"error":"internal: lock poisoned"}"#,
            );
        }
    };

    let body = format!(
        r#"{{"address":"{}","denom":"{}","balance":{}}}"#,
        address, denom, balance
    );
    http_response(200, "application/json", &body)
}

fn handle_account_query(app: &GridwayApp, path: &str) -> String {
    let address = path.trim_start_matches("/account/");
    if address.is_empty() {
        return http_response(
            400,
            "application/json",
            r#"{"error":"use /account/{address}"}"#,
        );
    }

    let account = match app.baseapp().lock() {
        Ok(baseapp) => baseapp.get_account(address),
        Err(_) => {
            return http_response(
                500,
                "application/json",
                r#"{"error":"internal: lock poisoned"}"#,
            );
        }
    };

    match account {
        Some(acct) => {
            let body = format!(
                r#"{{"address":"{}","public_key":"{}","sequence":{}}}"#,
                address, acct.public_key, acct.sequence
            );
            http_response(200, "application/json", &body)
        }
        None => {
            let body = format!(r#"{{"error":"account not found: {}"}}"#, address);
            http_response(404, "application/json", &body)
        }
    }
}

fn http_response(status: u16, content_type: &str, body: &str) -> String {
    let status_text = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "Unknown",
    };
    format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        status, status_text, content_type, body.len(), body
    )
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
        .get_matches();

    // Load peers file
    let peers_file = matches.get_one::<String>("peers").unwrap();
    let peers_content = std::fs::read_to_string(peers_file).expect("Could not read peers file");
    let peers: Peers =
        serde_yaml::from_str(&peers_content).expect("Could not parse peers file");
    let peer_map: std::collections::HashMap<PublicKey, SocketAddr> = peers
        .addresses
        .into_iter()
        .map(|peer| {
            let key = from_hex_formatted(&peer.0).expect("Could not parse peer key");
            let key = PublicKey::decode(key.as_ref()).expect("Peer key is invalid");
            (key, peer.1)
        })
        .collect();

    // Load config
    let config_file = matches.get_one::<String>("config").unwrap();
    let config_content =
        std::fs::read_to_string(config_file).expect("Could not read config file");
    let config: NodeConfig =
        serde_yaml::from_str(&config_content).expect("Could not parse config file");
    let key = from_hex_formatted(&config.private_key).expect("Could not parse private key");
    let signer = PrivateKey::decode(key.as_ref()).expect("Private key is invalid");
    let public_key = signer.public_key();

    // Capture tx_port before config is moved into the async block
    let tx_port = config.tx_port;

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
        let log_level = Level::from_str(&config.log_level).expect("Invalid log level");
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
            let key =
                from_hex_formatted(bootstrapper_hex).expect("Could not parse bootstrapper key");
            let key = PublicKey::decode(key.as_ref()).expect("Bootstrapper key is invalid");
            let socket = peer_map
                .get(&key)
                .expect("Could not find bootstrapper in peers");
            bootstrappers.push((key, Ingress::Socket(*socket)));
        }

        let ip = peer_map
            .get(&public_key)
            .expect("Could not find self in peers")
            .ip();
        info!(peers = peer_keys.len(), "loaded peers");

        // Parse BLS keys
        let share = from_hex_formatted(&config.share).expect("Could not parse share");
        let share = group::Share::decode(share.as_ref()).expect("Share is invalid");
        let polynomial =
            from_hex_formatted(&config.polynomial).expect("Could not parse polynomial");
        let polynomial = Sharing::<MinSig>::decode_cfg(polynomial.as_ref(), &NZU32!(peers_u32))
            .expect("Polynomial is invalid");
        let identity = polynomial.public();
        info!(
            ?public_key,
            ?identity,
            ?ip,
            port = config.port,
            "loaded config"
        );

        // ====================================================================
        // Create BaseApp with genesis state using deterministic keypairs
        // ====================================================================
        let mut baseapp = BaseApp::new("gridway".to_string()).expect("Failed to create BaseApp");

        // Generate deterministic keypairs from fixed seeds
        let alice_key = commonware_cryptography::ed25519::PrivateKey::from_seed(ALICE_SEED);
        let alice_pk = alice_key.public_key();
        let alice_addr = gridway_crypto::Address::from_public_key(&alice_pk).to_hex();

        let bob_key = commonware_cryptography::ed25519::PrivateKey::from_seed(BOB_SEED);
        let bob_pk = bob_key.public_key();
        let bob_addr = gridway_crypto::Address::from_public_key(&bob_pk).to_hex();

        // Print genesis keypair info for test script use
        println!("=== GENESIS KEYPAIRS ===");
        println!("ALICE_PRIVKEY={}", hex::encode(alice_key.encode()));
        println!("ALICE_PUBKEY={}", hex::encode(alice_pk.as_ref()));
        println!("ALICE_ADDRESS={}", alice_addr);
        println!("BOB_PRIVKEY={}", hex::encode(bob_key.encode()));
        println!("BOB_PUBKEY={}", hex::encode(bob_pk.as_ref()));
        println!("BOB_ADDRESS={}", bob_addr);
        println!("========================");

        // Create accounts
        baseapp.set_account(&alice_addr, &Account {
            public_key: hex::encode(alice_pk.as_ref()),
            sequence: 0,
        }).expect("Failed to set alice account");
        baseapp.set_account(&bob_addr, &Account {
            public_key: hex::encode(bob_pk.as_ref()),
            sequence: 0,
        }).expect("Failed to set bob account");

        // Set genesis balances using hex addresses
        baseapp.set_balance(&alice_addr, "ugridway", 1_000_000).expect("Failed to set alice balance");
        baseapp.set_balance(&bob_addr, "ugridway", 0).expect("Failed to set bob balance");

        // Commit genesis state so the Merkle root reflects initial balances
        let genesis_root = baseapp.commit().expect("Failed to commit genesis state");
        info!(
            state_root = hex::encode(genesis_root),
            alice_address = %alice_addr,
            bob_address = %bob_addr,
            "committed genesis state (alice: 1_000_000 ugridway, bob: 0 ugridway)"
        );

        // Create the application wrapper
        let gridway_app = GridwayApp::new(baseapp);

        // ====================================================================
        // Start HTTP API server (if tx_port configured)
        // ====================================================================
        if tx_port > 0 {
            let http_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), tx_port);
            start_http_server(http_addr, gridway_app.clone());
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
        let resolver =
            network.register(RESOLVER_CHANNEL, resolver_limit, config.message_backlog);

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
        let strategy = context
            .create_strategy(NZUsize!(config.signature_threads))
            .unwrap();

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
        };
        let engine = engine::Engine::new(
            context.with_label("engine"),
            engine_cfg,
            gridway_app,
        ).await;

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
