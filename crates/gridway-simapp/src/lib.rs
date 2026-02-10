//! Simulation and property-based testing framework for gridway.
//!
//! Provides helper utilities for constructing test scenarios:
//! - `SimState` — expected-state tracker for verifying invariants
//! - `random_keypair` — deterministic keypair generation from seed
//! - `build_transfer_tx` — build a signed bank.MsgSend transaction
//! - `setup_genesis` — create a BaseApp with N genesis accounts

use std::collections::HashMap;

use commonware_cryptography::ed25519::PrivateKey;
use commonware_cryptography::Signer as _;
use gridway_baseapp::{Account, BaseApp};
use gridway_client::{Coin, TxBuilder};
use gridway_crypto::Address;

/// Default denomination used in tests.
pub const TEST_DENOM: &str = "ugridway";

/// Default chain ID used in tests.
pub const TEST_CHAIN_ID: &str = "gridway-simtest";

// ─── SimState ────────────────────────────────────────────────────────────────

/// Tracks expected state (balances, sequences) alongside the real BaseApp,
/// so tests can assert invariants after executing transactions.
#[derive(Debug, Clone)]
pub struct SimState {
    /// Expected balances: address → denom → amount
    pub balances: HashMap<String, HashMap<String, u64>>,
    /// Expected sequences: address → next sequence
    pub sequences: HashMap<String, u64>,
}

impl SimState {
    /// Create a new empty SimState.
    pub fn new() -> Self {
        Self {
            balances: HashMap::new(),
            sequences: HashMap::new(),
        }
    }

    /// Set balance for an address/denom pair.
    pub fn set_balance(&mut self, address: &str, denom: &str, amount: u64) {
        self.balances
            .entry(address.to_string())
            .or_default()
            .insert(denom.to_string(), amount);
    }

    /// Get balance for an address/denom pair (defaults to 0).
    pub fn get_balance(&self, address: &str, denom: &str) -> u64 {
        self.balances
            .get(address)
            .and_then(|denoms| denoms.get(denom))
            .copied()
            .unwrap_or(0)
    }

    /// Compute total supply for a given denom across all tracked accounts.
    pub fn total_supply(&self, denom: &str) -> u64 {
        self.balances
            .values()
            .filter_map(|denoms| denoms.get(denom))
            .sum()
    }

    /// Apply a successful transfer: debit sender, credit receiver, bump sequence.
    pub fn apply_transfer(&mut self, from: &str, to: &str, denom: &str, amount: u64) {
        let from_bal = self.get_balance(from, denom);
        let to_bal = self.get_balance(to, denom);
        self.set_balance(from, denom, from_bal - amount);
        self.set_balance(to, denom, to_bal + amount);
    }

    /// Increment the expected sequence for an address.
    pub fn increment_sequence(&mut self, address: &str) {
        let seq = self.sequences.entry(address.to_string()).or_insert(0);
        *seq += 1;
    }

    /// Get the expected next sequence for an address.
    pub fn get_sequence(&self, address: &str) -> u64 {
        self.sequences.get(address).copied().unwrap_or(0)
    }

    /// Verify that on-chain balances (from BaseApp) match expected state
    /// for all tracked accounts and denoms.
    pub fn verify_balances(&self, app: &BaseApp) -> Result<(), String> {
        for (address, denoms) in &self.balances {
            for (denom, &expected) in denoms {
                let actual = app
                    .get_balance(address, denom)
                    .map_err(|e| format!("get_balance({address}, {denom}): {e}"))?;
                if actual != expected {
                    return Err(format!(
                        "balance mismatch for {address}/{denom}: expected {expected}, got {actual}"
                    ));
                }
            }
        }
        Ok(())
    }
}

impl Default for SimState {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Key generation ──────────────────────────────────────────────────────────

/// Generate a deterministic ed25519 keypair from a seed.
///
/// Returns `(PrivateKey, hex-encoded public key, hex address)`.
pub fn random_keypair(seed: u64) -> (PrivateKey, String, String) {
    let private_key = PrivateKey::from_seed(seed);
    let public_key = private_key.public_key();
    let pk_hex = hex::encode(public_key.as_ref());
    let address = Address::from_public_key(&public_key).to_hex();
    (private_key, pk_hex, address)
}

// ─── TX building ─────────────────────────────────────────────────────────────

/// Build a signed bank.MsgSend transaction as raw bytes (JSON).
///
/// This produces the exact format consumed by `BaseApp::execute_block`.
pub fn build_transfer_tx(
    from_key: &PrivateKey,
    to_addr: &str,
    amount: u64,
    denom: &str,
    sequence: u64,
) -> Vec<u8> {
    let tx = TxBuilder::new(from_key.clone())
        .chain_id(TEST_CHAIN_ID)
        .sequence(sequence)
        .bank_send(to_addr, vec![Coin::new(denom, amount)])
        .build()
        .expect("TxBuilder::build should not fail with valid inputs");

    serde_json::to_vec(&tx).expect("SignedTx serialization should not fail")
}

// ─── Genesis setup ───────────────────────────────────────────────────────────

/// Create a fresh BaseApp with `n_accounts` genesis accounts, each holding
/// `initial_balance` of `TEST_DENOM`.
///
/// Returns `(BaseApp, Vec<(PrivateKey, address)>)` and a `SimState` that
/// mirrors the genesis state.
pub fn setup_genesis(
    n_accounts: usize,
    initial_balance: u64,
) -> (BaseApp, Vec<(PrivateKey, String)>, SimState) {
    let mut app = BaseApp::new("simapp".to_string()).expect("BaseApp::new should succeed");
    let mut accounts = Vec::with_capacity(n_accounts);
    let mut sim = SimState::new();

    for i in 0..n_accounts {
        let (private_key, pk_hex, address) = random_keypair(i as u64 + 100);

        // Register account in auth store
        app.set_account(
            &address,
            &Account {
                public_key: pk_hex,
                sequence: 0,
            },
        )
        .expect("set_account should succeed");

        // Set initial balance
        app.set_balance(&address, TEST_DENOM, initial_balance)
            .expect("set_balance should succeed");

        // Mirror in SimState
        sim.set_balance(&address, TEST_DENOM, initial_balance);

        accounts.push((private_key, address));
    }

    // Commit genesis state
    app.commit().expect("genesis commit should succeed");

    (app, accounts, sim)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_random_keypair_deterministic() {
        let (_, pk1, addr1) = random_keypair(42);
        let (_, pk2, addr2) = random_keypair(42);
        assert_eq!(pk1, pk2);
        assert_eq!(addr1, addr2);

        let (_, pk3, addr3) = random_keypair(43);
        assert_ne!(pk1, pk3);
        assert_ne!(addr1, addr3);
    }

    #[test]
    fn test_setup_genesis() {
        let (app, accounts, sim) = setup_genesis(3, 1_000_000);
        assert_eq!(accounts.len(), 3);

        for (_, addr) in &accounts {
            assert_eq!(app.get_balance(addr, TEST_DENOM).unwrap(), 1_000_000);
            assert_eq!(sim.get_balance(addr, TEST_DENOM), 1_000_000);
        }

        assert_eq!(sim.total_supply(TEST_DENOM), 3_000_000);
    }

    #[test]
    fn test_build_transfer_tx_produces_valid_json() {
        let (key, _, _) = random_keypair(1);
        let tx_bytes = build_transfer_tx(&key, "deadbeef", 500, TEST_DENOM, 0);
        let parsed: serde_json::Value = serde_json::from_slice(&tx_bytes).unwrap();
        assert!(parsed["body"]["messages"][0]["@type"] == "bank.MsgSend");
    }

    #[test]
    fn test_sim_state_apply_transfer() {
        let mut sim = SimState::new();
        sim.set_balance("alice", "ugridway", 1000);
        sim.set_balance("bob", "ugridway", 500);

        sim.apply_transfer("alice", "bob", "ugridway", 200);
        assert_eq!(sim.get_balance("alice", "ugridway"), 800);
        assert_eq!(sim.get_balance("bob", "ugridway"), 700);
        assert_eq!(sim.total_supply("ugridway"), 1500);
    }

    #[test]
    fn test_sim_state_verify_balances() {
        let (app, accounts, sim) = setup_genesis(2, 5000);
        // SimState should match BaseApp at genesis
        sim.verify_balances(&app).expect("balances should match at genesis");
    }
}
