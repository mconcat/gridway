//! Gridway Setup — generates validator configuration files for local testing.
//!
//! Generates Ed25519 keypairs, BLS12-381 threshold shares, and YAML config
//! files for a set of validators to run locally.

use gridway_consensus::{
    config::{NodeConfig, Peers},
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
                .help("Starting port number (each validator uses 2 ports)"),
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
        .get_matches();

    let n_peers = *matches.get_one::<usize>("peers").unwrap();
    let n_bootstrappers = *matches.get_one::<usize>("bootstrappers").unwrap();
    let start_port = *matches.get_one::<u16>("start_port").unwrap();
    let worker_threads = *matches.get_one::<usize>("worker_threads").unwrap();
    let log_level = matches.get_one::<String>("log_level").unwrap().clone();
    let message_backlog = *matches.get_one::<usize>("message_backlog").unwrap();
    let mailbox_size = *matches.get_one::<usize>("mailbox_size").unwrap();
    let deque_size = *matches.get_one::<usize>("deque_size").unwrap();
    let signature_threads = *matches.get_one::<usize>("signature_threads").unwrap();
    let output = matches.get_one::<String>("output").unwrap().clone();

    assert!(
        n_bootstrappers <= n_peers,
        "bootstrappers must be <= peers"
    );

    // Construct output paths
    let raw_current_dir = std::env::current_dir().unwrap();
    let current_dir = raw_current_dir.to_str().unwrap();
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

    // Generate configs
    let mut port = start_port;
    let mut addresses = HashMap::new();
    let mut configurations = Vec::new();
    for (signer, scheme) in peer_signers.iter().zip(schemes.iter()) {
        let name: String = signer.public_key().to_string();
        addresses.insert(
            name.clone(),
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        );
        let peer_config_file = format!("{name}.yaml");
        let directory = format!("{storage_output}/{name}");

        let share_bytes = scheme.share().unwrap().encode();
        let poly_bytes = scheme.polynomial().encode();
        let signer_bytes = signer.encode();

        let peer_config = NodeConfig {
            private_key: hex(&signer_bytes),
            share: hex(&share_bytes),
            polynomial: hex(&poly_bytes),

            port,
            metrics_port: port + 1,
            directory,
            worker_threads,
            log_level: log_level.clone(),

            local: true,
            allowed_peers: allowed_peers.clone(),
            bootstrappers: bootstrappers.clone(),

            message_backlog,
            mailbox_size,
            deque_size,
            signature_threads,
        };
        configurations.push((name, peer_config_file, peer_config));
        port += 2;
    }

    // Write output
    fs::create_dir_all(&output).unwrap();
    fs::create_dir_all(&storage_output).unwrap();

    // Write peers file
    let peers_path = format!("{output}/peers.yaml");
    let file = fs::File::create(&peers_path).unwrap();
    serde_yaml::to_writer(file, &Peers { addresses }).unwrap();

    // Write config files
    for (name, peer_config_file, peer_config) in &configurations {
        let path = format!("{output}/{peer_config_file}");
        let file = fs::File::create(&path).unwrap();
        serde_yaml::to_writer(file, peer_config).unwrap();
        info!(path = peer_config_file, name, "wrote peer configuration");
    }

    // Print start commands
    info!("Setup complete!");
    println!("\nTo start validators, run:");
    for (name, peer_config_file, _) in &configurations {
        let path = format!("{output}/{peer_config_file}");
        let command = format!(
            "cargo run --bin gridway-node -- --peers={peers_path} --config={path}"
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
}
