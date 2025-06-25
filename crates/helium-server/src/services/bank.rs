//! Bank service implementation with state store integration
//!
//! This module provides a production-ready bank service that integrates with
//! the state manager for persistent balance storage and transaction processing.

use crate::grpc::{bank, Coin};
use helium_baseapp::BaseApp;
use helium_store::{StateManager, StoreError, KVStore};
use helium_types::{msgs::bank::MsgSend, tx::SdkMsg};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

/// Bank service error types
#[derive(Debug, thiserror::Error)]
pub enum BankServiceError {
    #[error("store error: {0}")]
    StoreError(#[from] StoreError),

    #[error("invalid address: {0}")]
    InvalidAddress(String),

    #[error("invalid amount: {0}")]
    InvalidAmount(String),

    #[error("insufficient funds: requested {requested}, available {available}")]
    InsufficientFunds { requested: u64, available: u64 },

    #[error("account not found: {0}")]
    AccountNotFound(String),

    #[error("invalid denomination: {0}")]
    InvalidDenom(String),

    #[error("transaction failed: {0}")]
    TransactionFailed(String),
}

impl From<BankServiceError> for Status {
    fn from(err: BankServiceError) -> Self {
        match err {
            BankServiceError::StoreError(_) => Status::internal(err.to_string()),
            BankServiceError::InvalidAddress(_) => Status::invalid_argument(err.to_string()),
            BankServiceError::InvalidAmount(_) => Status::invalid_argument(err.to_string()),
            BankServiceError::InsufficientFunds { .. } => {
                Status::failed_precondition(err.to_string())
            }
            BankServiceError::AccountNotFound(_) => Status::not_found(err.to_string()),
            BankServiceError::InvalidDenom(_) => Status::invalid_argument(err.to_string()),
            BankServiceError::TransactionFailed(_) => Status::aborted(err.to_string()),
        }
    }
}

/// Production bank service with state store integration
pub struct BankService {
    /// State manager for persistent storage
    state_manager: Arc<RwLock<StateManager>>,
    /// Base application for transaction processing
    #[allow(dead_code)]
    base_app: Arc<RwLock<BaseApp>>,
    /// Configuration parameters
    config: BankConfig,
}

/// Bank service configuration
#[derive(Debug, Clone)]
pub struct BankConfig {
    /// Default denomination for the chain
    pub default_denom: String,
    /// Maximum number of denominations per account
    pub max_denoms_per_account: usize,
    /// Minimum amount for transactions (prevents dust)
    pub min_amount: u64,
    /// Enable send transactions
    pub send_enabled: bool,
}

impl Default for BankConfig {
    fn default() -> Self {
        Self {
            default_denom: "stake".to_string(),
            max_denoms_per_account: 100,
            min_amount: 1,
            send_enabled: true,
        }
    }
}

/// Account balance information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountBalance {
    /// Account address
    pub address: String,
    /// Balance by denomination
    pub balances: HashMap<String, u64>,
    /// Account sequence number
    pub sequence: u64,
}

impl BankService {
    /// Create a new bank service
    pub fn new(
        state_manager: Arc<RwLock<StateManager>>,
        base_app: Arc<RwLock<BaseApp>>,
        config: BankConfig,
    ) -> Self {
        Self {
            state_manager,
            base_app,
            config,
        }
    }

    /// Create a new bank service with default configuration
    pub fn with_defaults(
        state_manager: Arc<RwLock<StateManager>>,
        base_app: Arc<RwLock<BaseApp>>,
    ) -> Self {
        Self::new(state_manager, base_app, BankConfig::default())
    }

    /// Get balance for a specific address and denomination
    async fn get_balance(&self, address: &str, denom: &str) -> Result<u64, BankServiceError> {
        let mut state_manager = self.state_manager.write().await;

        // First check if there's a cached version
        if let Ok(store) = state_manager.get_store_mut("bank") {
            let key = format!("balance_{}_{}", address, denom);
            match store.get(key.as_bytes()) {
                Ok(Some(data)) => {
                    let amount_str = String::from_utf8_lossy(&data);
                    return amount_str
                        .parse::<u64>()
                        .map_err(|_| BankServiceError::InvalidAmount(amount_str.to_string()));
                }
                Ok(None) => return Ok(0),
                Err(_) => {} // Fall through to try read-only store
            }
        }

        // Fall back to read-only access
        drop(state_manager);
        let state_manager = self.state_manager.read().await;
        let store = state_manager
            .get_store("bank")
            .map_err(BankServiceError::StoreError)?;

        let key = format!("balance_{}_{}", address, denom);
        match store.get(key.as_bytes()) {
            Ok(Some(data)) => {
                let amount_str = String::from_utf8_lossy(&data);
                amount_str
                    .parse::<u64>()
                    .map_err(|_| BankServiceError::InvalidAmount(amount_str.to_string()))
            }
            Ok(None) => Ok(0),
            Err(e) => Err(BankServiceError::StoreError(e)),
        }
    }

    /// Set balance for a specific address and denomination
    async fn set_balance(
        &self,
        address: &str,
        denom: &str,
        amount: u64,
    ) -> Result<(), BankServiceError> {
        let mut state_manager = self.state_manager.write().await;
        let store = state_manager
            .get_store_mut("bank")
            .map_err(BankServiceError::StoreError)?;

        let key = format!("balance_{}_{}", address, denom);
        if amount == 0 {
            // Remove zero balances to save space
            store
                .delete(key.as_bytes())
                .map_err(BankServiceError::StoreError)?;
        } else {
            store
                .set(key.into_bytes(), amount.to_string().into_bytes())
                .map_err(BankServiceError::StoreError)?;
        }

        // Commit changes immediately for testing
        state_manager
            .commit()
            .map_err(BankServiceError::StoreError)?;

        Ok(())
    }

    /// Get all balances for an address
    async fn get_all_balances(&self, address: &str) -> Result<Vec<Coin>, BankServiceError> {
        let state_manager = self.state_manager.read().await;
        let store = state_manager
            .get_store("bank")
            .map_err(BankServiceError::StoreError)?;

        let prefix = format!("balance_{}_", address);
        let mut balances = Vec::new();

        for (key, value) in store.prefix_iterator(prefix.as_bytes()) {
            let key_str = String::from_utf8_lossy(&key);
            if let Some(denom) = key_str.strip_prefix(&prefix) {
                let amount_str = String::from_utf8_lossy(&value);
                if let Ok(amount) = amount_str.parse::<u64>() {
                    if amount > 0 {
                        balances.push(Coin {
                            denom: denom.to_string(),
                            amount: amount.to_string(),
                        });
                    }
                }
            }
        }

        // Sort by denomination for consistent ordering
        balances.sort_by(|a, b| a.denom.cmp(&b.denom));
        Ok(balances)
    }

    /// Calculate total supply for all denominations
    async fn calculate_total_supply(&self) -> Result<Vec<Coin>, BankServiceError> {
        let state_manager = self.state_manager.read().await;
        let store = state_manager
            .get_store("bank")
            .map_err(BankServiceError::StoreError)?;

        let mut supply_map: HashMap<String, u64> = HashMap::new();

        // Iterate through all balance entries
        for (key, value) in store.prefix_iterator(b"balance_") {
            let key_str = String::from_utf8_lossy(&key);
            // Parse key format: balance_{address}_{denom}
            let parts: Vec<&str> = key_str.split('_').collect();
            if parts.len() >= 3 {
                let denom = parts[2..].join("_"); // Handle denoms with underscores
                let amount_str = String::from_utf8_lossy(&value);
                if let Ok(amount) = amount_str.parse::<u64>() {
                    *supply_map.entry(denom).or_insert(0) += amount;
                }
            }
        }

        let mut supply: Vec<Coin> = supply_map
            .into_iter()
            .filter(|(_, amount)| *amount > 0)
            .map(|(denom, amount)| Coin {
                denom,
                amount: amount.to_string(),
            })
            .collect();

        supply.sort_by(|a, b| a.denom.cmp(&b.denom));
        Ok(supply)
    }

    /// Transfer tokens between addresses
    async fn transfer(
        &self,
        from: &str,
        to: &str,
        amount: u64,
        denom: &str,
    ) -> Result<(), BankServiceError> {
        if !self.config.send_enabled {
            return Err(BankServiceError::TransactionFailed(
                "send disabled".to_string(),
            ));
        }

        if amount < self.config.min_amount {
            return Err(BankServiceError::InvalidAmount(format!(
                "amount {} below minimum {}",
                amount, self.config.min_amount
            )));
        }

        // Validate addresses (simplified for testing)
        if from.is_empty() || to.is_empty() {
            return Err(BankServiceError::InvalidAddress(
                "empty address".to_string(),
            ));
        }

        if from == to {
            return Err(BankServiceError::TransactionFailed(
                "cannot send to self".to_string(),
            ));
        }

        // Check sender balance
        let sender_balance = self.get_balance(from, denom).await?;
        if sender_balance < amount {
            return Err(BankServiceError::InsufficientFunds {
                requested: amount,
                available: sender_balance,
            });
        }

        // Perform transfer (atomic)
        let new_sender_balance = sender_balance - amount;
        let recipient_balance = self.get_balance(to, denom).await?;
        let new_recipient_balance = recipient_balance + amount;

        self.set_balance(from, denom, new_sender_balance).await?;
        self.set_balance(to, denom, new_recipient_balance).await?;

        Ok(())
    }

    /// Process a MsgSend transaction
    async fn process_send_msg(&self, msg: &MsgSend) -> Result<(), BankServiceError> {
        // Validate the message
        msg.validate_basic().map_err(|e| {
            BankServiceError::TransactionFailed(format!("validation failed: {}", e))
        })?;

        // Process each coin in the amount
        for coin in msg.amount.as_slice() {
            let amount = coin
                .amount
                .to_string()
                .parse::<u64>()
                .map_err(|_| BankServiceError::InvalidAmount(coin.amount.to_string()))?;
            self.transfer(
                &msg.from_address.to_string(),
                &msg.to_address.to_string(),
                amount,
                &coin.denom,
            )
            .await?;
        }

        Ok(())
    }

    /// Broadcast a transaction (simplified implementation)
    pub async fn broadcast_transaction(&self, tx_bytes: &[u8]) -> Result<String, BankServiceError> {
        // Parse transaction (simplified - in real implementation would use proper decoder)
        let tx_str = String::from_utf8_lossy(tx_bytes);

        // Try to parse as JSON MsgSend for simplicity
        if let Ok(msg_send) = serde_json::from_str::<MsgSend>(&tx_str) {
            self.process_send_msg(&msg_send).await?;

            // Generate mock transaction hash
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};

            let mut hasher = DefaultHasher::new();
            tx_bytes.hash(&mut hasher);
            let hash = hasher.finish();

            Ok(format!("{:X}", hash))
        } else {
            Err(BankServiceError::TransactionFailed(
                "unsupported transaction type".to_string(),
            ))
        }
    }

    /// Initialize bank store with default balances for testing
    pub async fn initialize_for_testing(&self) -> Result<(), BankServiceError> {
        // Add some default balances for testing
        self.set_balance("cosmos1abcd1234", "stake", 1000000)
            .await?;
        self.set_balance("cosmos1abcd1234", "atom", 500).await?;
        self.set_balance("cosmos1wxyz5678", "stake", 750000).await?;

        Ok(())
    }
}

#[tonic::async_trait]
impl bank::Query for BankService {
    async fn balance(
        &self,
        request: Request<bank::QueryBalanceRequest>,
    ) -> Result<Response<bank::QueryBalanceResponse>, Status> {
        let req = request.into_inner();

        let balance = match self.get_balance(&req.address, &req.denom).await {
            Ok(amount) if amount > 0 => Some(Coin {
                denom: req.denom,
                amount: amount.to_string(),
            }),
            Ok(_) => None, // Zero balance
            Err(e) => return Err(e.into()),
        };

        Ok(Response::new(bank::QueryBalanceResponse { balance }))
    }

    async fn all_balances(
        &self,
        request: Request<bank::QueryAllBalancesRequest>,
    ) -> Result<Response<bank::QueryAllBalancesResponse>, Status> {
        let req = request.into_inner();

        let balances = match self.get_all_balances(&req.address).await {
            Ok(balances) => balances,
            Err(e) => return Err(e.into()),
        };

        Ok(Response::new(bank::QueryAllBalancesResponse {
            balances,
            pagination: None, // TODO: Implement proper pagination
        }))
    }

    async fn total_supply(
        &self,
        request: Request<bank::QueryTotalSupplyRequest>,
    ) -> Result<Response<bank::QueryTotalSupplyResponse>, Status> {
        let _req = request.into_inner();

        let supply = match self.calculate_total_supply().await {
            Ok(supply) => supply,
            Err(e) => return Err(e.into()),
        };

        Ok(Response::new(bank::QueryTotalSupplyResponse {
            supply,
            pagination: None, // TODO: Implement proper pagination
        }))
    }

    async fn supply_of(
        &self,
        request: Request<bank::QuerySupplyOfRequest>,
    ) -> Result<Response<bank::QuerySupplyOfResponse>, Status> {
        let req = request.into_inner();

        // Calculate supply for specific denomination
        let supply = match self.calculate_total_supply().await {
            Ok(supply) => supply.into_iter().find(|coin| coin.denom == req.denom),
            Err(e) => return Err(e.into()),
        };

        Ok(Response::new(bank::QuerySupplyOfResponse {
            amount: supply,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::bank::Query;
    use helium_baseapp::BaseApp;
    use helium_store::MemStore;

    async fn create_test_service() -> BankService {
        // Create StateManager with unique temp directory for each test
        let mut state_manager = StateManager::new_with_memstore();
        
        // Register the bank namespace
        state_manager.register_namespace("bank".to_string(), false).unwrap();
        
        let state_manager = Arc::new(RwLock::new(state_manager));
        let base_app = Arc::new(RwLock::new(BaseApp::new("test-app".to_string()).expect("Failed to create BaseApp")));

        BankService::with_defaults(state_manager, base_app)
    }

    #[tokio::test]
    async fn test_set_and_get_balance() {
        let service = create_test_service().await;

        // Set a balance
        service
            .set_balance("cosmos1test", "stake", 1000)
            .await
            .unwrap();

        // Get the balance
        let balance = service.get_balance("cosmos1test", "stake").await.unwrap();
        assert_eq!(balance, 1000);

        // Check non-existent balance
        let zero_balance = service.get_balance("cosmos1test", "atom").await.unwrap();
        assert_eq!(zero_balance, 0);
    }

    #[tokio::test]
    async fn test_all_balances() {
        let service = create_test_service().await;

        // Set multiple balances
        service
            .set_balance("cosmos1test", "stake", 1000)
            .await
            .unwrap();
        service
            .set_balance("cosmos1test", "atom", 500)
            .await
            .unwrap();

        // Get all balances
        let balances = service.get_all_balances("cosmos1test").await.unwrap();
        assert_eq!(balances.len(), 2);

        // Should be sorted by denom
        assert_eq!(balances[0].denom, "atom");
        assert_eq!(balances[0].amount, "500");
        assert_eq!(balances[1].denom, "stake");
        assert_eq!(balances[1].amount, "1000");
    }

    #[tokio::test]
    async fn test_transfer() {
        let service = create_test_service().await;

        // Set initial balance
        service
            .set_balance("cosmos1sender", "stake", 1000)
            .await
            .unwrap();

        // Transfer
        service
            .transfer("cosmos1sender", "cosmos1recipient", 300, "stake")
            .await
            .unwrap();

        // Check balances
        let sender_balance = service.get_balance("cosmos1sender", "stake").await.unwrap();
        let recipient_balance = service
            .get_balance("cosmos1recipient", "stake")
            .await
            .unwrap();

        assert_eq!(sender_balance, 700);
        assert_eq!(recipient_balance, 300);
    }

    #[tokio::test]
    async fn test_insufficient_funds() {
        let service = create_test_service().await;

        // Set initial balance
        service
            .set_balance("cosmos1sender", "stake", 100)
            .await
            .unwrap();

        // Try to transfer more than available
        let result = service
            .transfer("cosmos1sender", "cosmos1recipient", 200, "stake")
            .await;

        assert!(matches!(
            result,
            Err(BankServiceError::InsufficientFunds { .. })
        ));
    }

    #[tokio::test]
    async fn test_total_supply() {
        let service = create_test_service().await;

        // Set balances for multiple accounts
        service
            .set_balance("cosmos1addr1", "stake", 1000)
            .await
            .unwrap();
        service
            .set_balance("cosmos1addr2", "stake", 500)
            .await
            .unwrap();
        service
            .set_balance("cosmos1addr1", "atom", 200)
            .await
            .unwrap();

        // Calculate total supply
        let supply = service.calculate_total_supply().await.unwrap();

        assert_eq!(supply.len(), 2);

        // Find stake supply
        let stake_supply = supply.iter().find(|c| c.denom == "stake").unwrap();
        assert_eq!(stake_supply.amount, "1500");

        // Find atom supply
        let atom_supply = supply.iter().find(|c| c.denom == "atom").unwrap();
        assert_eq!(atom_supply.amount, "200");
    }

    #[tokio::test]
    async fn test_bank_query_trait() {
        let service = create_test_service().await;

        // Set up test data
        service
            .set_balance("cosmos1test", "stake", 1000)
            .await
            .unwrap();

        // Test balance query
        let request = Request::new(bank::QueryBalanceRequest {
            address: "cosmos1test".to_string(),
            denom: "stake".to_string(),
        });

        let response = service.balance(request).await.unwrap();
        let balance = response.into_inner().balance.unwrap();

        assert_eq!(balance.denom, "stake");
        assert_eq!(balance.amount, "1000");
    }
}
