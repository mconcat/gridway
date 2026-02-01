//! Gridway Keygen — key generation and transaction signing utility.
//!
//! Subcommands:
//! - `generate [--seed N]` — generate a new keypair
//! - `sign --key <hex> --body '<json>'` — sign a TX body
//! - `address --pubkey <hex>` — derive address from public key

use clap::{Arg, Command};
use commonware_codec::{DecodeExt, Encode};
use commonware_cryptography::{ed25519, Signer};
use commonware_math::algebra::Random;
use gridway_crypto::Address;
use rand::rngs::OsRng;

fn main() {
    let matches = Command::new("gridway-keygen")
        .about("Key generation and TX signing for gridway")
        .subcommand(
            Command::new("generate")
                .about("Generate a new ed25519 keypair")
                .arg(
                    Arg::new("seed")
                        .long("seed")
                        .help("Deterministic seed (u64) for reproducible keys")
                        .value_parser(clap::value_parser!(u64)),
                ),
        )
        .subcommand(
            Command::new("sign")
                .about("Sign a transaction body")
                .arg(
                    Arg::new("key")
                        .long("key")
                        .required(true)
                        .help("Hex-encoded 32-byte ed25519 private key"),
                )
                .arg(
                    Arg::new("body")
                        .long("body")
                        .required(true)
                        .help("JSON transaction body to sign"),
                ),
        )
        .subcommand(
            Command::new("address")
                .about("Derive address from public key")
                .arg(
                    Arg::new("pubkey")
                        .long("pubkey")
                        .required(true)
                        .help("Hex-encoded 32-byte ed25519 public key"),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("generate", sub_m)) => cmd_generate(sub_m),
        Some(("sign", sub_m)) => cmd_sign(sub_m),
        Some(("address", sub_m)) => cmd_address(sub_m),
        _ => {
            eprintln!("No subcommand provided. Use --help for usage.");
            std::process::exit(1);
        }
    }
}

fn cmd_generate(matches: &clap::ArgMatches) {
    let private_key = if let Some(&seed) = matches.get_one::<u64>("seed") {
        ed25519::PrivateKey::from_seed(seed)
    } else {
        ed25519::PrivateKey::random(&mut OsRng)
    };

    let public_key = private_key.public_key();
    let address = Address::from_public_key(&public_key).to_hex();

    let output = serde_json::json!({
        "private_key": hex::encode(private_key.encode()),
        "public_key": hex::encode(public_key.as_ref()),
        "address": address,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

fn cmd_sign(matches: &clap::ArgMatches) {
    let key_hex = matches.get_one::<String>("key").unwrap();
    let body_json_str = matches.get_one::<String>("body").unwrap();

    // Decode private key
    let key_bytes = hex::decode(key_hex).expect("Invalid private key hex");
    let private_key = ed25519::PrivateKey::decode(key_bytes.as_ref())
        .expect("Invalid ed25519 private key");
    let public_key = private_key.public_key();

    // Parse and re-serialize body for canonical JSON
    let body_value: serde_json::Value = serde_json::from_str(body_json_str)
        .expect("Invalid body JSON");
    let canonical_body = serde_json::to_string(&body_value)
        .expect("Failed to serialize body");

    // Sign
    let signature = gridway_crypto::sign_tx_body(&private_key, canonical_body.as_bytes());

    // Build signed TX
    let signed_tx = serde_json::json!({
        "body": body_value,
        "public_key": hex::encode(public_key.as_ref()),
        "signature": hex::encode(signature.as_ref()),
    });

    println!("{}", serde_json::to_string(&signed_tx).unwrap());
}

fn cmd_address(matches: &clap::ArgMatches) {
    let pubkey_hex = matches.get_one::<String>("pubkey").unwrap();

    let pk_bytes = hex::decode(pubkey_hex).expect("Invalid public key hex");
    let public_key = ed25519::PublicKey::decode(pk_bytes.as_ref())
        .expect("Invalid ed25519 public key");
    let address = Address::from_public_key(&public_key).to_hex();

    let output = serde_json::json!({
        "public_key": pubkey_hex,
        "address": address,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
