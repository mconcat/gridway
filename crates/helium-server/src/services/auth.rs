//! Auth service implementation with state store integration
//!
//! This module provides a production-ready auth service that integrates with
//! the state manager for persistent account storage and authentication.

use crate::grpc::{auth, AuthParams, BaseAccount};
use helium_baseapp::BaseApp;
use helium_store::{KVStore, StateManager, StoreError};
use helium_types::address::AccAddress;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};

/// Auth service error types
#[derive(Debug, thiserror::Error)]
pub enum AuthServiceError {
    #[error("store error: {0}")]
    StoreError(#[from] StoreError),

    #[error("invalid address: {0}")]
    InvalidAddress(String),

    #[error("account not found: {0}")]
    AccountNotFound(String),

    #[error("account already exists: {0}")]
    AccountExists(String),

    #[error("serialization error: {0}")]
    SerializationError(String),

    #[error("invalid account number: {0}")]
    InvalidAccountNumber(String),

    #[error("invalid sequence: {0}")]
    InvalidSequence(String),
}

impl From<AuthServiceError> for Status {
    fn from(err: AuthServiceError) -> Self {
        match err {
            AuthServiceError::StoreError(_) => Status::internal(err.to_string()),
            AuthServiceError::InvalidAddress(_) => Status::invalid_argument(err.to_string()),
            AuthServiceError::AccountNotFound(_) => Status::not_found(err.to_string()),
            AuthServiceError::AccountExists(_) => Status::already_exists(err.to_string()),
            AuthServiceError::SerializationError(_) => Status::internal(err.to_string()),
            AuthServiceError::InvalidAccountNumber(_) => Status::invalid_argument(err.to_string()),
            AuthServiceError::InvalidSequence(_) => Status::invalid_argument(err.to_string()),
        }
    }
}

/// Production auth service with state store integration
pub struct AuthService {
    /// State manager for persistent storage
    state_manager: Arc<RwLock<StateManager>>,
    /// Base application for transaction processing
    #[allow(dead_code)]
    base_app: Arc<RwLock<BaseApp>>,
    /// Configuration parameters
    config: AuthConfig,
    /// Next account number to assign
    next_account_number: Arc<RwLock<u64>>,
}

/// Auth service configuration
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Maximum memo characters allowed
    pub max_memo_characters: u64,
    /// Transaction signature limit
    pub tx_sig_limit: u64,
    /// Transaction size cost per byte
    pub tx_size_cost_per_byte: u64,
    /// Signature verification cost for ed25519
    pub sig_verify_cost_ed25519: u64,
    /// Signature verification cost for secp256k1
    pub sig_verify_cost_secp256k1: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            max_memo_characters: 256,
            tx_sig_limit: 7,
            tx_size_cost_per_byte: 10,
            sig_verify_cost_ed25519: 590,
            sig_verify_cost_secp256k1: 1000,
        }
    }
}

/// Account information stored in state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredAccount {
    /// Account address
    pub address: String,
    /// Account number (unique identifier)
    pub account_number: u64,
    /// Sequence number (for replay protection)
    pub sequence: u64,
    /// Public key (if set)
    pub pub_key: Option<Vec<u8>>,
}

impl AuthService {
    /// Create a new auth service
    pub fn new(
        state_manager: Arc<RwLock<StateManager>>,
        base_app: Arc<RwLock<BaseApp>>,
        config: AuthConfig,
    ) -> Self {
        Self {
            state_manager,
            base_app,
            config,
            next_account_number: Arc::new(RwLock::new(1)),
        }
    }

    /// Create a new auth service with default configuration
    pub fn with_defaults(
        state_manager: Arc<RwLock<StateManager>>,
        base_app: Arc<RwLock<BaseApp>>,
    ) -> Self {
        Self::new(state_manager, base_app, AuthConfig::default())
    }

    /// Get account by address
    async fn get_account(&self, address: &str) -> Result<Option<StoredAccount>, AuthServiceError> {
        // Validate address format
        AccAddress::from_str(address)
            .map_err(|_| AuthServiceError::InvalidAddress(address.to_string()))?;

        let state_manager = self.state_manager.read().await;
        let store = state_manager
            .get_store("auth")
            .map_err(AuthServiceError::StoreError)?;

        let key = format!("account_{address}");
        match store.get(key.as_bytes()) {
            Ok(Some(data)) => {
                let account: StoredAccount = serde_json::from_slice(&data)
                    .map_err(|e| AuthServiceError::SerializationError(e.to_string()))?;
                Ok(Some(account))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(AuthServiceError::StoreError(e)),
        }
    }

    /// Create a new account
    async fn create_account(
        &self,
        address: &str,
        pub_key: Option<Vec<u8>>,
    ) -> Result<StoredAccount, AuthServiceError> {
        // Validate address format
        AccAddress::from_str(address)
            .map_err(|_| AuthServiceError::InvalidAddress(address.to_string()))?;

        // Check if account already exists
        if self.get_account(address).await?.is_some() {
            return Err(AuthServiceError::AccountExists(address.to_string()));
        }

        // Get next account number
        let account_number = {
            let mut next_num = self.next_account_number.write().await;
            let num = *next_num;
            *next_num += 1;
            num
        };

        let account = StoredAccount {
            address: address.to_string(),
            account_number,
            sequence: 0,
            pub_key,
        };

        // Store the account
        self.store_account(&account).await?;

        // Update next account number in persistent storage
        self.store_next_account_number(account_number + 1).await?;

        Ok(account)
    }

    /// Store account in state
    async fn store_account(&self, account: &StoredAccount) -> Result<(), AuthServiceError> {
        let mut state_manager = self.state_manager.write().await;
        let store = state_manager
            .get_store_mut("auth")
            .map_err(AuthServiceError::StoreError)?;

        let key = format!("account_{}", account.address);
        let data = serde_json::to_vec(account)
            .map_err(|e| AuthServiceError::SerializationError(e.to_string()))?;

        store
            .set(&key.into_bytes(), &data)
            .map_err(AuthServiceError::StoreError)?;

        // Commit changes immediately for consistency
        state_manager
            .commit()
            .map_err(AuthServiceError::StoreError)?;

        Ok(())
    }

    /// Store next account number
    async fn store_next_account_number(&self, next_num: u64) -> Result<(), AuthServiceError> {
        let mut state_manager = self.state_manager.write().await;
        let store = state_manager
            .get_store_mut("auth")
            .map_err(AuthServiceError::StoreError)?;

        store
            .set(b"next_account_number", &next_num.to_string().into_bytes())
            .map_err(AuthServiceError::StoreError)?;

        state_manager
            .commit()
            .map_err(AuthServiceError::StoreError)?;

        Ok(())
    }

    /// Load next account number from storage
    async fn load_next_account_number(&self) -> Result<(), AuthServiceError> {
        let state_manager = self.state_manager.read().await;
        let store = state_manager
            .get_store("auth")
            .map_err(AuthServiceError::StoreError)?;

        match store.get(b"next_account_number") {
            Ok(Some(data)) => {
                let num_str = String::from_utf8_lossy(&data);
                let num = num_str
                    .parse::<u64>()
                    .map_err(|_| AuthServiceError::InvalidAccountNumber(num_str.to_string()))?;
                *self.next_account_number.write().await = num;
            }
            Ok(None) => {
                // Initialize to 1 if not found
                *self.next_account_number.write().await = 1;
            }
            Err(e) => return Err(AuthServiceError::StoreError(e)),
        }

        Ok(())
    }

    /// Get all accounts
    async fn get_all_accounts(&self) -> Result<Vec<StoredAccount>, AuthServiceError> {
        let state_manager = self.state_manager.read().await;
        let store = state_manager
            .get_store("auth")
            .map_err(AuthServiceError::StoreError)?;

        let mut accounts = Vec::new();

        for (key, value) in store.prefix_iterator(b"account_") {
            let key_str = String::from_utf8_lossy(&key);
            if key_str.starts_with("account_") && key_str != "next_account_number" {
                let account: StoredAccount = serde_json::from_slice(&value)
                    .map_err(|e| AuthServiceError::SerializationError(e.to_string()))?;
                accounts.push(account);
            }
        }

        // Sort by account number for consistent ordering
        accounts.sort_by(|a, b| a.account_number.cmp(&b.account_number));
        Ok(accounts)
    }

    /// Convert StoredAccount to gRPC BaseAccount
    fn to_base_account(&self, stored: &StoredAccount) -> BaseAccount {
        BaseAccount {
            address: stored.address.clone(),
            pub_key: stored.pub_key.as_ref().map(|key| crate::grpc::Any {
                type_url: "/cosmos.crypto.secp256k1.PubKey".to_string(),
                value: key.clone(),
            }),
            account_number: stored.account_number,
            sequence: stored.sequence,
        }
    }

    /// Get auth module parameters
    pub fn get_params(&self) -> AuthParams {
        AuthParams {
            max_memo_characters: self.config.max_memo_characters,
            tx_sig_limit: self.config.tx_sig_limit,
            tx_size_cost_per_byte: self.config.tx_size_cost_per_byte,
            sig_verify_cost_ed25519: self.config.sig_verify_cost_ed25519,
            sig_verify_cost_secp256k1: self.config.sig_verify_cost_secp256k1,
        }
    }

    /// Initialize auth store with default accounts for testing
    pub async fn initialize_for_testing(&self) -> Result<(), AuthServiceError> {
        // Load next account number from storage
        self.load_next_account_number().await?;

        // Create some default accounts for testing if they don't exist
        if self
            .get_account("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux")
            .await?
            .is_none()
        {
            self.create_account("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux", None)
                .await?;
        }

        if self
            .get_account("cosmos1fl48vsnmsdzcv85q5d2q4z5ajdha8yu34mf0eh")
            .await?
            .is_none()
        {
            self.create_account("cosmos1fl48vsnmsdzcv85q5d2q4z5ajdha8yu34mf0eh", None)
                .await?;
        }

        Ok(())
    }

    /// Increment account sequence (for transaction processing)
    pub async fn increment_sequence(&self, address: &str) -> Result<(), AuthServiceError> {
        let mut account = self
            .get_account(address)
            .await?
            .ok_or_else(|| AuthServiceError::AccountNotFound(address.to_string()))?;

        account.sequence += 1;
        self.store_account(&account).await?;

        Ok(())
    }

    /// Set account public key
    pub async fn set_public_key(
        &self,
        address: &str,
        pub_key: Vec<u8>,
    ) -> Result<(), AuthServiceError> {
        let mut account = self
            .get_account(address)
            .await?
            .ok_or_else(|| AuthServiceError::AccountNotFound(address.to_string()))?;

        account.pub_key = Some(pub_key);
        self.store_account(&account).await?;

        Ok(())
    }
}

#[tonic::async_trait]
impl auth::Query for AuthService {
    async fn account(
        &self,
        request: Request<auth::QueryAccountRequest>,
    ) -> Result<Response<auth::QueryAccountResponse>, Status> {
        let req = request.into_inner();

        let account = match self.get_account(&req.address).await {
            Ok(Some(stored)) => Some(self.to_base_account(&stored)),
            Ok(None) => None,
            Err(e) => return Err(e.into()),
        };

        Ok(Response::new(auth::QueryAccountResponse { account }))
    }

    async fn accounts(
        &self,
        request: Request<auth::QueryAccountsRequest>,
    ) -> Result<Response<auth::QueryAccountsResponse>, Status> {
        let _req = request.into_inner();

        let accounts = match self.get_all_accounts().await {
            Ok(stored_accounts) => stored_accounts
                .iter()
                .map(|stored| self.to_base_account(stored))
                .collect(),
            Err(e) => return Err(e.into()),
        };

        Ok(Response::new(auth::QueryAccountsResponse {
            accounts,
            pagination: None, // TODO: Implement proper pagination
        }))
    }

    async fn params(
        &self,
        request: Request<auth::QueryParamsRequest>,
    ) -> Result<Response<auth::QueryParamsResponse>, Status> {
        let _req = request.into_inner();

        Ok(Response::new(auth::QueryParamsResponse {
            params: self.get_params(),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grpc::auth::Query;
    use helium_baseapp::BaseApp;

    async fn create_test_service() -> AuthService {
        let mut state_manager = StateManager::new_with_memstore();
        state_manager
            .register_namespace("auth".to_string(), false)
            .unwrap();
        let state_manager = Arc::new(RwLock::new(state_manager));
        let base_app = Arc::new(RwLock::new(BaseApp::new("test-app".to_string()).unwrap()));

        AuthService::with_defaults(state_manager, base_app)
    }

    #[tokio::test]
    async fn test_create_and_get_account() {
        let service = create_test_service().await;

        // Create an account
        let account = service
            .create_account("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux", None)
            .await
            .unwrap();

        assert_eq!(
            account.address,
            "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux"
        );
        assert_eq!(account.account_number, 1);
        assert_eq!(account.sequence, 0);
        assert!(account.pub_key.is_none());

        // Retrieve the account
        let retrieved = service
            .get_account("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.address, account.address);
        assert_eq!(retrieved.account_number, account.account_number);
    }

    #[tokio::test]
    async fn test_account_sequence_increment() {
        let service = create_test_service().await;

        // Create an account
        service
            .create_account("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux", None)
            .await
            .unwrap();

        // Increment sequence
        service
            .increment_sequence("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux")
            .await
            .unwrap();

        // Check sequence was incremented
        let account = service
            .get_account("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.sequence, 1);
    }

    #[tokio::test]
    async fn test_set_public_key() {
        let service = create_test_service().await;

        // Create an account
        service
            .create_account("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux", None)
            .await
            .unwrap();

        // Set public key
        let pub_key = vec![1, 2, 3, 4];
        service
            .set_public_key(
                "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux",
                pub_key.clone(),
            )
            .await
            .unwrap();

        // Check public key was set
        let account = service
            .get_account("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(account.pub_key, Some(pub_key));
    }

    #[tokio::test]
    async fn test_account_numbers() {
        let service = create_test_service().await;

        // Create multiple accounts
        let account1 = service
            .create_account("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux", None)
            .await
            .unwrap();
        let account2 = service
            .create_account("cosmos1fl48vsnmsdzcv85q5d2q4z5ajdha8yu34mf0eh", None)
            .await
            .unwrap();

        // Account numbers should be sequential
        assert_eq!(account1.account_number, 1);
        assert_eq!(account2.account_number, 2);
    }

    #[tokio::test]
    async fn test_auth_query_trait() {
        let service = create_test_service().await;

        // Create an account
        service
            .create_account("cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux", None)
            .await
            .unwrap();

        // Test account query
        let request = Request::new(auth::QueryAccountRequest {
            address: "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux".to_string(),
        });

        let response = service.account(request).await.unwrap();
        let account = response.into_inner().account.unwrap();

        assert_eq!(
            account.address,
            "cosmos1syavy2npfyt9tcncdtsdzf7kny9lh777pahuux"
        );
        assert_eq!(account.account_number, 1);
        assert_eq!(account.sequence, 0);
    }

    #[tokio::test]
    async fn test_params_query() {
        let service = create_test_service().await;

        let request = Request::new(auth::QueryParamsRequest {});
        let response = service.params(request).await.unwrap();
        let params = response.into_inner().params;

        assert_eq!(params.max_memo_characters, 256);
        assert_eq!(params.tx_sig_limit, 7);
        assert_eq!(params.sig_verify_cost_secp256k1, 1000);
    }
}
