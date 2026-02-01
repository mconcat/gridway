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

use crate::types::{GridwayScheme, PublicKey};

use commonware_consensus::{
    marshal::{ingress::mailbox::AncestorStream, Update},
    simplex::types::Context,
    Heightable, Reporter,
};
use commonware_cryptography::{sha256::Digest, Digestible};
use commonware_runtime::{Clock, Metrics, Spawner};
use commonware_utils::SystemTimeExt;
use commonware_utils::Acknowledgement;
use futures::StreamExt;
use rand::Rng;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use tracing::info;

/// Milliseconds in the future to allow for block timestamps.
const SYNCHRONY_BOUND: u64 = 500;

/// The chain ID used for block execution.
const CHAIN_ID: &str = "gridway-1";

/// GridwayApp wraps BaseApp and implements Commonware consensus traits.
///
/// The Application trait is what the simplex consensus engine calls
/// to propose and verify blocks. GridwayApp bridges these calls to
/// the WASM microkernel (BaseApp).
#[derive(Clone)]
pub struct GridwayApp {
    /// The genesis block (cached)
    genesis: Arc<GridwayBlock>,

    /// Shared reference to the BaseApp (thread-safe)
    /// The BaseApp manages all WASM module execution and state.
    baseapp: Arc<Mutex<BaseApp>>,

    /// Pending transactions waiting to be included in a block.
    /// In a full implementation, this would be a mempool.
    pending_txs: Arc<Mutex<VecDeque<Vec<u8>>>>,
}

impl GridwayApp {
    /// Create a new GridwayApp wrapping a BaseApp
    pub fn new(baseapp: BaseApp) -> Self {
        let genesis = GridwayBlock::genesis();
        Self {
            genesis: Arc::new(genesis),
            baseapp: Arc::new(Mutex::new(baseapp)),
            pending_txs: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    /// Submit a transaction to the pending pool
    pub fn submit_tx(&self, tx: Vec<u8>) {
        if let Ok(mut pending) = self.pending_txs.lock() {
            pending.push_back(tx);
            tracing::info!(pending_count = pending.len(), "TX submitted to pending pool");
        }
    }

    /// Get access to the BaseApp (for queries, etc.)
    pub fn baseapp(&self) -> &Arc<Mutex<BaseApp>> {
        &self.baseapp
    }

    /// Drain pending transactions (up to max_count)
    fn drain_pending(&self, max_count: usize) -> Vec<Vec<u8>> {
        let mut pending = self.pending_txs.lock().unwrap();
        let count = pending.len().min(max_count);
        if count > 0 {
            tracing::info!(drained = count, remaining = pending.len() - count, "Draining pending txs for proposal");
        }
        pending.drain(..count).collect()
    }

    /// Replay a sequence of finalized blocks to rebuild state.
    ///
    /// Used on node restart to catch up BaseApp with persisted block history.
    /// Genesis state must already be applied before calling this method.
    pub fn replay_blocks(&self, blocks: &[GridwayBlock]) -> std::result::Result<(), String> {
        let mut app = self.baseapp.lock().map_err(|e| format!("lock: {e}"))?;

        // Clear executed_heights since we're replaying from scratch.
        // BaseApp tracks these for idempotency within a session, but on
        // replay we need to re-execute all blocks.
        app.clear_executed_heights();

        for block in blocks {
            let height = block.height.get();
            match app.execute_block(height, block.timestamp, CHAIN_ID, &block.transactions) {
                Ok((state_root, _responses)) => {
                    if state_root != block.state_root {
                        return Err(format!(
                            "state root mismatch at height {}: expected {}, got {}",
                            height,
                            hex::encode(block.state_root),
                            hex::encode(state_root)
                        ));
                    }
                    app.commit().map_err(|e| format!("commit at height {height}: {e}"))?;
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

        // Execute through BaseApp to get state root
        let state_root = {
            let mut app = self.baseapp.lock().unwrap();
            match app.execute_block(
                new_height.get(),
                current,
                CHAIN_ID,
                &txs,
            ) {
                Ok((root, _responses)) => root,
                Err(e) => {
                    tracing::error!("Block execution failed: {}", e);
                    // On error, propose empty block with current state
                    *app.last_state_root()
                }
            }
        };

        Some(GridwayBlock::new(
            parent.digest(),
            new_height,
            current,
            state_root,
            txs,
        ))
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

        // Re-execute transactions and verify state root
        let verified = {
            let mut app = self.baseapp.lock().unwrap();
            match app.execute_block(
                block.height.get(),
                block.timestamp,
                CHAIN_ID,
                &block.transactions,
            ) {
                Ok((computed_root, _)) => computed_root == block.state_root,
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
    async fn report(&mut self, activity: Self::Activity) {
        if let Update::Block(block, ack_rx) = activity {
            info!(height = %block.height(), txs = block.transactions.len(), "finalized block");

            // Commit state on finalization
            {
                let mut app = self.baseapp.lock().unwrap();

                // Re-execute to ensure state is applied (idempotent if already executed)
                let _ = app.execute_block(
                    block.height().get(),
                    block.timestamp,
                    CHAIN_ID,
                    &block.transactions,
                );

                // Commit to persistent store
                match app.commit() {
                    Ok(root) => {
                        info!(
                            height = %block.height(),
                            state_root = hex::encode(root),
                            "committed state"
                        );
                    }
                    Err(e) => {
                        tracing::error!("State commit failed: {}", e);
                    }
                }
            }

            ack_rx.acknowledge();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gridway_app_creation() {
        let baseapp = BaseApp::new("test".to_string()).unwrap();
        let app = GridwayApp::new(baseapp);

        // Should be able to submit transactions
        app.submit_tx(vec![1, 2, 3]);
        app.submit_tx(vec![4, 5, 6]);

        // Drain should return them
        let txs = app.drain_pending(10);
        assert_eq!(txs.len(), 2);
    }

    #[test]
    fn test_gridway_app_pending_limit() {
        let baseapp = BaseApp::new("test".to_string()).unwrap();
        let app = GridwayApp::new(baseapp);

        for i in 0..100 {
            app.submit_tx(vec![i as u8]);
        }

        // Drain with limit
        let txs = app.drain_pending(10);
        assert_eq!(txs.len(), 10);

        // Remaining
        let txs = app.drain_pending(1000);
        assert_eq!(txs.len(), 90);
    }
}
