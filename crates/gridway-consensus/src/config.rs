//! Node configuration for gridway validators.
//!
//! Follows Alto's Config pattern for YAML-based configuration.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;

/// Configuration for a gridway validator node.
#[derive(Deserialize, Serialize)]
pub struct NodeConfig {
    /// Hex-encoded Ed25519 private key for node identity.
    pub private_key: String,
    /// Hex-encoded BLS12-381 threshold share for consensus signing.
    pub share: String,
    /// Hex-encoded BLS12-381 threshold polynomial for the validator set.
    pub polynomial: String,

    /// P2P listening port.
    pub port: u16,
    /// Prometheus metrics port.
    pub metrics_port: u16,
    /// Directory for persistent storage (consensus state, archives).
    pub directory: String,
    /// Number of tokio worker threads.
    pub worker_threads: usize,
    /// Log level (trace, debug, info, warn, error).
    pub log_level: String,

    /// Whether to use local networking mode (loopback addresses).
    pub local: bool,
    /// Hex-encoded Ed25519 public keys of allowed peers.
    pub allowed_peers: Vec<String>,
    /// Hex-encoded Ed25519 public keys of bootstrapper nodes.
    pub bootstrappers: Vec<String>,

    /// Maximum number of pending messages per p2p channel.
    pub message_backlog: usize,
    /// Size of internal mailboxes for actor communication.
    pub mailbox_size: usize,
    /// Size of deque buffers for broadcast engine.
    pub deque_size: usize,

    /// Number of threads for parallel signature verification.
    pub signature_threads: usize,
}

/// A list of peers provided when a validator is run locally.
#[derive(Deserialize, Serialize)]
pub struct Peers {
    pub addresses: HashMap<String, SocketAddr>,
}
