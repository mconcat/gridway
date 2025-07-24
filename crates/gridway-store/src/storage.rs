//! Storage initialization and configuration module
//!
//! This module provides RocksDB-based storage initialization and management for Gridway.
//!
//! # Example
//!
//! ```rust,no_run
//! use gridway_store::{StorageConfig, init_storage};
//! use std::path::Path;
//!
//! // Create storage configuration
//! let mut config = StorageConfig::default();
//! config.cache_size = Some(1024 * 1024 * 1024); // 1GB cache
//! config.compression = Some("lz4".to_string());
//!
//! // Initialize storage
//! let home_dir = Path::new("/path/to/gridway/home");
//! let storage = init_storage(home_dir, &config).unwrap();
//!
//! // Access different databases
//! let app_store = &storage.app;
//! let block_store = &storage.blocks;
//! let state_store = &storage.state;
//! ```
//!
//! # Configuration Options
//!
//! The `StorageConfig` struct provides the following options:
//!
//! - `cache_size`: LRU block cache size in bytes (default: 512MB)
//! - `write_buffer_size`: Write buffer size in bytes (default: 64MB)
//! - `max_open_files`: Maximum number of open files (default: 5000)
//! - `block_size`: Block size in bytes (default: 4KB)
//! - `compression`: Compression type - "lz4", "snappy", "zstd", or "none" (default: "lz4")
//! - `compaction_style`: Compaction style - "level", "universal", or "fifo" (default: "level")
//!
//! # Directory Structure
//!
//! The storage system creates the following directory structure:
//!
//! ```text
//! data/
//!   application.db/    # Main application state
//!   blockstore.db/     # Block storage
//!   state.db/          # Consensus state
//!   tx_index.db/       # Transaction indexing (optional)
//! ```
//!
//! # Migration Support
//!
//! The module includes a migration system for upgrading storage schemas:
//!
//! ```rust,no_run
//! use gridway_store::{StorageMigration, StoreError, KVStore, run_migrations};
//!
//! struct MyMigration;
//!
//! impl StorageMigration for MyMigration {
//!     fn version(&self) -> u32 { 1 }
//!     
//!     fn migrate(&self, store: &mut dyn KVStore) -> Result<(), StoreError> {
//!         // Perform migration logic
//!         store.set(b"migrated", b"true")?;
//!         Ok(())
//!     }
//!     
//!     fn description(&self) -> &str {
//!         "Initial migration"
//!     }
//! }
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # use gridway_store::{StorageConfig, init_storage};
//! # use std::path::Path;
//! # let home_dir = Path::new("/tmp");
//! # let config = StorageConfig::default();
//! // Run migrations
//! let mut storage = init_storage(home_dir, &config)?;
//! let migrations: Vec<Box<dyn StorageMigration>> = vec![
//!     Box::new(MyMigration),
//! ];
//! run_migrations(&mut storage, migrations)?;
//! # Ok(())
//! # }
//! ```

use crate::{KVStore, StoreError};
use rocksdb::{BlockBasedOptions, Cache, DBCompressionType, Options as RocksDBOptions, DB};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

/// Storage configuration options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Size of the LRU block cache in bytes
    pub cache_size: Option<usize>,
    /// Size of the write buffer in bytes
    pub write_buffer_size: Option<usize>,
    /// Maximum number of open files
    pub max_open_files: Option<i32>,
    /// Size of blocks in bytes
    pub block_size: Option<usize>,
    /// Compression type: "lz4", "snappy", "zstd", "none"
    pub compression: Option<String>,
    /// Compaction style: "level", "universal", "fifo"
    pub compaction_style: Option<String>,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            cache_size: Some(512 * 1024 * 1024),       // 512MB
            write_buffer_size: Some(64 * 1024 * 1024), // 64MB
            max_open_files: Some(5000),
            block_size: Some(4 * 1024), // 4KB
            compression: Some("lz4".to_string()),
            compaction_style: Some("level".to_string()),
        }
    }
}

/// RocksDB-backed key-value store
pub struct RocksDBStore {
    db: Arc<DB>,
}

impl RocksDBStore {
    /// Create a new RocksDB store with the given database
    pub fn new(db: DB) -> Self {
        Self { db: Arc::new(db) }
    }

    /// Get the underlying database reference
    pub fn db(&self) -> &Arc<DB> {
        &self.db
    }
}

impl KVStore for RocksDBStore {
    fn get(&self, key: &[u8]) -> Result<Option<Vec<u8>>, StoreError> {
        self.db
            .get(key)
            .map_err(|e| StoreError::Backend(format!("RocksDB get error:: {e}")))
    }

    fn set(&mut self, key: &[u8], value: &[u8]) -> Result<(), StoreError> {
        self.db
            .put(key, value)
            .map_err(|e| StoreError::Backend(format!("RocksDB put error:: {e}")))
    }

    fn delete(&mut self, key: &[u8]) -> Result<(), StoreError> {
        self.db
            .delete(key)
            .map_err(|e| StoreError::Backend(format!("RocksDB delete error:: {e}")))
    }

    fn has(&self, key: &[u8]) -> Result<bool, StoreError> {
        self.db
            .get(key)
            .map(|v| v.is_some())
            .map_err(|e| StoreError::Backend(format!("RocksDB get error:: {e}")))
    }

    fn prefix_iterator(&self, prefix: &[u8]) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
        let prefix = prefix.to_vec();
        let iter = self.db.prefix_iterator(&prefix);
        Box::new(iter.filter_map(move |result| {
            result.ok().and_then(|(key, value)| {
                if key.starts_with(&prefix) {
                    Some((key.to_vec(), value.to_vec()))
                } else {
                    None
                }
            })
        }))
    }
}

/// Storage manager that handles multiple database instances
pub struct Storage {
    /// Application state database
    pub app: Arc<RocksDBStore>,
    /// Block storage database
    pub blocks: Arc<RocksDBStore>,
    /// Consensus state database
    pub state: Arc<RocksDBStore>,
    /// Transaction index database (optional)
    pub tx_index: Option<Arc<RocksDBStore>>,
}

impl Storage {
    /// Get version key for a database
    const VERSION_KEY: &'static [u8] = b"__db_version__";

    /// Get the current version of a store
    pub fn get_version(&self) -> Result<u32, StoreError> {
        match self.app.get(Self::VERSION_KEY)? {
            Some(data) => {
                let bytes: [u8; 4] = data
                    .as_slice()
                    .try_into()
                    .map_err(|_| StoreError::InvalidData("Invalid version data".into()))?;
                Ok(u32::from_be_bytes(bytes))
            }
            None => Ok(0),
        }
    }

    /// Set the version of a store
    pub fn set_version(&mut self, version: u32) -> Result<(), StoreError> {
        let app_store = Arc::get_mut(&mut self.app).ok_or_else(|| {
            StoreError::Backend("Cannot get mutable reference to app store".into())
        })?;
        app_store.set(Self::VERSION_KEY, &version.to_be_bytes())
    }
}

/// Configure RocksDB options based on storage config
fn configure_db_options(config: &StorageConfig) -> Result<RocksDBOptions, StoreError> {
    let mut opts = RocksDBOptions::default();
    opts.create_if_missing(true);
    opts.create_missing_column_families(true);

    // Set compression type
    if let Some(compression) = &config.compression {
        let compression_type = match compression.as_str() {
            "lz4" => DBCompressionType::Lz4,
            "snappy" => DBCompressionType::Snappy,
            "zstd" => DBCompressionType::Zstd,
            "none" => DBCompressionType::None,
            _ => {
                return Err(StoreError::InvalidConfig(format!(
                    "Unknown compression type:: {compression}"
                )))
            }
        };
        opts.set_compression_type(compression_type);
    }

    // Set cache size using block-based options
    let mut block_opts = BlockBasedOptions::default();
    if let Some(cache_size) = config.cache_size {
        let cache = Cache::new_lru_cache(cache_size);
        block_opts.set_block_cache(&cache);
    }

    // Set write buffer size
    if let Some(write_buffer_size) = config.write_buffer_size {
        opts.set_write_buffer_size(write_buffer_size);
    }

    // Set max open files
    if let Some(max_open_files) = config.max_open_files {
        opts.set_max_open_files(max_open_files);
    }

    // Set block size
    if let Some(block_size) = config.block_size {
        block_opts.set_block_size(block_size);
    }

    // Apply block-based options to the main options
    opts.set_block_based_table_factory(&block_opts);

    // Set compaction style
    if let Some(compaction_style) = &config.compaction_style {
        match compaction_style.as_str() {
            "level" => opts.set_level_compaction_dynamic_level_bytes(true),
            "universal" => opts.set_universal_compaction_options(&Default::default()),
            "fifo" => opts.set_fifo_compaction_options(&Default::default()),
            _ => {
                return Err(StoreError::InvalidConfig(format!(
                    "Unknown compaction style:: {compaction_style}"
                )))
            }
        }
    }

    // Additional optimizations
    opts.set_bytes_per_sync(1024 * 1024); // 1MB
    opts.optimize_for_point_lookup(10); // 10MB block cache
    opts.set_max_background_jobs(4);

    Ok(opts)
}

/// Initialize storage with the given configuration
pub fn init_storage(home_dir: &Path, config: &StorageConfig) -> Result<Storage, StoreError> {
    // Create data directory
    let data_dir = home_dir.join("data");
    std::fs::create_dir_all(&data_dir)
        .map_err(|e| StoreError::Backend(format!("Failed to create data directory:: {e}")))?;

    // Configure RocksDB options
    let db_opts = configure_db_options(config)?;

    // Open application database
    let app_path = data_dir.join("application.db");
    let app_db = DB::open(&db_opts, &app_path)
        .map_err(|e| StoreError::Backend(format!("Failed to open application.db:: {e}")))?;
    let app_store = Arc::new(RocksDBStore::new(app_db));

    // Open blockstore database
    let block_path = data_dir.join("blockstore.db");
    let block_db = DB::open(&db_opts, &block_path)
        .map_err(|e| StoreError::Backend(format!("Failed to open blockstore.db:: {e}")))?;
    let block_store = Arc::new(RocksDBStore::new(block_db));

    // Open state database
    let state_path = data_dir.join("state.db");
    let state_db = DB::open(&db_opts, &state_path)
        .map_err(|e| StoreError::Backend(format!("Failed to open state.db:: {e}")))?;
    let state_store = Arc::new(RocksDBStore::new(state_db));

    // Open transaction index database (optional)
    let tx_index_path = data_dir.join("tx_index.db");
    let tx_index_store = if tx_index_path.exists() || config.cache_size.is_some() {
        let tx_index_db = DB::open(&db_opts, &tx_index_path)
            .map_err(|e| StoreError::Backend(format!("Failed to open tx_index.db:: {e}")))?;
        Some(Arc::new(RocksDBStore::new(tx_index_db)))
    } else {
        None
    };

    Ok(Storage {
        app: app_store,
        blocks: block_store,
        state: state_store,
        tx_index: tx_index_store,
    })
}

/// Migration trait for storage upgrades
pub trait StorageMigration: Send + Sync {
    /// Get the version this migration upgrades to
    fn version(&self) -> u32;

    /// Run the migration
    fn migrate(&self, store: &mut dyn KVStore) -> Result<(), StoreError>;

    /// Description of what this migration does
    fn description(&self) -> &str;
}

/// Run migrations on storage
pub fn run_migrations(
    storage: &mut Storage,
    migrations: Vec<Box<dyn StorageMigration>>,
) -> Result<(), StoreError> {
    let current_version = storage.get_version()?;

    for migration in migrations {
        if migration.version() > current_version {
            println!(
                "Running migration v{}: {}",
                migration.version(),
                migration.description()
            );

            // Get mutable reference to app store for migration
            let app_store = Arc::get_mut(&mut storage.app).ok_or_else(|| {
                StoreError::Backend("Cannot get mutable reference for migration".into())
            })?;

            migration.migrate(app_store)?;
            storage.set_version(migration.version())?;

            println!("Migration v{} completed successfully", migration.version());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_storage_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default();

        let _storage = init_storage(temp_dir.path(), &config).unwrap();

        // Verify data directory structure
        assert!(temp_dir.path().join("data").exists());
        assert!(temp_dir.path().join("data/application.db").exists());
        assert!(temp_dir.path().join("data/blockstore.db").exists());
        assert!(temp_dir.path().join("data/state.db").exists());

        // Test basic operations - can't open same DB twice, so use storage directly
        // Storage is already opened, test is complete
    }

    #[test]
    fn test_storage_config_validation() {
        // Test valid compression types
        let mut config = StorageConfig {
            compression: Some("lz4".to_string()),
            ..Default::default()
        };
        assert!(configure_db_options(&config).is_ok());

        config.compression = Some("invalid".to_string());
        assert!(configure_db_options(&config).is_err());

        // Test valid compaction styles
        config.compression = Some("lz4".to_string());
        config.compaction_style = Some("level".to_string());
        assert!(configure_db_options(&config).is_ok());

        config.compaction_style = Some("invalid".to_string());
        assert!(configure_db_options(&config).is_err());
    }

    struct TestMigration {
        version: u32,
    }

    impl StorageMigration for TestMigration {
        fn version(&self) -> u32 {
            self.version
        }

        fn migrate(&self, store: &mut dyn KVStore) -> Result<(), StoreError> {
            store.set(b"migrated", &self.version.to_be_bytes())
        }

        fn description(&self) -> &str {
            "Test migration"
        }
    }

    #[test]
    fn test_migrations() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default();
        let mut storage = init_storage(temp_dir.path(), &config).unwrap();

        // Run migrations
        let migrations: Vec<Box<dyn StorageMigration>> = vec![
            Box::new(TestMigration { version: 1 }),
            Box::new(TestMigration { version: 2 }),
        ];

        run_migrations(&mut storage, migrations).unwrap();

        // Verify version was updated
        assert_eq!(storage.get_version().unwrap(), 2);

        // Verify migration was run
        let migrated = storage.app.get(b"migrated").unwrap().unwrap();
        assert_eq!(u32::from_be_bytes(migrated.try_into().unwrap()), 2);
    }
}
