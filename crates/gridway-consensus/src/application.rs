//! GridwayApp — the Commonware Application implementation.
//!
//! Wraps the gridway BaseApp to implement the consensus Application trait.
//! This is where consensus meets the WASM microkernel:
//!
//! - `genesis()` creates the initial block
//! - `propose()` collects pending txs, executes them, returns a GridwayBlock
//! - `verify()` re-executes the proposed block's txs and checks state root
//! - `report()` commits state on finalization

use gridway_baseapp::BaseApp;
use gridway_types::GridwayBlock;

use crate::mempool::{Mempool, MempoolConfig, MempoolError};
use crate::types::{GridwayScheme, PublicKey};

use commonware_consensus::{
    marshal::{ingress::mailbox::AncestorStream, Update},
    simplex::types::Context,
    Heightable, Reporter,
};
use commonware_cryptography::{sha256::Digest, Digestible};
use commonware_runtime::{Clock, Metrics, Spawner};
use commonware_utils::Acknowledgement;
use commonware_utils::SystemTimeExt;
use futures::StreamExt;
use rand::Rng;
use std::sync::{Arc, Mutex, RwLock};
use tracing::info;

/// Milliseconds in the future to allow for block timestamps.
const SYNCHRONY_BOUND: u64 = 500;

/// GridwayApp wraps BaseApp and implements Commonware consensus traits.
///
/// The Application trait is what the simplex consensus engine calls
/// to propose and verify blocks. GridwayApp bridges these calls to
/// the WASM microkernel (BaseApp).
#[derive(Clone)]
pub struct GridwayApp {
    /// The chain ID used for block execution.
    chain_id: Arc<String>,

    /// The genesis block (cached)
    genesis: Arc<GridwayBlock>,

    /// Shared reference to the BaseApp (thread-safe)
    /// The BaseApp manages all WASM module execution and state.
    /// Uses RwLock so that read-only HTTP queries can run concurrently,
    /// while propose/verify/report take exclusive write locks.
    baseapp: Arc<RwLock<BaseApp>>,

    /// Production-grade mempool with size limits and duplicate detection.
    mempool: Arc<Mutex<Mempool>>,
}

impl GridwayApp {
    /// Create a new GridwayApp wrapping a BaseApp with the given chain ID.
    /// Uses default mempool configuration.
    pub fn new(baseapp: BaseApp, chain_id: String) -> Self {
        Self::with_mempool_config(baseapp, chain_id, MempoolConfig::default())
    }

    /// Create a new GridwayApp with a custom mempool configuration.
    pub fn with_mempool_config(
        baseapp: BaseApp,
        chain_id: String,
        mempool_config: MempoolConfig,
    ) -> Self {
        let genesis = GridwayBlock::genesis();
        Self {
            chain_id: Arc::new(chain_id),
            genesis: Arc::new(genesis),
            baseapp: Arc::new(RwLock::new(baseapp)),
            mempool: Arc::new(Mutex::new(Mempool::new(mempool_config))),
        }
    }

    /// Return the chain ID.
    pub fn chain_id(&self) -> &str {
        &self.chain_id
    }

    /// Submit a transaction to the mempool.
    ///
    /// Returns the hex-encoded SHA-256 hash on success, or a `MempoolError` on failure.
    pub fn submit_tx(&self, tx: Vec<u8>) -> Result<String, MempoolError> {
        match self.mempool.lock() {
            Ok(mut pool) => pool.submit(tx),
            Err(_) => Err(MempoolError::LockPoisoned),
        }
    }

    /// Return the number of pending transactions.
    pub fn pending_tx_count(&self) -> usize {
        self.mempool.lock().map(|pool| pool.len()).unwrap_or(0)
    }

    /// Get access to the BaseApp (for queries, etc.)
    /// Returns an RwLock — HTTP query handlers should use `.read()`,
    /// while state-mutating operations use `.write()`.
    pub fn baseapp(&self) -> &Arc<RwLock<BaseApp>> {
        &self.baseapp
    }

    /// Drain pending transactions (up to max_count)
    fn drain_pending(&self, max_count: usize) -> Vec<Vec<u8>> {
        match self.mempool.lock() {
            Ok(mut pool) => pool.drain(max_count),
            Err(e) => {
                tracing::error!("mempool lock poisoned in drain_pending: {e}");
                Vec::new()
            }
        }
    }

    /// Re-insert drained transactions back into the mempool.
    ///
    /// Called when block execution fails so that the drained transactions
    /// are not lost.
    fn requeue_txs(&self, txs: Vec<Vec<u8>>) {
        match self.mempool.lock() {
            Ok(mut pool) => {
                let count = txs.len();
                pool.requeue(txs);
                tracing::info!(count, "requeued transactions after execution failure");
            }
            Err(e) => {
                tracing::error!(
                    "mempool lock poisoned in requeue_txs: {e} — {} txs lost",
                    txs.len()
                );
            }
        }
    }

    /// Replay a sequence of finalized blocks to rebuild state.
    ///
    /// Used on node restart to catch up BaseApp with persisted block history.
    /// Genesis state must already be applied before calling this method.
    pub fn replay_blocks(&self, blocks: &[GridwayBlock]) -> std::result::Result<(), String> {
        let mut app = self
            .baseapp
            .write()
            .map_err(|e| format!("write lock: {e}"))?;

        for block in blocks {
            let height = block.height.get();
            match app.execute_block(height, block.timestamp, &self.chain_id, &block.transactions) {
                Ok((state_root, _responses)) => {
                    if state_root != block.state_root {
                        return Err(format!(
                            "state root mismatch at height {}: expected {}, got {}",
                            height,
                            hex::encode(block.state_root),
                            hex::encode(state_root)
                        ));
                    }
                    app.commit()
                        .map_err(|e| format!("commit at height {height}: {e}"))?;
                    info!(
                        height,
                        state_root = hex::encode(state_root),
                        "replayed block"
                    );
                }
                Err(e) => return Err(format!("execute_block at height {height}: {e}")),
            }
        }
        Ok(())
    }
}

impl<E> commonware_consensus::Application<E> for GridwayApp
where
    E: Rng + Spawner + Metrics + Clock,
{
    type SigningScheme = GridwayScheme;
    type Context = Context<Digest, PublicKey>;
    type Block = GridwayBlock;

    /// Return the genesis block.
    async fn genesis(&mut self) -> Self::Block {
        self.genesis.as_ref().clone()
    }

    /// Propose a new block.
    ///
    /// 1. Get parent block from ancestry
    /// 2. Drain pending transactions
    /// 3. Execute them through BaseApp
    /// 4. Build GridwayBlock with state root
    async fn propose(
        &mut self,
        (runtime_context, _context): (E, Self::Context),
        mut ancestry: AncestorStream<Self::SigningScheme, Self::Block>,
    ) -> Option<Self::Block> {
        let parent = ancestry.next().await?;

        // Calculate timestamp
        let mut current = runtime_context.current().epoch_millis();
        if current <= parent.timestamp {
            current = parent.timestamp + 1;
        }

        let new_height = parent.height.next();

        // Drain pending transactions
        let txs = self.drain_pending(1000); // Max 1000 txs per block

        // Execute through BaseApp to get state root.
        // Restore to committed state first so we always execute from a
        // clean baseline (not leftover state from a previous verify/propose).
        let mut app = match self.baseapp.write() {
            Ok(app) => app,
            Err(e) => {
                tracing::error!("baseapp write lock poisoned in propose: {e}");
                return None;
            }
        };
        if let Err(e) = app.restore_to_committed() {
            tracing::error!(
                height = new_height.get(),
                "failed to restore committed state in propose: {e}"
            );
            return None;
        }
        match app.execute_block(new_height.get(), current, &self.chain_id, &txs) {
            Ok((state_root, _responses)) => Some(GridwayBlock::new(
                parent.digest(),
                new_height,
                current,
                state_root,
                txs,
            )),
            Err(e) => {
                tracing::error!(height = new_height.get(), "block execution failed: {e}");
                // On failure, restore to committed and propose an empty block.
                let _ = app.restore_to_committed();
                let stale_root = *app.last_state_root();
                // Drop the baseapp lock before requeuing (requeue needs mempool lock only)
                drop(app);
                // Re-insert drained transactions so they aren't lost
                self.requeue_txs(txs);
                Some(GridwayBlock::new(
                    parent.digest(),
                    new_height,
                    current,
                    stale_root,
                    Vec::new(), // empty — no txs since execution failed
                ))
            }
        }
    }
}

impl<E> commonware_consensus::VerifyingApplication<E> for GridwayApp
where
    E: Rng + Spawner + Metrics + Clock,
{
    /// Verify a proposed block.
    ///
    /// Re-executes the block's transactions through BaseApp and checks
    /// that the resulting state root matches the proposed state root.
    async fn verify(
        &mut self,
        (runtime_context, _): (E, Self::Context),
        mut ancestry: AncestorStream<Self::SigningScheme, Self::Block>,
    ) -> bool {
        let Some(block) = ancestry.next().await else {
            return false;
        };
        let Some(parent) = ancestry.next().await else {
            return false;
        };

        // Verify timestamp
        if block.timestamp <= parent.timestamp {
            return false;
        }
        let current = runtime_context.current().epoch_millis();
        if block.timestamp > current + SYNCHRONY_BOUND {
            return false;
        }

        // Re-execute transactions ephemerally and verify state root.
        //
        // execute_block_ephemeral() checkpoints the store, executes the
        // block, reads the resulting state root, then restores the
        // checkpoint.  This ensures verify() NEVER mutates shared state,
        // so a verified-but-not-finalized block cannot pollute the trie.
        let verified = {
            let mut app = match self.baseapp.write() {
                Ok(app) => app,
                Err(e) => {
                    tracing::error!("baseapp write lock poisoned in verify: {e}");
                    return false;
                }
            };
            match app.execute_block_ephemeral(
                block.height.get(),
                block.timestamp,
                &self.chain_id,
                &block.transactions,
            ) {
                Ok(computed_root) => computed_root == block.state_root,
                Err(e) => {
                    tracing::error!("Block verification failed: {}", e);
                    false
                }
            }
        };

        verified
    }
}

impl Reporter for GridwayApp {
    type Activity = Update<GridwayBlock>;

    /// Called when a block is finalized by consensus.
    ///
    /// Commits the state to the MerkleStore, making it permanent.
    /// Only acknowledges finalization if the commit succeeds — refusing to
    /// ack on failure prevents the consensus engine from advancing past a
    /// block whose state was not durably persisted.
    async fn report(&mut self, activity: Self::Activity) {
        if let Update::Block(block, ack_rx) = activity {
            info!(height = %block.height(), txs = block.transactions.len(), "finalized block");

            let committed = match self.baseapp.write() {
                Ok(mut app) => {
                    // Restore to committed state first so we always execute
                    // the winning block from a clean baseline.
                    if let Err(e) = app.restore_to_committed() {
                        tracing::error!(
                            height = %block.height(),
                            error = %e,
                            "CRITICAL: failed to restore committed state in report"
                        );
                        return;
                    }

                    // Execute the finalized block
                    match app.execute_block(
                        block.height().get(),
                        block.timestamp,
                        &self.chain_id,
                        &block.transactions,
                    ) {
                        Ok(_) => {}
                        Err(e) => {
                            tracing::error!(
                                height = %block.height(),
                                error = %e,
                                "CRITICAL: execute_block failed in report — NOT committing"
                            );
                            return;
                        }
                    }

                    // Commit to persistent store
                    match app.commit() {
                        Ok(root) => {
                            info!(
                                height = %block.height(),
                                state_root = hex::encode(root),
                                "committed state"
                            );
                            true
                        }
                        Err(e) => {
                            tracing::error!(
                                height = %block.height(),
                                error = %e,
                                "CRITICAL: state commit failed — NOT acknowledging finalization"
                            );
                            false
                        }
                    }
                }
                Err(e) => {
                    tracing::error!(
                        height = %block.height(),
                        "CRITICAL: baseapp write lock poisoned in report — NOT acknowledging: {e}"
                    );
                    false
                }
            };

            if committed {
                ack_rx.acknowledge();
            } else {
                tracing::error!(
                    height = %block.height(),
                    "finalization NOT acknowledged — node may stall until restart"
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{GenesisAccount, GenesisBalance, GenesisConfig};
    use crate::mempool::MempoolError;

    #[test]
    fn test_gridway_app_creation() {
        let baseapp = BaseApp::new("test".to_string()).expect("baseapp creation failed");
        let app = GridwayApp::new(baseapp, "test-chain".to_string());

        // Should be able to submit transactions
        let hash1 = app.submit_tx(vec![1, 2, 3]).expect("submit should succeed");
        let hash2 = app.submit_tx(vec![4, 5, 6]).expect("submit should succeed");
        assert!(!hash1.is_empty());
        assert!(!hash2.is_empty());
        assert_ne!(hash1, hash2);

        // Drain should return them
        let txs = app.drain_pending(10);
        assert_eq!(txs.len(), 2);

        // Chain ID should be set
        assert_eq!(app.chain_id(), "test-chain");
    }

    #[test]
    fn test_gridway_app_pending_limit() {
        let baseapp = BaseApp::new("test".to_string()).expect("baseapp creation failed");
        let app = GridwayApp::new(baseapp, "test-chain".to_string());

        for i in 0..100u16 {
            // Use 2 bytes per tx to ensure uniqueness
            app.submit_tx(i.to_le_bytes().to_vec())
                .expect("submit should succeed");
        }

        // Drain with limit
        let txs = app.drain_pending(10);
        assert_eq!(txs.len(), 10);

        // Remaining
        let txs = app.drain_pending(1000);
        assert_eq!(txs.len(), 90);
    }

    #[test]
    fn test_gridway_app_submit_returns_error_on_duplicate() {
        let baseapp = BaseApp::new("test".to_string()).expect("baseapp creation failed");
        let app = GridwayApp::new(baseapp, "test-chain".to_string());

        app.submit_tx(vec![1, 2, 3]).expect("first submit");
        let result = app.submit_tx(vec![1, 2, 3]);
        assert!(matches!(result, Err(MempoolError::DuplicateTx { .. })));
    }

    #[test]
    fn test_gridway_app_submit_tx_too_large() {
        let config = MempoolConfig {
            max_txs: 100,
            max_tx_size: 10,
            max_total_size: 1000,
        };
        let baseapp = BaseApp::new("test".to_string()).expect("baseapp creation failed");
        let app = GridwayApp::with_mempool_config(baseapp, "test-chain".to_string(), config);

        let result = app.submit_tx(vec![0u8; 11]);
        assert!(matches!(result, Err(MempoolError::TxTooLarge { .. })));
    }

    #[test]
    fn test_gridway_app_mempool_full() {
        let config = MempoolConfig {
            max_txs: 3,
            max_tx_size: 100,
            max_total_size: 1000,
        };
        let baseapp = BaseApp::new("test".to_string()).expect("baseapp creation failed");
        let app = GridwayApp::with_mempool_config(baseapp, "test-chain".to_string(), config);

        app.submit_tx(vec![1]).expect("submit 1");
        app.submit_tx(vec![2]).expect("submit 2");
        app.submit_tx(vec![3]).expect("submit 3");

        let result = app.submit_tx(vec![4]);
        assert!(matches!(result, Err(MempoolError::MempoolFull { .. })));
    }

    #[test]
    fn test_gridway_app_pending_tx_count() {
        let baseapp = BaseApp::new("test".to_string()).expect("baseapp creation failed");
        let app = GridwayApp::new(baseapp, "test-chain".to_string());

        assert_eq!(app.pending_tx_count(), 0);
        app.submit_tx(vec![1]).expect("submit");
        assert_eq!(app.pending_tx_count(), 1);
        app.submit_tx(vec![2]).expect("submit");
        assert_eq!(app.pending_tx_count(), 2);
        app.drain_pending(1);
        assert_eq!(app.pending_tx_count(), 1);
    }

    #[test]
    fn test_genesis_loading_balance() {
        let genesis = GenesisConfig {
            chain_id: "test-genesis".to_string(),
            accounts: vec![GenesisAccount {
                address: "aabbccddee00112233445566778899aabbccddee".to_string(),
                public_key_hex: "00".repeat(32),
                balances: vec![GenesisBalance {
                    denom: "ugridway".to_string(),
                    amount: 1_000_000,
                }],
            }],
        };

        let mut baseapp =
            BaseApp::new("test-genesis".to_string()).expect("baseapp creation failed");

        // Apply genesis
        for account in &genesis.accounts {
            baseapp
                .set_account(
                    &account.address,
                    &gridway_baseapp::Account {
                        public_key: account.public_key_hex.clone(),
                        sequence: 0,
                    },
                )
                .expect("set_account failed");

            for balance in &account.balances {
                baseapp
                    .set_balance(&account.address, &balance.denom, balance.amount)
                    .expect("set_balance failed");
            }
        }

        let root = baseapp.commit().expect("commit failed");
        // Genesis commit should produce a non-zero hash
        assert_ne!(root, [0u8; 32], "genesis state root should be non-zero");

        // Verify balance
        let bal = baseapp
            .get_balance("aabbccddee00112233445566778899aabbccddee", "ugridway")
            .expect("get_balance failed");
        assert_eq!(bal, 1_000_000);
    }

    #[test]
    fn test_genesis_config_yaml_roundtrip() {
        let genesis = GenesisConfig {
            chain_id: "test-chain-1".to_string(),
            accounts: vec![GenesisAccount {
                address: "aabbccddee00112233445566778899aabbccddee".to_string(),
                public_key_hex: "aa".repeat(32),
                balances: vec![
                    GenesisBalance {
                        denom: "ugridway".to_string(),
                        amount: 500_000,
                    },
                    GenesisBalance {
                        denom: "uatom".to_string(),
                        amount: 100,
                    },
                ],
            }],
        };

        let yaml = serde_yaml::to_string(&genesis).expect("serialize failed");
        let parsed: GenesisConfig = serde_yaml::from_str(&yaml).expect("deserialize failed");

        assert_eq!(parsed.chain_id, "test-chain-1");
        assert_eq!(parsed.accounts.len(), 1);
        assert_eq!(parsed.accounts[0].balances.len(), 2);
        assert_eq!(parsed.accounts[0].balances[0].amount, 500_000);
    }
}
