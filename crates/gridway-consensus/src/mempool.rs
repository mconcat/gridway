//! Production-grade mempool with size limits, duplicate detection, and spam defense.

use sha2::{Digest, Sha256};
use std::collections::{HashSet, VecDeque};
use std::fmt;

/// Default maximum number of pending transactions.
pub const DEFAULT_MAX_TXS: usize = 10_000;

/// Default maximum size of a single transaction in bytes (256 KB).
pub const DEFAULT_MAX_TX_SIZE: usize = 256 * 1024;

/// Default maximum total size of all pending transactions in bytes (64 MB).
pub const DEFAULT_MAX_TOTAL_SIZE: usize = 64 * 1024 * 1024;

/// Errors that can occur when submitting a transaction to the mempool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MempoolError {
    /// The transaction exceeds the maximum allowed size.
    TxTooLarge { size: usize, max: usize },
    /// The mempool is full (either by count or total size).
    MempoolFull { reason: String },
    /// A transaction with the same hash already exists in the mempool.
    DuplicateTx { tx_hash: String },
    /// The mempool lock is poisoned (internal error).
    LockPoisoned,
}

impl fmt::Display for MempoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MempoolError::TxTooLarge { size, max } => {
                write!(f, "transaction too large: {} bytes (max {})", size, max)
            }
            MempoolError::MempoolFull { reason } => {
                write!(f, "mempool full: {}", reason)
            }
            MempoolError::DuplicateTx { tx_hash } => {
                write!(f, "duplicate transaction: {}", tx_hash)
            }
            MempoolError::LockPoisoned => {
                write!(f, "internal error: mempool lock poisoned")
            }
        }
    }
}

impl std::error::Error for MempoolError {}

/// Configuration for the mempool.
#[derive(Debug, Clone)]
pub struct MempoolConfig {
    /// Maximum number of transactions allowed in the mempool.
    pub max_txs: usize,
    /// Maximum size of an individual transaction in bytes.
    pub max_tx_size: usize,
    /// Maximum total size of all transactions in bytes.
    pub max_total_size: usize,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        Self {
            max_txs: DEFAULT_MAX_TXS,
            max_tx_size: DEFAULT_MAX_TX_SIZE,
            max_total_size: DEFAULT_MAX_TOTAL_SIZE,
        }
    }
}

/// Production-grade mempool with size limits and duplicate detection.
///
/// Tracks pending transactions with:
/// - Maximum transaction count limit
/// - Maximum individual transaction size limit
/// - Maximum total byte size limit
/// - SHA-256 based duplicate detection
pub struct Mempool {
    config: MempoolConfig,
    /// Pending transactions in FIFO order.
    txs: VecDeque<Vec<u8>>,
    /// SHA-256 hashes of pending transactions for O(1) duplicate detection.
    seen_hashes: HashSet<[u8; 32]>,
    /// Current total size in bytes of all pending transactions.
    total_size: usize,
}

impl Mempool {
    /// Create a new mempool with the given configuration.
    pub fn new(config: MempoolConfig) -> Self {
        Self {
            config,
            txs: VecDeque::new(),
            seen_hashes: HashSet::new(),
            total_size: 0,
        }
    }

    /// Compute the SHA-256 hash of a transaction body.
    fn tx_hash(tx: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(tx);
        let result = hasher.finalize();
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&result);
        hash
    }

    /// Submit a transaction to the mempool.
    ///
    /// Returns the hex-encoded SHA-256 hash on success, or a `MempoolError` on failure.
    pub fn submit(&mut self, tx: Vec<u8>) -> Result<String, MempoolError> {
        let tx_size = tx.len();

        // Check individual transaction size
        if tx_size > self.config.max_tx_size {
            return Err(MempoolError::TxTooLarge {
                size: tx_size,
                max: self.config.max_tx_size,
            });
        }

        // Check transaction count limit
        if self.txs.len() >= self.config.max_txs {
            return Err(MempoolError::MempoolFull {
                reason: format!(
                    "transaction count limit reached ({}/{})",
                    self.txs.len(),
                    self.config.max_txs
                ),
            });
        }

        // Check total size limit
        if self.total_size + tx_size > self.config.max_total_size {
            return Err(MempoolError::MempoolFull {
                reason: format!(
                    "total size limit would be exceeded ({} + {} > {})",
                    self.total_size, tx_size, self.config.max_total_size
                ),
            });
        }

        // Check for duplicates
        let hash = Self::tx_hash(&tx);
        let hash_hex = hex::encode(hash);
        if self.seen_hashes.contains(&hash) {
            return Err(MempoolError::DuplicateTx { tx_hash: hash_hex });
        }

        // All checks passed — insert
        self.seen_hashes.insert(hash);
        self.total_size += tx_size;
        self.txs.push_back(tx);

        tracing::info!(
            pending_count = self.txs.len(),
            total_size = self.total_size,
            tx_hash = %hash_hex,
            "TX submitted to mempool"
        );

        Ok(hash_hex)
    }

    /// Drain up to `max_count` transactions from the front of the mempool.
    ///
    /// Also removes the corresponding hashes from the dedup set and updates total size.
    pub fn drain(&mut self, max_count: usize) -> Vec<Vec<u8>> {
        let count = self.txs.len().min(max_count);
        if count == 0 {
            return Vec::new();
        }

        let drained: Vec<Vec<u8>> = self.txs.drain(..count).collect();

        for tx in &drained {
            let hash = Self::tx_hash(tx);
            self.seen_hashes.remove(&hash);
            self.total_size -= tx.len();
        }

        tracing::info!(
            drained = drained.len(),
            remaining = self.txs.len(),
            total_size = self.total_size,
            "Drained txs from mempool"
        );

        drained
    }

    /// Return the number of pending transactions.
    pub fn len(&self) -> usize {
        self.txs.len()
    }

    /// Return whether the mempool is empty.
    pub fn is_empty(&self) -> bool {
        self.txs.is_empty()
    }

    /// Return the current total size of all pending transactions in bytes.
    pub fn total_size(&self) -> usize {
        self.total_size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> MempoolConfig {
        MempoolConfig {
            max_txs: 5,
            max_tx_size: 100,
            max_total_size: 300,
        }
    }

    #[test]
    fn test_submit_and_drain() {
        let mut pool = Mempool::new(test_config());

        let hash = pool.submit(vec![1, 2, 3]).expect("submit should succeed");
        assert!(!hash.is_empty());
        assert_eq!(pool.len(), 1);
        assert_eq!(pool.total_size(), 3);

        let drained = pool.drain(10);
        assert_eq!(drained.len(), 1);
        assert_eq!(drained[0], vec![1, 2, 3]);
        assert_eq!(pool.len(), 0);
        assert_eq!(pool.total_size(), 0);
    }

    #[test]
    fn test_tx_too_large() {
        let mut pool = Mempool::new(test_config());

        let big_tx = vec![0u8; 101]; // exceeds max_tx_size=100
        let result = pool.submit(big_tx);
        assert!(result.is_err());
        match result {
            Err(MempoolError::TxTooLarge { size, max }) => {
                assert_eq!(size, 101);
                assert_eq!(max, 100);
            }
            other => panic!("expected TxTooLarge, got {:?}", other),
        }
        assert_eq!(pool.len(), 0);
    }

    #[test]
    fn test_mempool_full_by_count() {
        let mut pool = Mempool::new(test_config());

        for i in 0..5 {
            pool.submit(vec![i]).expect("submit should succeed");
        }
        assert_eq!(pool.len(), 5);

        let result = pool.submit(vec![99]);
        assert!(result.is_err());
        match result {
            Err(MempoolError::MempoolFull { .. }) => {}
            other => panic!("expected MempoolFull, got {:?}", other),
        }
    }

    #[test]
    fn test_mempool_full_by_total_size() {
        let config = MempoolConfig {
            max_txs: 100,
            max_tx_size: 200,
            max_total_size: 50,
        };
        let mut pool = Mempool::new(config);

        pool.submit(vec![0u8; 30]).expect("first submit");
        assert_eq!(pool.total_size(), 30);

        // This would push total to 55 > 50
        let result = pool.submit(vec![1u8; 25]);
        assert!(result.is_err());
        match result {
            Err(MempoolError::MempoolFull { .. }) => {}
            other => panic!("expected MempoolFull, got {:?}", other),
        }
    }

    #[test]
    fn test_duplicate_tx() {
        let mut pool = Mempool::new(test_config());

        pool.submit(vec![1, 2, 3]).expect("first submit");
        let result = pool.submit(vec![1, 2, 3]);
        assert!(result.is_err());
        match result {
            Err(MempoolError::DuplicateTx { .. }) => {}
            other => panic!("expected DuplicateTx, got {:?}", other),
        }
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn test_duplicate_allowed_after_drain() {
        let mut pool = Mempool::new(test_config());

        pool.submit(vec![1, 2, 3]).expect("first submit");
        pool.drain(10);

        // Same tx should be allowed after drain
        pool.submit(vec![1, 2, 3]).expect("resubmit after drain should succeed");
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn test_drain_partial() {
        let mut pool = Mempool::new(test_config());

        for i in 0u8..5 {
            pool.submit(vec![i]).expect("submit");
        }
        assert_eq!(pool.len(), 5);

        let drained = pool.drain(2);
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0], vec![0]);
        assert_eq!(drained[1], vec![1]);
        assert_eq!(pool.len(), 3);
        assert_eq!(pool.total_size(), 3);
    }

    #[test]
    fn test_drain_empty() {
        let mut pool = Mempool::new(test_config());
        let drained = pool.drain(10);
        assert!(drained.is_empty());
    }

    #[test]
    fn test_total_size_tracking() {
        let mut pool = Mempool::new(test_config());

        pool.submit(vec![1, 2, 3]).expect("submit 3 bytes");
        pool.submit(vec![4, 5]).expect("submit 2 bytes");
        assert_eq!(pool.total_size(), 5);

        pool.drain(1); // drain the 3-byte tx
        assert_eq!(pool.total_size(), 2);

        pool.drain(1); // drain the 2-byte tx
        assert_eq!(pool.total_size(), 0);
    }

    #[test]
    fn test_hash_deterministic() {
        let hash1 = Mempool::tx_hash(&[1, 2, 3]);
        let hash2 = Mempool::tx_hash(&[1, 2, 3]);
        assert_eq!(hash1, hash2);

        let hash3 = Mempool::tx_hash(&[3, 2, 1]);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_is_empty() {
        let mut pool = Mempool::new(test_config());
        assert!(pool.is_empty());
        pool.submit(vec![1]).expect("submit");
        assert!(!pool.is_empty());
    }

    #[test]
    fn test_error_display() {
        let err = MempoolError::TxTooLarge { size: 500, max: 100 };
        assert!(err.to_string().contains("500"));
        assert!(err.to_string().contains("100"));

        let err = MempoolError::DuplicateTx { tx_hash: "abc".to_string() };
        assert!(err.to_string().contains("abc"));

        let err = MempoolError::MempoolFull { reason: "test".to_string() };
        assert!(err.to_string().contains("test"));

        let err = MempoolError::LockPoisoned;
        assert!(err.to_string().contains("poisoned"));
    }
}
