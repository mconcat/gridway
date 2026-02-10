//! Gridway CLI — unified command-line tool for key management, TX building, and queries.
//!
//! Subcommands:
//! - `generate [--seed N]` — generate a new keypair
//! - `sign --key <hex> --body '<json>'` — sign a TX body
//! - `address --pubkey <hex>` — derive address from public key
//! - `keys store --name <name> --key <hex>` — store encrypted key
//! - `keys list` — list stored keys
//! - `keys export --name <name>` — export decrypted key hex
//! - `keys delete --name <name>` — delete a stored key
//! - `tx send` — build, sign, and submit a bank.MsgSend
//! - `query balance` — query balance
//! - `query account` — query account info
//! - `status` — query node status

use clap::{Arg, Command};
use commonware_codec::{DecodeExt, Encode};
use commonware_cryptography::{ed25519, Signer};
use commonware_math::algebra::Random;
use gridway_client::keystore::Keystore;
use gridway_client::{Coin, GridwayClient, TxBuilder};
use gridway_crypto::Address;
use rand::rngs::OsRng;
use std::path::PathBuf;

const DEFAULT_NODE: &str = "http://localhost:4547";
const DEFAULT_DENOM: &str = "ugridway";

/// Read a password from the `GRIDWAY_KEY_PASSWORD` env var, or prompt on stderr + read from stdin.
fn read_password(prompt: &str) -> String {
    if let Ok(pw) = std::env::var("GRIDWAY_KEY_PASSWORD") {
        return pw;
    }
    eprint!("{}", prompt);
    let mut password = String::new();
    std::io::stdin()
        .read_line(&mut password)
        .expect("Failed to read password from stdin");
    // Trim trailing newline
    password.trim_end().to_string()
}

/// Get the keystore directory from `GRIDWAY_KEYSTORE_DIR` env var, or None for default.
fn keystore_dir() -> Option<PathBuf> {
    std::env::var("GRIDWAY_KEYSTORE_DIR").ok().map(PathBuf::from)
}

#[tokio::main]
async fn main() {
    let matches = Command::new("gridway-cli")
        .about("Gridway CLI — key management, transactions, and queries")
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
                        .help("Hex-encoded ed25519 private key"),
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
                        .help("Hex-encoded ed25519 public key"),
                ),
        )
        .subcommand(
            Command::new("keys")
                .about("Encrypted keystore management")
                .subcommand(
                    Command::new("store")
                        .about("Store a private key encrypted with a password")
                        .arg(
                            Arg::new("name")
                                .long("name")
                                .required(true)
                                .help("Name for the stored key"),
                        )
                        .arg(
                            Arg::new("key")
                                .long("key")
                                .required(true)
                                .help("Hex-encoded ed25519 private key to store"),
                        ),
                )
                .subcommand(Command::new("list").about("List all stored key names"))
                .subcommand(
                    Command::new("export")
                        .about("Decrypt and print a stored key as hex")
                        .arg(
                            Arg::new("name")
                                .long("name")
                                .required(true)
                                .help("Name of the key to export"),
                        ),
                )
                .subcommand(
                    Command::new("delete")
                        .about("Delete a stored key")
                        .arg(
                            Arg::new("name")
                                .long("name")
                                .required(true)
                                .help("Name of the key to delete"),
                        ),
                ),
        )
        .subcommand(
            Command::new("tx")
                .about("Transaction commands")
                .subcommand(
                    Command::new("send")
                        .about("Build, sign, and submit a bank.MsgSend transaction")
                        .arg(
                            Arg::new("key")
                                .long("key")
                                .help("Hex-encoded ed25519 private key")
                                .conflicts_with("keyname"),
                        )
                        .arg(
                            Arg::new("keyname")
                                .long("keyname")
                                .help("Name of a key in the encrypted keystore")
                                .conflicts_with("key"),
                        )
                        .arg(
                            Arg::new("to")
                                .long("to")
                                .required(true)
                                .help("Recipient address (hex)"),
                        )
                        .arg(
                            Arg::new("amount")
                                .long("amount")
                                .required(true)
                                .help("Amount to send")
                                .value_parser(clap::value_parser!(u64)),
                        )
                        .arg(
                            Arg::new("denom")
                                .long("denom")
                                .default_value(DEFAULT_DENOM)
                                .help("Denomination (default: ugridway)"),
                        )
                        .arg(
                            Arg::new("node")
                                .long("node")
                                .default_value(DEFAULT_NODE)
                                .help("Node URL (default: http://localhost:4547)"),
                        )
                        .arg(
                            Arg::new("memo")
                                .long("memo")
                                .default_value("")
                                .help("Transaction memo"),
                        )
                        .arg(
                            Arg::new("dry-run")
                                .long("dry-run")
                                .action(clap::ArgAction::SetTrue)
                                .help("Print signed TX JSON without submitting"),
                        ),
                ),
        )
        .subcommand(
            Command::new("query")
                .about("Query commands")
                .subcommand(
                    Command::new("balance")
                        .about("Query balance of an address")
                        .arg(
                            Arg::new("address")
                                .long("address")
                                .required(true)
                                .help("Address to query (hex)"),
                        )
                        .arg(
                            Arg::new("denom")
                                .long("denom")
                                .default_value(DEFAULT_DENOM)
                                .help("Denomination (default: ugridway)"),
                        )
                        .arg(
                            Arg::new("node")
                                .long("node")
                                .default_value(DEFAULT_NODE)
                                .help("Node URL"),
                        ),
                )
                .subcommand(
                    Command::new("account")
                        .about("Query account info")
                        .arg(
                            Arg::new("address")
                                .long("address")
                                .required(true)
                                .help("Address to query (hex)"),
                        )
                        .arg(
                            Arg::new("node")
                                .long("node")
                                .default_value(DEFAULT_NODE)
                                .help("Node URL"),
                        ),
                ),
        )
        .subcommand(
            Command::new("status")
                .about("Query node status")
                .arg(
                    Arg::new("node")
                        .long("node")
                        .default_value(DEFAULT_NODE)
                        .help("Node URL"),
                ),
        )
        .get_matches();

    match matches.subcommand() {
        Some(("generate", sub_m)) => cmd_generate(sub_m),
        Some(("sign", sub_m)) => cmd_sign(sub_m),
        Some(("address", sub_m)) => cmd_address(sub_m),
        Some(("keys", sub_m)) => match sub_m.subcommand() {
            Some(("store", store_m)) => cmd_keys_store(store_m),
            Some(("list", _)) => cmd_keys_list(),
            Some(("export", export_m)) => cmd_keys_export(export_m),
            Some(("delete", delete_m)) => cmd_keys_delete(delete_m),
            _ => {
                eprintln!("Unknown keys subcommand. Use --help for usage.");
                std::process::exit(1);
            }
        },
        Some(("tx", sub_m)) => match sub_m.subcommand() {
            Some(("send", send_m)) => cmd_tx_send(send_m).await,
            _ => {
                eprintln!("Unknown tx subcommand. Use --help for usage.");
                std::process::exit(1);
            }
        },
        Some(("query", sub_m)) => match sub_m.subcommand() {
            Some(("balance", balance_m)) => cmd_query_balance(balance_m).await,
            Some(("account", account_m)) => cmd_query_account(account_m).await,
            _ => {
                eprintln!("Unknown query subcommand. Use --help for usage.");
                std::process::exit(1);
            }
        },
        Some(("status", sub_m)) => cmd_status(sub_m).await,
        _ => {
            eprintln!("No subcommand provided. Use --help for usage.");
            std::process::exit(1);
        }
    }
}

// ============================================================================
// Key management commands (backward compatible with gridway-keygen)
// ============================================================================

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
    let private_key =
        ed25519::PrivateKey::decode(key_bytes.as_ref()).expect("Invalid ed25519 private key");
    let public_key = private_key.public_key();

    // Parse and re-serialize body for canonical JSON
    let body_value: serde_json::Value =
        serde_json::from_str(body_json_str).expect("Invalid body JSON");
    let canonical_body = serde_json::to_string(&body_value).expect("Failed to serialize body");

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
    let public_key =
        ed25519::PublicKey::decode(pk_bytes.as_ref()).expect("Invalid ed25519 public key");
    let address = Address::from_public_key(&public_key).to_hex();

    let output = serde_json::json!({
        "public_key": pubkey_hex,
        "address": address,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}

// ============================================================================
// Keystore commands
// ============================================================================

fn cmd_keys_store(matches: &clap::ArgMatches) {
    let name = matches.get_one::<String>("name").unwrap();
    let key_hex = matches.get_one::<String>("key").unwrap();
    let password = read_password("Password: ");

    let ks = Keystore::new(keystore_dir());
    match ks.import_key(name, key_hex, &password) {
        Ok(()) => {
            eprintln!("Key '{}' stored successfully.", name);
        }
        Err(e) => {
            eprintln!("Error storing key: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_keys_list() {
    let ks = Keystore::new(keystore_dir());
    match ks.list_keys() {
        Ok(names) => {
            if names.is_empty() {
                println!("No keys stored.");
            } else {
                println!("Stored keys:");
                for name in &names {
                    println!("  - {}", name);
                }
            }
        }
        Err(e) => {
            eprintln!("Error listing keys: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_keys_export(matches: &clap::ArgMatches) {
    let name = matches.get_one::<String>("name").unwrap();
    let password = read_password("Password: ");

    let ks = Keystore::new(keystore_dir());
    match ks.export_key(name, &password) {
        Ok(hex_key) => {
            println!("{}", hex_key);
        }
        Err(e) => {
            eprintln!("Error exporting key: {}", e);
            std::process::exit(1);
        }
    }
}

fn cmd_keys_delete(matches: &clap::ArgMatches) {
    let name = matches.get_one::<String>("name").unwrap();

    let ks = Keystore::new(keystore_dir());
    match ks.delete_key(name) {
        Ok(()) => {
            eprintln!("Key '{}' deleted.", name);
        }
        Err(e) => {
            eprintln!("Error deleting key: {}", e);
            std::process::exit(1);
        }
    }
}

// ============================================================================
// Transaction commands
// ============================================================================

/// Resolve private key hex from either `--key` or `--keyname` flag.
fn resolve_private_key_hex(matches: &clap::ArgMatches) -> String {
    if let Some(key_hex) = matches.get_one::<String>("key") {
        return key_hex.clone();
    }

    if let Some(keyname) = matches.get_one::<String>("keyname") {
        let password = read_password("Password: ");
        let ks = Keystore::new(keystore_dir());
        match ks.export_key(keyname, &password) {
            Ok(hex_key) => return hex_key,
            Err(e) => {
                eprintln!("Error loading key '{}': {}", keyname, e);
                std::process::exit(1);
            }
        }
    }

    eprintln!("Error: either --key <hex> or --keyname <name> is required");
    std::process::exit(1);
}

async fn cmd_tx_send(matches: &clap::ArgMatches) {
    let key_hex = resolve_private_key_hex(matches);
    let to_address = matches.get_one::<String>("to").unwrap();
    let amount = *matches.get_one::<u64>("amount").unwrap();
    let denom = matches.get_one::<String>("denom").unwrap();
    let node_url = matches.get_one::<String>("node").unwrap();
    let memo = matches.get_one::<String>("memo").unwrap();
    let dry_run = matches.get_flag("dry-run");

    // Build the transaction
    let builder = match TxBuilder::from_hex_key(&key_hex) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    let mut builder = builder.bank_send(to_address, vec![Coin::new(denom, amount)]);
    if !memo.is_empty() {
        builder = builder.memo(memo);
    }

    let signed_tx = match builder.build() {
        Ok(tx) => tx,
        Err(e) => {
            eprintln!("Error building transaction: {}", e);
            std::process::exit(1);
        }
    };

    if dry_run {
        println!(
            "{}",
            signed_tx
                .to_json_pretty()
                .unwrap_or_else(|e| format!("JSON error: {}", e))
        );
        return;
    }

    // Submit
    let client = GridwayClient::new(node_url);
    match client.submit_tx(&signed_tx).await {
        Ok(resp) => {
            let output = serde_json::json!({
                "status": resp.status,
                "tx_hash": resp.tx_hash,
                "from": signed_tx.body.messages[0]["value"]["from_address"],
                "to": to_address,
                "amount": format!("{}{}", amount, denom),
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        Err(e) => {
            eprintln!("Error submitting transaction: {}", e);
            std::process::exit(1);
        }
    }
}

// ============================================================================
// Query commands
// ============================================================================

async fn cmd_query_balance(matches: &clap::ArgMatches) {
    let address = matches.get_one::<String>("address").unwrap();
    let denom = matches.get_one::<String>("denom").unwrap();
    let node_url = matches.get_one::<String>("node").unwrap();

    let client = GridwayClient::new(node_url);
    match client.get_balance(address, denom).await {
        Ok(resp) => {
            let output = serde_json::json!({
                "address": resp.address,
                "denom": resp.denom,
                "balance": resp.balance,
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        Err(e) => {
            eprintln!("Error querying balance: {}", e);
            std::process::exit(1);
        }
    }
}

async fn cmd_query_account(matches: &clap::ArgMatches) {
    let address = matches.get_one::<String>("address").unwrap();
    let node_url = matches.get_one::<String>("node").unwrap();

    let client = GridwayClient::new(node_url);
    match client.get_account(address).await {
        Ok(resp) => {
            let output = serde_json::json!({
                "address": resp.address,
                "public_key": resp.public_key,
                "sequence": resp.sequence,
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        Err(e) => {
            eprintln!("Error querying account: {}", e);
            std::process::exit(1);
        }
    }
}

async fn cmd_status(matches: &clap::ArgMatches) {
    let node_url = matches.get_one::<String>("node").unwrap();

    let client = GridwayClient::new(node_url);
    match client.get_status().await {
        Ok(resp) => {
            let output = serde_json::json!({
                "chain_id": resp.chain_id,
                "state_root": resp.state_root,
                "pending_tx_count": resp.pending_tx_count,
            });
            println!("{}", serde_json::to_string_pretty(&output).unwrap());
        }
        Err(e) => {
            eprintln!("Error querying status: {}", e);
            std::process::exit(1);
        }
    }
}
