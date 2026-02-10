//! Commonware consensus integration for gridway.
//!
//! Implements the `commonware_consensus::Application` trait by wrapping
//! the gridway BaseApp. This is the bridge between the Commonware simplex
//! consensus engine and gridway's WASM microkernel.
//!
//! Architecture:
//! - `GridwayApp` implements `Application`, `VerifyingApplication`, and `Reporter`
//! - `propose()` collects pending txs → creates block → executes via BaseApp
//! - `verify()` re-executes block txs → verifies state root matches
//! - `report()` on finalization → commits state to MerkleStore

pub mod application;
pub mod config;
pub mod engine;
pub mod mempool;
pub mod types;

pub use application::GridwayApp;
pub use config::{GenesisAccount, GenesisBalance, GenesisConfig, NodeConfig, Peers};
pub use mempool::{Mempool, MempoolConfig, MempoolError};
pub use types::{
    Activity, Finalization, GridwayScheme, Notarization, Seedable,
};
