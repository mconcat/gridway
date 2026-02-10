//! Gridway Setup — generates validator configuration files for local testing.
//!
//! Generates Ed25519 keypairs, BLS12-381 threshold shares, YAML config
//! files, and a genesis.yaml for a set of validators to run locally.

use gridway_consensus::{
    config::{GenesisAccount, GenesisBalance, GenesisConfig, NodeConfig, Peers},
    types::NAMESPACE,
};

use clap::{value_parser, Arg, Command};
use commonware_codec::Encode;
use commonware_consensus::simplex::scheme::bls12381_threshold;
use commonware_cryptography::{
    bls12381::primitives::variant::MinSig,
    certificate::mocks::Fixture,
    ed25519::PrivateKey,
    Signer,
};
use commonware_math::algebra::Random;
use commonware_utils::hex;
use rand::rngs::OsRng;
use std::{
    collections::HashMap,
    fs,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};
use tracing::info;

/// Default initial balance for each validator in ugridway.

fn main() {
    // Initialize logger
    tracing_subscriber::fmt().init();

    let matches = Command::new("gridway-setup")
        .about("Generate configuration files for a local gridway testnet.")
        .arg(
            Arg::new("peers")
                .long("peers")
                .required(true)
                .value_parser(value_parser!(usize))
                .help("Number of validator peers"),
        )
        .arg(
            Arg::new("bootstrappers")
                .long("bootstrappers")
                .required(true)
                .value_parser(value_parser!(usize))
                .help("Number of bootstrapper nodes"),
        )
        .arg(
            Arg::new("start_port")
                .long("start-port")
                .default_value("4545")
                .value_parser(value_parser!(u16))
                .help("Starting port number (each validator uses 3 ports: p2p, metrics, tx)"),
        )
        .arg(
            Arg::new("chain_id")
                .long("chain-id")
                .default_value("gridway-1")
                .value_parser(value_parser!(String))
                .help("Chain identifier for genesis"),
        )
        .arg(
            Arg::new("genesis_balance")
                .long("genesis-balance")
                .default_value("1000000")
                .value_parser(value_parser!(u64))
                .help("Initial ugridway balance for each validator"),
        )
        .arg(
            Arg::new("worker_threads")
                .long("worker-threads")
                .default_value("2")
                .value_parser(value_parser!(usize)),
        )
        .arg(
            Arg::new("log_level")
                .long("log-level")
                .default_value("info")
                .value_parser(value_parser!(String)),
        )
        .arg(
            Arg::new("message_backlog")
                .long("message-backlog")
                .default_value("256")
                .value_parser(value_parser!(usize)),
        )
        .arg(
            Arg::new("mailbox_size")
                .long("mailbox-size")
                .default_value("1024")
                .value_parser(value_parser!(usize)),
        )
        .arg(
            Arg::new("deque_size")
                .long("deque-size")
                .default_value("10")
                .value_parser(value_parser!(usize)),
        )
        .arg(
            Arg::new("signature_threads")
                .long("signature-threads")
                .default_value("2")
                .value_parser(value_parser!(usize)),
        )
        .arg(
            Arg::new("output")
                .long("output")
                .required(true)
                .value_parser(value_parser!(String))
                .help("Output directory for config files"),
        )
        .arg(
            Arg::new("host_pattern")
                .long("host-pattern")
                .value_parser(value_parser!(String))
                .help("Hostname pattern for peers (use {} for index, e.g. 'gridway-node-{}')"),
        )
        .arg(
            Arg::new("hosts")
                .long("hosts")
                .value_parser(value_parser!(String))
                .help("Comma-separated list of hostnames/IPs for each peer"),
        )
        .arg(
            Arg::new("no_local")
                .long("no-local")
                .action(clap::ArgAction::SetTrue)
                .help("Use recommended (non-local) P2P config (for Docker/remote)"),
        )
        .arg(
            Arg::new("internal_ports")
                .long("internal-ports")
                .action(clap::ArgAction::SetTrue)
                .help("All nodes use same internal ports (4545/4546/4547) — for Docker with port mapping"),
        )
        .get_matches();

    let n_peers = *matches.get_one::<usize>("peers").unwrap();
    let n_bootstrappers = *matches.get_one::<usize>("bootstrappers").unwrap();
    let start_port = *matches.get_one::<u16>("start_port").unwrap();
    let chain_id = matches.get_one::<String>("chain_id").unwrap().clone();
    let genesis_balance = *matches.get_one::<u64>("genesis_balance").unwrap();
    let worker_threads = *matches.get_one::<usize>("worker_threads").unwrap();
    let log_level = matches.get_one::<String>("log_level").unwrap().clone();
    let message_backlog = *matches.get_one::<usize>("message_backlog").unwrap();
    let mailbox_size = *matches.get_one::<usize>("mailbox_size").unwrap();
    let deque_size = *matches.get_one::<usize>("deque_size").unwrap();
    let signature_threads = *matches.get_one::<usize>("signature_threads").unwrap();
    let output = matches.get_one::<String>("output").unwrap().clone();
    let host_pattern = matches.get_one::<String>("host_pattern").cloned();
    let hosts_csv = matches.get_one::<String>("hosts").cloned();
    let no_local = matches.get_flag("no_local");
    let internal_ports = matches.get_flag("internal_ports");

    assert!(
        n_bootstrappers <= n_peers,
        "bootstrappers must be <= peers"
    );

    // Construct output paths
    let raw_current_dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Could not determine current directory: {}", e);
            std::process::exit(1);
        }
    };
    let current_dir = match raw_current_dir.to_str() {
        Some(s) => s,
        None => {
            eprintln!("Current directory path contains invalid UTF-8");
            std::process::exit(1);
        }
    };
    let output = format!("{current_dir}/{output}");
    let storage_output = format!("{output}/storage");

    if fs::metadata(&output).is_ok() {
        eprintln!("Output directory already exists: {output}");
        std::process::exit(1);
    }

    // Generate Ed25519 keys for each peer
    let mut peer_signers: Vec<PrivateKey> = (0..n_peers)
        .map(|_| PrivateKey::random(&mut OsRng))
        .collect();
    peer_signers.sort_by_key(|s: &PrivateKey| s.public_key());

    let allowed_peers: Vec<String> = peer_signers
        .iter()
        .map(|s: &PrivateKey| s.public_key().to_string())
        .collect();

    // Select bootstrappers
    let bootstrappers: Vec<String> = allowed_peers
        .iter()
        .take(n_bootstrappers)
        .cloned()
        .collect();

    // Generate BLS12-381 threshold keys
    let peers_u32 = n_peers as u32;
    let Fixture { schemes, .. } =
        bls12381_threshold::fixture::<MinSig, _>(&mut OsRng, NAMESPACE, peers_u32);

    let identity = schemes[0].polynomial().public();
    info!(%identity, "generated network key");

    // Build genesis accounts from validator public keys
    let mut genesis_accounts = Vec::with_capacity(n_peers);
    for signer in &peer_signers {
        let pk = signer.public_key();
        let address = gridway_crypto::Address::from_public_key(&pk);
        genesis_accounts.push(GenesisAccount {
            address: address.to_hex(),
            public_key_hex: ::hex::encode(pk.as_ref()),
            balances: vec![GenesisBalance {
                denom: "ugridway".to_string(),
                amount: genesis_balance,
            }],
        });
    }

    let genesis = GenesisConfig {
        chain_id: chain_id.clone(),
        accounts: genesis_accounts,
    };

    // Resolve per-peer hostnames/IPs
    let hosts: Vec<IpAddr> = if let Some(csv) = &hosts_csv {
        csv.split(',')
            .map(|h| h.trim().parse::<IpAddr>().unwrap_or_else(|_| {
                eprintln!("Invalid IP in --hosts: '{}'", h.trim());
                std::process::exit(1);
            }))
            .collect()
    } else if let Some(pattern) = &host_pattern {
        // For hostname patterns we still need IPs in peers.yaml (SocketAddr).
        // Use 172.28.0.{10+i} as convention for Docker bridge network.
        (0..n_peers)
            .map(|i| IpAddr::V4(Ipv4Addr::new(172, 28, 0, 10 + i as u8)))
            .collect()
    } else {
        vec![IpAddr::V4(Ipv4Addr::LOCALHOST); n_peers]
    };

    if hosts.len() != n_peers {
        eprintln!(
            "--hosts count ({}) does not match --peers count ({})",
            hosts.len(),
            n_peers
        );
        std::process::exit(1);
    }

    let use_local = !no_local && host_pattern.is_none() && hosts_csv.is_none();

    // Generate configs
    let mut port = start_port;
    let mut addresses = HashMap::new();
    let mut configurations = Vec::new();
    for (i, (signer, scheme)) in peer_signers.iter().zip(schemes.iter()).enumerate() {
        let name: String = signer.public_key().to_string();

        // Port assignment: internal_ports → all nodes use same base ports (Docker)
        // otherwise sequential (local mode)
        let (p2p_port, metrics_port_val, tx_port_val) = if internal_ports {
            (start_port, start_port + 1, start_port + 2)
        } else {
            (port, port + 1, port + 2)
        };

        addresses.insert(
            name.clone(),
            SocketAddr::new(hosts[i], p2p_port),
        );
        let peer_config_file = format!("{name}.yaml");
        let directory = if internal_ports {
            // Docker: use a fixed path inside the container
            "/gridway/data".to_string()
        } else {
            format!("{storage_output}/{name}")
        };

        let share_bytes = match scheme.share() {
            Some(s) => s.encode(),
            None => {
                eprintln!("Failed to get share for peer");
                std::process::exit(1);
            }
        };
        let poly_bytes = scheme.polynomial().encode();
        let signer_bytes = signer.encode();

        let peer_config = NodeConfig {
            private_key: hex(&signer_bytes),
            share: hex(&share_bytes),
            polynomial: hex(&poly_bytes),

            port: p2p_port,
            metrics_port: metrics_port_val,
            tx_port: tx_port_val,
            directory,
            worker_threads,
            log_level: log_level.clone(),

            local: use_local,
            allowed_peers: allowed_peers.clone(),
            bootstrappers: bootstrappers.clone(),

            message_backlog,
            mailbox_size,
            deque_size,
            signature_threads,
        };
        configurations.push((name, peer_config_file, peer_config));
        if !internal_ports {
            port += 3; // p2p, metrics, tx
        }
    }

    // Write output
    if let Err(e) = fs::create_dir_all(&output) {
        eprintln!("Failed to create output directory '{}': {}", output, e);
        std::process::exit(1);
    }
    if let Err(e) = fs::create_dir_all(&storage_output) {
        eprintln!("Failed to create storage directory '{}': {}", storage_output, e);
        std::process::exit(1);
    }

    // Write peers file
    let peers_path = format!("{output}/peers.yaml");
    let file = match fs::File::create(&peers_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create peers file '{}': {}", peers_path, e);
            std::process::exit(1);
        }
    };
    if let Err(e) = serde_yaml::to_writer(file, &Peers { addresses }) {
        eprintln!("Failed to write peers file: {}", e);
        std::process::exit(1);
    }

    // Write genesis file
    let genesis_path = format!("{output}/genesis.yaml");
    let file = match fs::File::create(&genesis_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create genesis file '{}': {}", genesis_path, e);
            std::process::exit(1);
        }
    };
    if let Err(e) = serde_yaml::to_writer(file, &genesis) {
        eprintln!("Failed to write genesis file: {}", e);
        std::process::exit(1);
    }
    info!(path = %genesis_path, accounts = genesis.accounts.len(), "wrote genesis configuration");

    // Write config files — both named and indexed (validator-N/config.yaml for Docker)
    for (i, (name, peer_config_file, peer_config)) in configurations.iter().enumerate() {
        // Named config (e.g. <pubkey>.yaml)
        let path = format!("{output}/{peer_config_file}");
        let file = match fs::File::create(&path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to create config file '{}': {}", path, e);
                std::process::exit(1);
            }
        };
        if let Err(e) = serde_yaml::to_writer(file, peer_config) {
            eprintln!("Failed to write config file '{}': {}", path, e);
            std::process::exit(1);
        }
        info!(path = peer_config_file, name, "wrote peer configuration");

        // Docker-friendly indexed directory (validator-N/config.yaml)
        let validator_dir = format!("{output}/validator-{i}");
        if let Err(e) = fs::create_dir_all(&validator_dir) {
            eprintln!("Failed to create validator dir '{}': {}", validator_dir, e);
            std::process::exit(1);
        }
        let validator_config_path = format!("{validator_dir}/config.yaml");
        let file = match fs::File::create(&validator_config_path) {
            Ok(f) => f,
            Err(e) => {
                eprintln!("Failed to create '{}': {}", validator_config_path, e);
                std::process::exit(1);
            }
        };
        if let Err(e) = serde_yaml::to_writer(file, peer_config) {
            eprintln!("Failed to write '{}': {}", validator_config_path, e);
            std::process::exit(1);
        }
    }

    // Print start commands
    info!("Setup complete!");
    println!("\nTo start validators, run:");
    for (name, peer_config_file, _) in &configurations {
        let path = format!("{output}/{peer_config_file}");
        let command = format!(
            "cargo run --bin gridway-node -- --peers={peers_path} --config={path} --genesis={genesis_path}"
        );
        println!("  {name}: {command}");
    }
    println!("\nTo view metrics:");
    for (name, _, peer_config) in &configurations {
        println!(
            "  {name}: curl http://localhost:{}/metrics",
            peer_config.metrics_port
        );
    }
    println!("\nTo submit transactions / query balances:");
    for (name, _, peer_config) in &configurations {
        if peer_config.tx_port > 0 {
            println!(
                "  {name}: curl http://localhost:{}/balance/<address>/ugridway",
                peer_config.tx_port
            );
        }
    }
    println!("\nGenesis accounts:");
    for account in &genesis.accounts {
        println!(
            "  {} — {} ugridway",
            account.address,
            account.balances.first().map_or(0, |b| b.amount)
        );
    }
}
