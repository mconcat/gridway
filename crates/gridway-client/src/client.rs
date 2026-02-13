//! HTTP client for communicating with a gridway node.
//!
//! Provides methods for submitting transactions and querying state.

use crate::{ClientError, SignedTx};
use serde::Deserialize;

/// HTTP client for a gridway node.
pub struct GridwayClient {
    base_url: String,
    client: reqwest::Client,
}

/// Response from submitting a transaction.
#[derive(Debug, Clone, Deserialize)]
pub struct SubmitTxResponse {
    pub status: String,
    pub tx_hash: String,
}

/// Response from a balance query.
#[derive(Debug, Clone, Deserialize)]
pub struct BalanceResponse {
    pub address: String,
    pub denom: String,
    pub balance: u64,
}

/// Response from an account query.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountResponse {
    pub address: String,
    pub public_key: String,
    pub sequence: u64,
}

/// Response from a node status query.
#[derive(Debug, Clone, Deserialize)]
pub struct StatusResponse {
    pub chain_id: String,
    pub state_root: String,
    pub pending_tx_count: usize,
}

/// Error response from the node.
#[derive(Debug, Deserialize)]
struct ErrorResponse {
    error: String,
}

impl GridwayClient {
    /// Create a new client connecting to the given node URL.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use gridway_client::GridwayClient;
    /// let client = GridwayClient::new("http://localhost:4547");
    /// ```
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            client: reqwest::Client::new(),
        }
    }

    /// Submit a signed transaction to the node.
    pub async fn submit_tx(&self, tx: &SignedTx) -> Result<SubmitTxResponse, ClientError> {
        let url = format!("{}/tx", self.base_url);
        let body = tx.to_json()?;

        let resp = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await?;

        if resp.status().is_success() {
            let result: SubmitTxResponse = resp.json().await?;
            Ok(result)
        } else {
            let status = resp.status();
            match resp.json::<ErrorResponse>().await {
                Ok(err_resp) => Err(ClientError::NodeError(err_resp.error)),
                Err(_) => Err(ClientError::NodeError(format!("HTTP {}", status))),
            }
        }
    }

    /// Query the balance of an address for a specific denomination.
    pub async fn get_balance(
        &self,
        address: &str,
        denom: &str,
    ) -> Result<BalanceResponse, ClientError> {
        let url = format!("{}/balance/{}/{}", self.base_url, address, denom);
        let resp = self.client.get(&url).send().await?;

        if resp.status().is_success() {
            let result: BalanceResponse = resp.json().await?;
            Ok(result)
        } else {
            let status = resp.status();
            match resp.json::<ErrorResponse>().await {
                Ok(err_resp) => Err(ClientError::NodeError(err_resp.error)),
                Err(_) => Err(ClientError::NodeError(format!("HTTP {}", status))),
            }
        }
    }

    /// Query account information for an address.
    pub async fn get_account(&self, address: &str) -> Result<AccountResponse, ClientError> {
        let url = format!("{}/account/{}", self.base_url, address);
        let resp = self.client.get(&url).send().await?;

        if resp.status().is_success() {
            let result: AccountResponse = resp.json().await?;
            Ok(result)
        } else {
            let status = resp.status();
            match resp.json::<ErrorResponse>().await {
                Ok(err_resp) => Err(ClientError::NodeError(err_resp.error)),
                Err(_) => Err(ClientError::NodeError(format!("HTTP {}", status))),
            }
        }
    }

    /// Query node status.
    pub async fn get_status(&self) -> Result<StatusResponse, ClientError> {
        let url = format!("{}/status", self.base_url);
        let resp = self.client.get(&url).send().await?;

        if resp.status().is_success() {
            let result: StatusResponse = resp.json().await?;
            Ok(result)
        } else {
            let status = resp.status();
            match resp.json::<ErrorResponse>().await {
                Ok(err_resp) => Err(ClientError::NodeError(err_resp.error)),
                Err(_) => Err(ClientError::NodeError(format!("HTTP {}", status))),
            }
        }
    }
}
