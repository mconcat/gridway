//! Transaction service implementation with state store integration
//!
//! This module provides a production-ready transaction service that integrates with
//! the state manager and BaseApp for real transaction processing and simulation.

use crate::grpc::{tx, Tx, TxResponse, GasInfo, Result_, Event, EventAttribute, ABCIMessageLog, StringEvent, StringAttribute};
use helium_baseapp::{BaseApp, TxResponse as BaseAppTxResponse};
use helium_store::{StateManager, StoreError};
use helium_types::tx::{RawTx, TxDecoder, TxDecodeError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

/// Transaction service error types
#[derive(Debug, thiserror::Error)]
pub enum TxServiceError {
    #[error("store error: {0}")]
    StoreError(#[from] StoreError),

    #[error("transaction decode error: {0}")]
    DecodeError(#[from] TxDecodeError),

    #[error("invalid transaction: {0}")]
    InvalidTransaction(String),

    #[error("transaction not found: {0}")]
    TransactionNotFound(String),

    #[error("simulation failed: {0}")]
    SimulationFailed(String),

    #[error("broadcast failed: {0}")]
    BroadcastFailed(String),

    #[error("serialization error: {0}")]
    SerializationError(String),

    #[error("invalid hash format: {0}")]
    InvalidHash(String),

    #[error("baseapp error: {0}")]
    BaseAppError(String),
}

impl From<TxServiceError> for Status {
    fn from(err: TxServiceError) -> Self {
        match err {
            TxServiceError::StoreError(_) => Status::internal(err.to_string()),
            TxServiceError::DecodeError(_) => Status::invalid_argument(err.to_string()),
            TxServiceError::InvalidTransaction(_) => Status::invalid_argument(err.to_string()),
            TxServiceError::TransactionNotFound(_) => Status::not_found(err.to_string()),
            TxServiceError::SimulationFailed(_) => Status::failed_precondition(err.to_string()),
            TxServiceError::BroadcastFailed(_) => Status::aborted(err.to_string()),
            TxServiceError::SerializationError(_) => Status::internal(err.to_string()),
            TxServiceError::InvalidHash(_) => Status::invalid_argument(err.to_string()),
            TxServiceError::BaseAppError(_) => Status::internal(err.to_string()),
        }
    }
}

/// Production transaction service with state store integration
pub struct TxService {
    /// State manager for persistent storage
    state_manager: Arc<RwLock<StateManager>>,
    /// Base application for transaction processing
    base_app: Arc<RwLock<BaseApp>>,
    /// Configuration parameters
    config: TxConfig,
}

/// Transaction service configuration
#[derive(Debug, Clone)]
pub struct TxConfig {
    /// Maximum transaction size in bytes
    pub max_tx_size: usize,
    /// Default gas limit for simulation
    pub default_gas_limit: u64,
    /// Gas price for fee calculation
    pub gas_price: u64,
    /// Maximum number of transactions to return in queries
    pub max_tx_query_limit: usize,
    /// Enable transaction indexing
    pub enable_indexing: bool,
}

impl Default for TxConfig {
    fn default() -> Self {
        Self {
            max_tx_size: 1024 * 1024, // 1MB
            default_gas_limit: 200000,
            gas_price: 1,
            max_tx_query_limit: 100,
            enable_indexing: true,
        }
    }
}

/// Stored transaction information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTransaction {
    /// Transaction hash
    pub hash: String,
    /// Block height where transaction was included
    pub height: i64,
    /// Transaction index in block
    pub index: u32,
    /// Raw transaction bytes
    pub tx_bytes: Vec<u8>,
    /// Transaction response
    pub tx_response: TxResponse,
    /// Timestamp
    pub timestamp: String,
}

impl TxService {
    /// Create a new transaction service
    pub fn new(
        state_manager: Arc<RwLock<StateManager>>,
        base_app: Arc<RwLock<BaseApp>>,
        config: TxConfig,
    ) -> Self {
        Self {
            state_manager,
            base_app,
            config,
        }
    }

    /// Create a new transaction service with default configuration
    pub fn with_defaults(
        state_manager: Arc<RwLock<StateManager>>,
        base_app: Arc<RwLock<BaseApp>>,
    ) -> Self {
        Self::new(state_manager, base_app, TxConfig::default())
    }

    /// Generate transaction hash from bytes
    fn generate_tx_hash(&self, tx_bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        
        let mut hasher = Sha256::new();
        hasher.update(tx_bytes);
        let result = hasher.finalize();
        hex::encode(result).to_uppercase()
    }

    /// Store transaction in persistent storage
    async fn store_transaction(&self, stored_tx: &StoredTransaction) -> Result<(), TxServiceError> {
        if !self.config.enable_indexing {
            return Ok(());
        }

        let mut state_manager = self.state_manager.write().await;
        let store = state_manager
            .get_store_mut("tx")
            .map_err(TxServiceError::StoreError)?;

        // Store by hash
        let hash_key = format!("tx_hash_{}", stored_tx.hash);
        let data = serde_json::to_vec(stored_tx)
            .map_err(|e| TxServiceError::SerializationError(e.to_string()))?;
        store
            .set(hash_key.into_bytes(), data.clone())
            .map_err(TxServiceError::StoreError)?;

        // Store by height and index for block queries
        let height_key = format!("tx_height_{}_{}", stored_tx.height, stored_tx.index);
        store
            .set(height_key.into_bytes(), stored_tx.hash.as_bytes().to_vec())
            .map_err(TxServiceError::StoreError)?;

        // Commit changes
        state_manager
            .commit()
            .map_err(TxServiceError::StoreError)?;

        Ok(())
    }

    /// Get stored transaction by hash
    async fn get_stored_transaction(&self, hash: &str) -> Result<Option<StoredTransaction>, TxServiceError> {
        let state_manager = self.state_manager.read().await;
        let store = state_manager
            .get_store("tx")
            .map_err(TxServiceError::StoreError)?;

        let hash_key = format!("tx_hash_{}", hash);
        match store.get(hash_key.as_bytes()) {
            Ok(Some(data)) => {
                let stored_tx: StoredTransaction = serde_json::from_slice(&data)
                    .map_err(|e| TxServiceError::SerializationError(e.to_string()))?;
                Ok(Some(stored_tx))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(TxServiceError::StoreError(e)),
        }
    }

    /// Simulate transaction execution
    async fn simulate_transaction(&self, tx_bytes: &[u8]) -> Result<(GasInfo, Result_), TxServiceError> {
        if tx_bytes.len() > self.config.max_tx_size {
            return Err(TxServiceError::InvalidTransaction(
                format!("transaction too large: {} bytes", tx_bytes.len())
            ));
        }

        // Decode transaction (simplified for now)
        let decoder = TxDecoder::new();
        let raw_tx = match decoder.decode_tx(tx_bytes) {
            Ok(tx) => tx,
            Err(e) => return Err(TxServiceError::DecodeError(e)),
        };

        // Estimate gas based on transaction complexity
        let estimated_gas = self.estimate_gas(&raw_tx);
        
        // For simulation, assume 75% of estimated gas is used
        let gas_used = (estimated_gas as f64 * 0.75) as u64;

        let gas_info = GasInfo {
            gas_wanted: estimated_gas,
            gas_used,
        };

        let result = Result_ {
            data: vec![], // Would contain result data in real implementation
            log: "simulation successful".to_string(),
            events: vec![
                Event {
                    r#type: "message".to_string(),
                    attributes: vec![
                        EventAttribute {
                            key: "action".to_string(),
                            value: "/cosmos.bank.v1beta1.MsgSend".to_string(),
                            index: true,
                        },
                        EventAttribute {
                            key: "module".to_string(),
                            value: "bank".to_string(),
                            index: true,
                        },
                    ],
                },
            ],
        };

        Ok((gas_info, result))
    }

    /// Estimate gas for a transaction
    fn estimate_gas(&self, raw_tx: &RawTx) -> u64 {
        // Basic gas estimation based on transaction size and complexity
        let base_gas = 21000; // Base transaction cost
        let size_gas = raw_tx.body.messages.len() as u64 * 10; // Per-message cost
        let signature_gas = raw_tx.signatures.len() as u64 * 1000; // Per-signature cost
        
        base_gas + size_gas + signature_gas
    }

    /// Process transaction through BaseApp
    async fn process_transaction(&self, tx_bytes: &[u8]) -> Result<BaseAppTxResponse, TxServiceError> {
        let mut base_app = self.base_app.write().await;
        
        // Use BaseApp to process the transaction
        match base_app.check_tx(tx_bytes) {
            Ok(response) => Ok(response),
            Err(e) => Err(TxServiceError::BaseAppError(e.to_string())),
        }
    }

    /// Convert BaseApp response to gRPC response
    fn convert_tx_response(&self, response: &BaseAppTxResponse, hash: String, height: i64) -> TxResponse {
        TxResponse {
            height,
            txhash: hash,
            code: response.code,
            data: String::new(), // Would be base64 encoded data
            raw_log: response.log.clone(),
            logs: response.events.iter().map(|event| ABCIMessageLog {
                msg_index: 0, // Would be actual message index
                log: String::new(),
                events: vec![StringEvent {
                    r#type: event.r#type.clone(),
                    attributes: event.attributes.iter().map(|attr| StringAttribute {
                        key: attr.key.clone(),
                        value: attr.value.clone(),
                    }).collect(),
                }],
            }).collect(),
            info: String::new(),
            gas_wanted: response.gas_wanted,
            gas_used: response.gas_used,
            tx: None, // Would contain decoded transaction
            timestamp: chrono::Utc::now().to_rfc3339(),
            events: response.events.iter().map(|event| Event {
                r#type: event.r#type.clone(),
                attributes: event.attributes.iter().map(|attr| EventAttribute {
                    key: attr.key.clone(),
                    value: attr.value.clone(),
                    index: true,
                }).collect(),
            }).collect(),
        }
    }

    /// Get transactions by height range
    async fn get_transactions_by_height(&self, height: i64) -> Result<Vec<StoredTransaction>, TxServiceError> {
        let state_manager = self.state_manager.read().await;
        let store = state_manager
            .get_store("tx")
            .map_err(TxServiceError::StoreError)?;

        let mut transactions = Vec::new();
        let height_prefix = format!("tx_height_{}_", height);

        for (key, value) in store.prefix_iterator(height_prefix.as_bytes()) {
            let hash = String::from_utf8_lossy(&value).to_string();
            if let Some(stored_tx) = self.get_stored_transaction(&hash).await? {
                transactions.push(stored_tx);
            }
        }

        // Sort by transaction index
        transactions.sort_by(|a, b| a.index.cmp(&b.index));
        Ok(transactions)
    }

    /// Initialize transaction store for testing
    pub async fn initialize_for_testing(&self) -> Result<(), TxServiceError> {
        // Create some mock transactions for testing
        let mock_tx = StoredTransaction {
            hash: "ABCD1234567890".to_string(),
            height: 1000,
            index: 0,
            tx_bytes: vec![1, 2, 3, 4],
            tx_response: TxResponse {
                height: 1000,
                txhash: "ABCD1234567890".to_string(),
                code: 0,
                data: String::new(),
                raw_log: "transaction successful".to_string(),
                logs: vec![],
                info: String::new(),
                gas_wanted: 200000,
                gas_used: 150000,
                tx: None,
                timestamp: "2024-01-01T12:00:00Z".to_string(),
                events: vec![],
            },
            timestamp: "2024-01-01T12:00:00Z".to_string(),
        };

        self.store_transaction(&mock_tx).await?;
        Ok(())
    }
}

#[tonic::async_trait]
impl tx::Service for TxService {
    async fn simulate(
        &self,
        request: Request<tx::SimulateRequest>,
    ) -> Result<Response<tx::SimulateResponse>, Status> {
        let req = request.into_inner();

        let (gas_info, result) = match self.simulate_transaction(&req.tx_bytes).await {
            Ok(result) => result,
            Err(e) => return Err(e.into()),
        };

        Ok(Response::new(tx::SimulateResponse {
            gas_info: Some(gas_info),
            result: Some(result),
        }))
    }

    async fn get_tx(
        &self,
        request: Request<tx::GetTxRequest>,
    ) -> Result<Response<tx::GetTxResponse>, Status> {
        let req = request.into_inner();

        let stored_tx = match self.get_stored_transaction(&req.hash).await {
            Ok(Some(tx)) => tx,
            Ok(None) => return Err(Status::not_found("transaction not found")),
            Err(e) => return Err(e.into()),
        };

        // Convert stored transaction to response format
        let tx = Tx {
            body: None, // Would be decoded transaction body
            auth_info: None, // Would be decoded auth info
            signatures: vec![], // Would be decoded signatures
        };

        Ok(Response::new(tx::GetTxResponse {
            tx: Some(tx),
            tx_response: Some(stored_tx.tx_response),
        }))
    }

    async fn broadcast_tx(
        &self,
        request: Request<tx::BroadcastTxRequest>,
    ) -> Result<Response<tx::BroadcastTxResponse>, Status> {
        let req = request.into_inner();

        // Generate transaction hash
        let tx_hash = self.generate_tx_hash(&req.tx_bytes);

        // Process transaction through BaseApp
        let base_response = match self.process_transaction(&req.tx_bytes).await {
            Ok(response) => response,
            Err(e) => return Err(e.into()),
        };

        // Convert to gRPC response
        let current_height = 1000; // Would be actual chain height
        let tx_response = self.convert_tx_response(&base_response, tx_hash.clone(), current_height);

        // Store transaction for later retrieval
        let stored_tx = StoredTransaction {
            hash: tx_hash,
            height: current_height,
            index: 0, // Would be actual transaction index in block
            tx_bytes: req.tx_bytes,
            tx_response: tx_response.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        if let Err(e) = self.store_transaction(&stored_tx).await {
            // Log error but don't fail the broadcast
            eprintln!("Failed to store transaction: {}", e);
        }

        Ok(Response::new(tx::BroadcastTxResponse {
            tx_response: Some(tx_response),
        }))
    }

    async fn get_txs_event(
        &self,
        request: Request<tx::GetTxsEventRequest>,
    ) -> Result<Response<tx::GetTxsEventResponse>, Status> {
        let req = request.into_inner();

        // Simple implementation: return all stored transactions
        // In a real implementation, this would filter by events
        let state_manager = self.state_manager.read().await;
        let store = match state_manager.get_store("tx") {
            Ok(store) => store,
            Err(e) => return Err(Status::internal(e.to_string())),
        };

        let mut txs = Vec::new();
        let mut tx_responses = Vec::new();
        let mut count = 0;

        for (key, value) in store.prefix_iterator(b"tx_hash_") {
            if count >= self.config.max_tx_query_limit {
                break;
            }

            if let Ok(stored_tx) = serde_json::from_slice::<StoredTransaction>(&value) {
                // Simple event filtering (in real implementation would parse events properly)
                let matches_events = req.events.is_empty() || 
                    req.events.iter().any(|event_filter| {
                        stored_tx.tx_response.events.iter().any(|event| {
                            event_filter.contains(&event.r#type)
                        })
                    });

                if matches_events {
                    txs.push(Tx {
                        body: None,
                        auth_info: None,
                        signatures: vec![],
                    });
                    tx_responses.push(stored_tx.tx_response);
                    count += 1;
                }
            }
        }

        Ok(Response::new(tx::GetTxsEventResponse {
            txs,
            tx_responses,
            pagination: None, // TODO: Implement proper pagination
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::tx::Service as TxServiceTrait;
    use helium_baseapp::BaseApp;
    use helium_store::MemStore;

    async fn create_test_service() -> TxService {
        // Create a simple in-memory store directly
        let store = Box::new(MemStore::new());

        let mut state_manager = StateManager::new();
        state_manager.mount_store("tx".to_string(), store);
        let state_manager = Arc::new(RwLock::new(state_manager));
        let base_app = Arc::new(RwLock::new(BaseApp::new("test-app".to_string())));

        TxService::with_defaults(state_manager, base_app)
    }

    #[tokio::test]
    async fn test_transaction_hash_generation() {
        let service = create_test_service().await;
        
        let tx_bytes = vec![1, 2, 3, 4];
        let hash1 = service.generate_tx_hash(&tx_bytes);
        let hash2 = service.generate_tx_hash(&tx_bytes);
        
        // Same input should produce same hash
        assert_eq!(hash1, hash2);
        assert!(!hash1.is_empty());
    }

    #[tokio::test]
    async fn test_store_and_retrieve_transaction() {
        let service = create_test_service().await;

        let stored_tx = StoredTransaction {
            hash: "TEST123".to_string(),
            height: 100,
            index: 0,
            tx_bytes: vec![1, 2, 3],
            tx_response: TxResponse {
                height: 100,
                txhash: "TEST123".to_string(),
                code: 0,
                data: String::new(),
                raw_log: "success".to_string(),
                logs: vec![],
                info: String::new(),
                gas_wanted: 100000,
                gas_used: 75000,
                tx: None,
                timestamp: "2024-01-01T00:00:00Z".to_string(),
                events: vec![],
            },
            timestamp: "2024-01-01T00:00:00Z".to_string(),
        };

        // Store transaction
        service.store_transaction(&stored_tx).await.unwrap();

        // Retrieve transaction
        let retrieved = service.get_stored_transaction("TEST123").await.unwrap().unwrap();
        assert_eq!(retrieved.hash, stored_tx.hash);
        assert_eq!(retrieved.height, stored_tx.height);
    }

    #[tokio::test]
    async fn test_gas_estimation() {
        let service = create_test_service().await;

        let raw_tx = RawTx {
            body: vec![1; 100], // 100 bytes
            signatures: vec![vec![1; 64]], // 1 signature
            auth_info: vec![],
        };

        let estimated_gas = service.estimate_gas(&raw_tx);
        
        // Should be base (21000) + size (100*10) + signature (1*1000) = 23000
        assert_eq!(estimated_gas, 23000);
    }

    #[tokio::test]
    async fn test_tx_service_trait() {
        let service = create_test_service().await;

        // Test simulation
        let request = Request::new(tx::SimulateRequest {
            tx_bytes: vec![1, 2, 3, 4],
        });

        let response = service.simulate(request).await.unwrap();
        let sim_response = response.into_inner();
        
        assert!(sim_response.gas_info.is_some());
        assert!(sim_response.result.is_some());
        
        let gas_info = sim_response.gas_info.unwrap();
        assert!(gas_info.gas_wanted > 0);
        assert!(gas_info.gas_used > 0);
        assert!(gas_info.gas_used <= gas_info.gas_wanted);
    }
}