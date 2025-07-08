//! Snapshot management for state sync
//!
//! This module provides functionality for creating, storing, and restoring
//! blockchain state snapshots for fast node synchronization.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use helium_baseapp::BaseApp;
use thiserror::Error;
use tracing::{debug, error, info};

/// Snapshot errors
#[derive(Error, Debug)]
pub enum SnapshotError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid snapshot format
    #[error("Invalid snapshot format: {0}")]
    InvalidFormat(String),

    /// Snapshot not found
    #[error("Snapshot not found: height={0}")]
    NotFound(u64),

    /// Chunk not found
    #[error("Chunk not found: index={0}")]
    ChunkNotFound(u32),

    /// Invalid chunk
    #[error("Invalid chunk: {0}")]
    InvalidChunk(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// State error
    #[error("State error: {0}")]
    State(String),
}

pub type Result<T> = std::result::Result<T, SnapshotError>;

/// Snapshot metadata
#[derive(Debug, Clone)]
pub struct SnapshotMetadata {
    /// Block height
    pub height: u64,
    /// Snapshot format version
    pub format: u32,
    /// Number of chunks
    pub chunks: u32,
    /// SHA256 hash of the snapshot
    pub hash: Vec<u8>,
    /// Additional metadata (JSON encoded)
    pub metadata: Vec<u8>,
    /// Creation timestamp
    pub created_at: u64,
    /// Size in bytes
    pub size: u64,
}

/// Snapshot chunk
#[derive(Debug, Clone)]
pub struct SnapshotChunk {
    /// Chunk index
    pub index: u32,
    /// Chunk data
    pub data: Vec<u8>,
}

/// Snapshot manager handles creation and restoration of state snapshots
pub struct SnapshotManager {
    /// Snapshot storage directory
    snapshot_dir: PathBuf,
    /// Maximum number of snapshots to keep
    max_snapshots: usize,
    /// Chunk size in bytes (default: 16MB)
    chunk_size: usize,
    /// Cached snapshot metadata
    snapshots: Arc<RwLock<HashMap<u64, SnapshotMetadata>>>,
    /// Current snapshot format version
    format_version: u32,
}

impl SnapshotManager {
    /// Create a new snapshot manager
    pub fn new(snapshot_dir: PathBuf) -> Result<Self> {
        // Create snapshot directory if it doesn't exist
        std::fs::create_dir_all(&snapshot_dir)?;

        let mut manager = Self {
            snapshot_dir,
            max_snapshots: 3,
            chunk_size: 16 * 1024 * 1024, // 16MB chunks
            snapshots: Arc::new(RwLock::new(HashMap::new())),
            format_version: 1,
        };

        // Load existing snapshots
        manager.load_existing_snapshots()?;

        Ok(manager)
    }

    /// Load existing snapshots from disk
    fn load_existing_snapshots(&mut self) -> Result<()> {
        let entries = std::fs::read_dir(&self.snapshot_dir)?;

        for entry in entries {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if let Some(height_str) = path.file_name().and_then(|n| n.to_str()) {
                    if let Ok(height) = height_str.parse::<u64>() {
                        // Load metadata file
                        let metadata_path = path.join("metadata.json");
                        if metadata_path.exists() {
                            match self.load_snapshot_metadata(&metadata_path) {
                                Ok((height, metadata)) => {
                                    let mut snapshots = self.snapshots.blocking_write();
                                    snapshots.insert(height, metadata);
                                }
                                Err(e) => {
                                    error!(
                                        "Failed to load snapshot metadata at height {}: {}",
                                        height, e
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Load snapshot metadata from file
    fn load_snapshot_metadata(&self, path: &Path) -> Result<(u64, SnapshotMetadata)> {
        let data = std::fs::read(path)?;
        let metadata: serde_json::Value = serde_json::from_slice(&data)
            .map_err(|e| SnapshotError::Serialization(e.to_string()))?;

        let height = metadata["height"]
            .as_u64()
            .ok_or_else(|| SnapshotError::InvalidFormat("Missing height".to_string()))?;

        let format = metadata["format"]
            .as_u64()
            .ok_or_else(|| SnapshotError::InvalidFormat("Missing format".to_string()))?
            as u32;

        let chunks = metadata["chunks"]
            .as_u64()
            .ok_or_else(|| SnapshotError::InvalidFormat("Missing chunks".to_string()))?
            as u32;

        let hash_str = metadata["hash"]
            .as_str()
            .ok_or_else(|| SnapshotError::InvalidFormat("Missing hash".to_string()))?;

        let hash = hex::decode(hash_str)
            .map_err(|_| SnapshotError::InvalidFormat("Invalid hash encoding".to_string()))?;

        let metadata_str = metadata["metadata"].as_str().unwrap_or("{}");

        let created_at = metadata["created_at"]
            .as_u64()
            .ok_or_else(|| SnapshotError::InvalidFormat("Missing created_at".to_string()))?;

        let size = metadata["size"]
            .as_u64()
            .ok_or_else(|| SnapshotError::InvalidFormat("Missing size".to_string()))?;

        let snapshot_metadata = SnapshotMetadata {
            height,
            format,
            chunks,
            hash,
            metadata: metadata_str.as_bytes().to_vec(),
            created_at,
            size,
        };

        Ok((height, snapshot_metadata))
    }

    /// Create a new snapshot of the current state
    pub async fn create_snapshot(
        &self,
        app: Arc<RwLock<BaseApp>>,
        height: u64,
    ) -> Result<SnapshotMetadata> {
        info!("Creating snapshot at height {}", height);

        let snapshot_path = self.snapshot_dir.join(height.to_string());
        std::fs::create_dir_all(&snapshot_path)?;

        // Export state from BaseApp
        let app_guard = app.read().await;
        let state_data = self.export_state(&app_guard).await?;
        drop(app_guard);

        // Calculate hash
        let hash = self.calculate_hash(&state_data);

        // Split into chunks
        let chunks = self.split_into_chunks(&state_data);
        let num_chunks = chunks.len() as u32;

        // Save chunks
        for (index, chunk) in chunks.into_iter().enumerate() {
            let chunk_path = snapshot_path.join(format!("chunk_{index:06}.dat"));
            std::fs::write(chunk_path, chunk)?;
        }

        // Create metadata
        let metadata = SnapshotMetadata {
            height,
            format: self.format_version,
            chunks: num_chunks,
            hash: hash.clone(),
            metadata: serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "chain_id": "helium",
            })
            .to_string()
            .into_bytes(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            size: state_data.len() as u64,
        };

        // Save metadata
        let metadata_json = serde_json::json!({
            "height": metadata.height,
            "format": metadata.format,
            "chunks": metadata.chunks,
            "hash": hex::encode(&metadata.hash),
            "metadata": String::from_utf8_lossy(&metadata.metadata),
            "created_at": metadata.created_at,
            "size": metadata.size,
        });

        let metadata_path = snapshot_path.join("metadata.json");
        std::fs::write(
            metadata_path,
            serde_json::to_string_pretty(&metadata_json)
                .map_err(|e| SnapshotError::Serialization(e.to_string()))?,
        )?;

        // Update cached snapshots
        {
            let mut snapshots = self.snapshots.write().await;
            snapshots.insert(height, metadata.clone());
        }

        // Prune old snapshots if needed
        self.prune_old_snapshots().await?;

        info!(
            "Snapshot created at height {} with {} chunks",
            height, num_chunks
        );
        Ok(metadata)
    }

    /// Export state from BaseApp
    async fn export_state(&self, app: &BaseApp) -> Result<Vec<u8>> {
        let height = app.get_height();
        let app_hash = app.get_last_app_hash();

        // Create state export with metadata
        let state_export = serde_json::json!({
            "version": self.format_version,
            "height": height,
            "app_hash": hex::encode(app_hash),
            "timestamp": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            // Store data would be exported here
            // For now, we include minimal state information
            "stores": {
                "metadata": {
                    "height": height,
                    "hash": hex::encode(app_hash),
                }
            },
            // In a full implementation, we would iterate through all stores
            // and export their key-value pairs. This requires access to the
            // underlying storage layer which is not yet exposed in BaseApp.
            "note": "Full state export requires storage layer integration"
        });

        serde_json::to_vec(&state_export).map_err(|e| SnapshotError::Serialization(e.to_string()))
    }

    /// Calculate SHA256 hash of data
    fn calculate_hash(&self, data: &[u8]) -> Vec<u8> {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(data);
        hasher.finalize().to_vec()
    }

    /// Split data into chunks
    fn split_into_chunks(&self, data: &[u8]) -> Vec<Vec<u8>> {
        data.chunks(self.chunk_size)
            .map(|chunk| chunk.to_vec())
            .collect()
    }

    /// List available snapshots
    pub async fn list_snapshots(&self) -> Vec<SnapshotMetadata> {
        let snapshots = self.snapshots.read().await;
        let mut snapshot_list: Vec<_> = snapshots.values().cloned().collect();
        // Sort by height descending
        snapshot_list.sort_by(|a, b| b.height.cmp(&a.height));
        snapshot_list
    }

    /// Get snapshot metadata by height
    pub async fn get_snapshot(&self, height: u64) -> Result<SnapshotMetadata> {
        let snapshots = self.snapshots.read().await;
        snapshots
            .get(&height)
            .cloned()
            .ok_or(SnapshotError::NotFound(height))
    }

    /// Load a snapshot chunk
    pub async fn load_chunk(&self, height: u64, chunk_index: u32) -> Result<Vec<u8>> {
        let snapshot = self.get_snapshot(height).await?;

        if chunk_index >= snapshot.chunks {
            return Err(SnapshotError::ChunkNotFound(chunk_index));
        }

        let chunk_path = self
            .snapshot_dir
            .join(height.to_string())
            .join(format!("chunk_{chunk_index:06}.dat"));

        std::fs::read(&chunk_path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => SnapshotError::ChunkNotFound(chunk_index),
            _ => SnapshotError::Io(e),
        })
    }

    /// Verify snapshot integrity
    pub async fn verify_snapshot(&self, height: u64) -> Result<bool> {
        let metadata = self.get_snapshot(height).await?;

        // Load all chunks and verify hash
        let mut all_data = Vec::new();
        for i in 0..metadata.chunks {
            let chunk = self.load_chunk(height, i).await?;
            all_data.extend(chunk);
        }

        let calculated_hash = self.calculate_hash(&all_data);
        Ok(calculated_hash == metadata.hash)
    }

    /// Restore state from snapshot
    pub async fn restore_snapshot(
        &self,
        app: Arc<RwLock<BaseApp>>,
        height: u64,
        chunks: Vec<Vec<u8>>,
    ) -> Result<()> {
        info!("Restoring snapshot at height {}", height);

        // Combine chunks
        let mut state_data = Vec::new();
        for chunk in chunks {
            state_data.extend(chunk);
        }

        // Verify hash
        let metadata = self.get_snapshot(height).await?;
        let calculated_hash = self.calculate_hash(&state_data);
        if calculated_hash != metadata.hash {
            return Err(SnapshotError::InvalidFormat("Hash mismatch".to_string()));
        }

        // Import state to BaseApp
        let mut app_guard = app.write().await;
        self.import_state(&mut app_guard, &state_data).await?;

        info!("Snapshot restored successfully at height {}", height);
        Ok(())
    }

    /// Import state to BaseApp
    async fn import_state(&self, _app: &mut BaseApp, data: &[u8]) -> Result<()> {
        // Parse state export
        let state_export: serde_json::Value = serde_json::from_slice(data)
            .map_err(|e| SnapshotError::Serialization(e.to_string()))?;

        // Validate format version
        let version = state_export["version"]
            .as_u64()
            .ok_or_else(|| SnapshotError::InvalidFormat("Missing version".to_string()))?;

        if version != self.format_version as u64 {
            return Err(SnapshotError::InvalidFormat(format!(
                "Unsupported snapshot version: {version}, expected: {}",
                self.format_version
            )));
        }

        // Extract metadata
        let height = state_export["height"]
            .as_u64()
            .ok_or_else(|| SnapshotError::InvalidFormat("Missing height".to_string()))?;

        let app_hash_str = state_export["app_hash"]
            .as_str()
            .ok_or_else(|| SnapshotError::InvalidFormat("Missing app_hash".to_string()))?;

        let _app_hash = hex::decode(app_hash_str)
            .map_err(|_| SnapshotError::InvalidFormat("Invalid app_hash format".to_string()))?;

        // In a full implementation, we would:
        // 1. Clear existing state
        // 2. Import key-value pairs from the snapshot
        // 3. Update BaseApp's internal state (height, app_hash)

        // For now, we log what would be imported
        info!(
            "Would import state at height {} with app_hash {}",
            height, app_hash_str
        );

        // Note: BaseApp needs methods to:
        // - Set the current height
        // - Set the app hash
        // - Access the underlying storage for state import

        debug!("State import validated (full import pending storage integration)");
        Ok(())
    }

    /// Prune old snapshots keeping only the most recent ones
    async fn prune_old_snapshots(&self) -> Result<()> {
        let snapshots = self.snapshots.read().await;

        if snapshots.len() <= self.max_snapshots {
            return Ok(());
        }

        // Get heights sorted in descending order
        let mut heights: Vec<_> = snapshots.keys().copied().collect();
        heights.sort_by(|a, b| b.cmp(a));

        // Drop the read lock before pruning
        drop(snapshots);

        // Remove old snapshots
        for &height in heights.iter().skip(self.max_snapshots) {
            info!("Pruning old snapshot at height {}", height);

            // Remove from disk
            let snapshot_path = self.snapshot_dir.join(height.to_string());
            if let Err(e) = std::fs::remove_dir_all(&snapshot_path) {
                error!("Failed to remove snapshot directory: {}", e);
            }

            // Remove from cache
            let mut snapshots = self.snapshots.write().await;
            snapshots.remove(&height);
        }

        Ok(())
    }

    /// Delete a specific snapshot
    pub async fn delete_snapshot(&self, height: u64) -> Result<()> {
        // Remove from disk
        let snapshot_path = self.snapshot_dir.join(height.to_string());
        std::fs::remove_dir_all(&snapshot_path)?;

        // Remove from cache
        let mut snapshots = self.snapshots.write().await;
        snapshots.remove(&height);

        Ok(())
    }

    /// Get total size of all snapshots
    pub async fn total_size(&self) -> u64 {
        let snapshots = self.snapshots.read().await;
        snapshots.values().map(|s| s.size).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_snapshot_manager_creation() {
        let temp_dir = TempDir::new().unwrap();
        let manager = SnapshotManager::new(temp_dir.path().to_path_buf()).unwrap();

        assert_eq!(manager.format_version, 1);
        assert_eq!(manager.max_snapshots, 3);
        assert_eq!(manager.chunk_size, 16 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_chunk_splitting() {
        let temp_dir = TempDir::new().unwrap();
        let manager = SnapshotManager::new(temp_dir.path().to_path_buf()).unwrap();

        // Create test data larger than chunk size
        let data = vec![0u8; 20 * 1024 * 1024]; // 20MB
        let chunks = manager.split_into_chunks(&data);

        // Should be split into 2 chunks (16MB + 4MB)
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 16 * 1024 * 1024);
        assert_eq!(chunks[1].len(), 4 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_hash_calculation() {
        let temp_dir = TempDir::new().unwrap();
        let manager = SnapshotManager::new(temp_dir.path().to_path_buf()).unwrap();

        let data = b"test data";
        let hash1 = manager.calculate_hash(data);
        let hash2 = manager.calculate_hash(data);

        // Same data should produce same hash
        assert_eq!(hash1, hash2);

        // Different data should produce different hash
        let hash3 = manager.calculate_hash(b"different data");
        assert_ne!(hash1, hash3);
    }

    #[tokio::test]
    async fn test_snapshot_metadata_loading() {
        let temp_dir = TempDir::new().unwrap();
        let manager = SnapshotManager::new(temp_dir.path().to_path_buf()).unwrap();

        // Create metadata file
        let snapshot_dir = temp_dir.path().join("100");
        std::fs::create_dir_all(&snapshot_dir).unwrap();

        let metadata_json = serde_json::json!({
            "height": 100,
            "format": 1,
            "chunks": 5,
            "hash": "abcd1234",
            "metadata": "{\"chain_id\":\"test\"}",
            "created_at": 1234567890,
            "size": 1024000,
        });

        let metadata_path = snapshot_dir.join("metadata.json");
        std::fs::write(
            &metadata_path,
            serde_json::to_string_pretty(&metadata_json).unwrap(),
        )
        .unwrap();

        // Load metadata
        let (height, metadata) = manager.load_snapshot_metadata(&metadata_path).unwrap();

        assert_eq!(height, 100);
        assert_eq!(metadata.height, 100);
        assert_eq!(metadata.format, 1);
        assert_eq!(metadata.chunks, 5);
        assert_eq!(hex::encode(&metadata.hash), "abcd1234");
        assert_eq!(metadata.created_at, 1234567890);
        assert_eq!(metadata.size, 1024000);
    }

    #[tokio::test]
    async fn test_snapshot_metadata_validation() {
        let temp_dir = TempDir::new().unwrap();
        let manager = SnapshotManager::new(temp_dir.path().to_path_buf()).unwrap();

        // Create invalid metadata files
        let snapshot_dir = temp_dir.path().join("100");
        std::fs::create_dir_all(&snapshot_dir).unwrap();

        // Missing required fields
        let invalid_jsons = vec![
            serde_json::json!({}),                                        // Empty
            serde_json::json!({"format": 1}),                             // Missing height
            serde_json::json!({"height": 100}),                           // Missing format
            serde_json::json!({"height": 100, "format": 1}),              // Missing chunks
            serde_json::json!({"height": 100, "format": 1, "chunks": 5}), // Missing hash
            serde_json::json!({"height": 100, "format": 1, "chunks": 5, "hash": "invalid-hex"}), // Invalid hash
        ];

        for (i, invalid_json) in invalid_jsons.iter().enumerate() {
            let metadata_path = snapshot_dir.join(format!("metadata_{}.json", i));
            std::fs::write(&metadata_path, serde_json::to_string(invalid_json).unwrap()).unwrap();

            // Should fail to load
            assert!(manager.load_snapshot_metadata(&metadata_path).is_err());
        }
    }

    #[tokio::test]
    async fn test_snapshot_pruning() {
        let temp_dir = TempDir::new().unwrap();
        let mut manager = SnapshotManager::new(temp_dir.path().to_path_buf()).unwrap();
        manager.max_snapshots = 2; // Keep only 2 snapshots

        // Create mock snapshots
        for height in [100, 200, 300] {
            let snapshot_dir = temp_dir.path().join(height.to_string());
            std::fs::create_dir_all(&snapshot_dir).unwrap();

            // Create a chunk file
            let chunk_path = snapshot_dir.join("chunk_000000.dat");
            std::fs::write(&chunk_path, b"test data").unwrap();

            // Create metadata
            let metadata = SnapshotMetadata {
                height,
                format: 1,
                chunks: 1,
                hash: vec![0u8; 32],
                metadata: vec![],
                created_at: height,
                size: 9,
            };

            // Add to cache
            manager.snapshots.write().await.insert(height, metadata);
        }

        // Prune old snapshots
        manager.prune_old_snapshots().await.unwrap();

        // Check that only the two most recent snapshots remain
        let snapshots = manager.list_snapshots().await;
        assert_eq!(snapshots.len(), 2);
        assert_eq!(snapshots[0].height, 300); // Most recent
        assert_eq!(snapshots[1].height, 200);

        // Check that the oldest snapshot was deleted from disk
        assert!(!temp_dir.path().join("100").exists());
        assert!(temp_dir.path().join("200").exists());
        assert!(temp_dir.path().join("300").exists());
    }

    #[tokio::test]
    async fn test_snapshot_chunk_error_handling() {
        let temp_dir = TempDir::new().unwrap();
        let manager = SnapshotManager::new(temp_dir.path().to_path_buf()).unwrap();

        // Create a snapshot with metadata but missing chunk
        let height = 100;
        let snapshot_dir = temp_dir.path().join(height.to_string());
        std::fs::create_dir_all(&snapshot_dir).unwrap();

        let metadata = SnapshotMetadata {
            height,
            format: 1,
            chunks: 2,
            hash: vec![0u8; 32],
            metadata: vec![],
            created_at: 1234567890,
            size: 1000,
        };

        manager.snapshots.write().await.insert(height, metadata);

        // Try to load non-existent chunk
        let result = manager.load_chunk(height, 0).await;
        assert!(matches!(result, Err(SnapshotError::ChunkNotFound(0))));

        // Try to load out-of-range chunk
        let result = manager.load_chunk(height, 5).await;
        assert!(matches!(result, Err(SnapshotError::ChunkNotFound(5))));

        // Create a chunk file with permission error (simulate IO error)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let chunk_path = snapshot_dir.join("chunk_000001.dat");
            std::fs::write(&chunk_path, b"test").unwrap();
            let mut perms = std::fs::metadata(&chunk_path).unwrap().permissions();
            perms.set_mode(0o000); // No permissions
            std::fs::set_permissions(&chunk_path, perms).unwrap();

            // Should return IO error, not ChunkNotFound
            let result = manager.load_chunk(height, 1).await;
            assert!(matches!(result, Err(SnapshotError::Io(_))));

            // Cleanup permissions
            let mut perms = std::fs::metadata(&chunk_path).unwrap().permissions();
            perms.set_mode(0o644);
            std::fs::set_permissions(&chunk_path, perms).unwrap();
        }
    }
}
