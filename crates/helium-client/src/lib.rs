//! Client library for interacting with the gridway blockchain.
//!
//! This crate provides HTTP client implementations for communicating
//! with gridway blockchain nodes via RPC and REST API, as well as
//! CLI framework for blockchain operations.

pub mod cli;
pub mod config;
pub mod keys;
pub mod tx_builder;

pub use tx_builder::{SignedTx, SigningConfig, TxBuilder};

use reqwest::Client as HttpClient;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;
use url::Url;

/// Client error types
#[derive(Error, Debug)]
pub enum ClientError {
    /// HTTP request error
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// JSON parsing error
    #[error("json parsing failed: {0}")]
    Json(#[from] serde_json::Error),

    /// URL parsing error
    #[error("invalid url: {0}")]
    Url(#[from] url::ParseError),

    /// RPC error
    #[error("rpc error {code}: {message}")]
    Rpc { code: i32, message: String },

    /// Invalid response
    #[error("invalid response: {0}")]
    InvalidResponse(String),

    /// Keys error
    #[error("keys error: {0}")]
    Keys(#[from] crate::keys::KeysError),
}

/// Result type for client operations
pub type Result<T> = std::result::Result<T, ClientError>;

/// RPC request
#[derive(Serialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub id: String,
    pub method: String,
    pub params: serde_json::Value,
}

/// RPC response
#[derive(Deserialize)]
pub struct RpcResponse<T> {
    pub jsonrpc: String,
    pub id: String,
    pub result: Option<T>,
    pub error: Option<RpcError>,
}

/// RPC error
#[derive(Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
}

/// Node status information
#[derive(Deserialize, Debug)]
pub struct NodeStatus {
    pub node_info: NodeInfo,
    pub sync_info: SyncInfo,
    pub validator_info: ValidatorInfo,
}

/// Node information
#[derive(Deserialize, Debug)]
pub struct NodeInfo {
    pub id: String,
    pub moniker: String,
    pub network: String,
    pub version: String,
}

/// Sync information
#[derive(Deserialize, Debug)]
pub struct SyncInfo {
    pub latest_block_height: String,
    pub latest_block_time: String,
    pub catching_up: bool,
}

/// Validator information
#[derive(Deserialize, Debug)]
pub struct ValidatorInfo {
    pub address: String,
    pub pub_key: Option<serde_json::Value>,
    pub voting_power: String,
}

/// Transaction broadcast response
#[derive(Deserialize, Debug)]
pub struct BroadcastResponse {
    pub txhash: String,
    pub code: u32,
    pub log: String,
    pub height: String,
}

/// Simulation result from transaction simulation
#[derive(Deserialize, Debug)]
pub struct SimulationResult {
    pub gas_used: u64,
    pub gas_wanted: u64,
    pub events: Vec<SimulationEvent>,
    pub log: String,
}

/// Event from transaction simulation
#[derive(Deserialize, Debug)]
pub struct SimulationEvent {
    pub event_type: String,
    pub attributes: Vec<SimulationAttribute>,
}

/// Attribute from simulation event
#[derive(Deserialize, Debug)]
pub struct SimulationAttribute {
    pub key: String,
    pub value: String,
}

/// Account information from the chain
#[derive(Deserialize, Debug)]
pub struct AccountInfo {
    pub address: String,
    pub account_number: u64,
    pub sequence: u64,
    pub pub_key: Option<String>,
}

/// Query response
#[derive(Deserialize, Debug)]
pub struct QueryResponse<T> {
    pub height: String,
    pub result: T,
}

/// Balance information
#[derive(Deserialize, Debug)]
pub struct Balance {
    pub denom: String,
    pub amount: String,
}

/// Supply information
#[derive(Deserialize, Debug)]
pub struct Supply {
    pub denom: String,
    pub amount: String,
}

/// Transaction information
#[derive(Deserialize, Debug)]
pub struct TxInfo {
    pub hash: String,
    pub height: u64,
    pub code: u32,
    pub log: String,
    pub gas_used: u64,
    pub gas_wanted: u64,
}

/// Client configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Node URL
    pub node_url: Url,
    /// Request timeout
    pub timeout: Duration,
    /// Chain ID
    pub chain_id: String,
}

impl Config {
    /// Create a new configuration
    pub fn new(node_url: &str, chain_id: &str) -> Result<Self> {
        Ok(Self {
            node_url: Url::parse(node_url)?,
            timeout: Duration::from_secs(30),
            chain_id: chain_id.to_string(),
        })
    }
}

/// Gridway blockchain client
pub struct Client {
    config: Config,
    http_client: HttpClient,
}

impl Client {
    /// Create a new client
    pub fn new(config: Config) -> Self {
        let http_client = HttpClient::builder()
            .timeout(config.timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            http_client,
        }
    }

    /// Make an RPC request
    async fn rpc_request<T>(&self, method: &str, params: serde_json::Value) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let request = RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: "1".to_string(),
            method: method.to_string(),
            params,
        };

        let response = self
            .http_client
            .post(self.config.node_url.clone())
            .json(&request)
            .send()
            .await?;

        let rpc_response: RpcResponse<T> = response.json().await?;

        if let Some(error) = rpc_response.error {
            return Err(ClientError::Rpc {
                code: error.code,
                message: error.message,
            });
        }

        rpc_response
            .result
            .ok_or_else(|| ClientError::InvalidResponse("missing result field".to_string()))
    }

    /// Get node status
    pub async fn status(&self) -> Result<NodeStatus> {
        self.rpc_request("status", serde_json::Value::Null).await
    }

    /// Get node health
    pub async fn health(&self) -> Result<()> {
        let response = self
            .http_client
            .get(self.config.node_url.join("/health")?)
            .send()
            .await?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(ClientError::InvalidResponse(format!(
                "health check failed with status {}",
                response.status()
            )))
        }
    }

    /// Broadcast a transaction
    pub async fn broadcast_tx(&self, tx_bytes: &[u8]) -> Result<BroadcastResponse> {
        let params = serde_json::json!({
            "tx": base64::encode(tx_bytes)
        });

        self.rpc_request("broadcast_tx_commit", params).await
    }

    /// Query the application
    pub async fn abci_query(
        &self,
        path: &str,
        data: &[u8],
        height: Option<u64>,
    ) -> Result<serde_json::Value> {
        let params = serde_json::json!({
            "path": path,
            "data": base64::encode(data),
            "height": height.map(|h| h.to_string())
        });

        self.rpc_request("abci_query", params).await
    }

    /// Get block by height
    pub async fn block(&self, height: Option<u64>) -> Result<serde_json::Value> {
        let params = if let Some(h) = height {
            serde_json::json!({ "height": h.to_string() })
        } else {
            serde_json::Value::Null
        };

        self.rpc_request("block", params).await
    }

    /// Query account balance
    pub async fn query_balance(&self, address: &str, denom: Option<&str>) -> Result<Vec<Balance>> {
        let path = "/cosmos.bank.v1beta1.Query/Balance";
        let query_data = if let Some(d) = denom {
            serde_json::json!({
                "address": address,
                "denom": d
            })
        } else {
            serde_json::json!({
                "address": address
            })
        };

        let data = serde_json::to_vec(&query_data)?;
        let response = self.abci_query(path, &data, None).await?;

        // Parse the actual ABCI query response
        if let Some(balances) = response.get("balances") {
            if let Some(balances_array) = balances.as_array() {
                let mut result = Vec::new();
                for balance in balances_array {
                    if let (Some(denom), Some(amount)) = (
                        balance.get("denom").and_then(|d| d.as_str()),
                        balance.get("amount").and_then(|a| a.as_str()),
                    ) {
                        result.push(Balance {
                            denom: denom.to_string(),
                            amount: amount.to_string(),
                        });
                    }
                }
                return Ok(result);
            }
        }

        // If parsing fails or no data found, return empty
        Ok(vec![])
    }

    /// Query total supply
    pub async fn query_supply(&self, denom: Option<&str>) -> Result<Vec<Supply>> {
        let path = "/cosmos.bank.v1beta1.Query/TotalSupply";
        let query_data = if let Some(d) = denom {
            serde_json::json!({
                "denom": d
            })
        } else {
            serde_json::json!({})
        };

        let data = serde_json::to_vec(&query_data)?;
        let response = self.abci_query(path, &data, None).await?;

        // Parse the actual ABCI query response
        if let Some(supply) = response.get("supply") {
            if let Some(supply_array) = supply.as_array() {
                let mut result = Vec::new();
                for supply_item in supply_array {
                    if let (Some(denom), Some(amount)) = (
                        supply_item.get("denom").and_then(|d| d.as_str()),
                        supply_item.get("amount").and_then(|a| a.as_str()),
                    ) {
                        result.push(Supply {
                            denom: denom.to_string(),
                            amount: amount.to_string(),
                        });
                    }
                }
                return Ok(result);
            }
        }

        // If parsing fails or no data found, return empty
        Ok(vec![])
    }

    /// Query account information
    pub async fn query_account(&self, address: &str) -> Result<AccountInfo> {
        let path = "/cosmos.auth.v1beta1.Query/Account";
        let query_data = serde_json::json!({
            "address": address
        });

        let data = serde_json::to_vec(&query_data)?;
        let response = self.abci_query(path, &data, None).await?;

        // Parse the actual ABCI query response
        if let Some(account) = response.get("account") {
            let account_number = account
                .get("account_number")
                .and_then(|n| n.as_u64())
                .unwrap_or(0);
            let sequence = account
                .get("sequence")
                .and_then(|s| s.as_u64())
                .unwrap_or(0);
            let pub_key = account
                .get("pub_key")
                .and_then(|pk| pk.as_str())
                .map(|s| s.to_string());

            return Ok(AccountInfo {
                address: address.to_string(),
                account_number,
                sequence,
                pub_key,
            });
        }

        // If parsing fails, return error instead of mock data
        Err(ClientError::InvalidResponse(format!(
            "Account not found or invalid response for address: {address}"
        )))
    }

    /// Simulate a transaction to estimate gas usage
    pub async fn simulate_transaction(&self, tx_bytes: &[u8]) -> Result<SimulationResult> {
        let path = "/cosmos.tx.v1beta1.Service/Simulate";
        let query_data = serde_json::json!({
            "tx_bytes": base64::encode(tx_bytes)
        });

        let data = serde_json::to_vec(&query_data)?;
        let response = self.abci_query(path, &data, None).await?;

        // Parse the actual simulation response
        if let Some(gas_info) = response.get("gas_info") {
            let gas_used = gas_info
                .get("gas_used")
                .and_then(|g| g.as_u64())
                .unwrap_or(0);
            let gas_wanted = gas_info
                .get("gas_wanted")
                .and_then(|g| g.as_u64())
                .unwrap_or(gas_used);

            let mut events = vec![];
            if let Some(result) = response.get("result") {
                if let Some(events_array) = result.get("events").and_then(|e| e.as_array()) {
                    for event in events_array {
                        if let Some(event_type) = event.get("type").and_then(|t| t.as_str()) {
                            let mut attributes = vec![];
                            if let Some(attrs) = event.get("attributes").and_then(|a| a.as_array())
                            {
                                for attr in attrs {
                                    if let (Some(key), Some(value)) = (
                                        attr.get("key").and_then(|k| k.as_str()),
                                        attr.get("value").and_then(|v| v.as_str()),
                                    ) {
                                        attributes.push(SimulationAttribute {
                                            key: key.to_string(),
                                            value: value.to_string(),
                                        });
                                    }
                                }
                            }
                            events.push(SimulationEvent {
                                event_type: event_type.to_string(),
                                attributes,
                            });
                        }
                    }
                }
            }

            let log = response
                .get("result")
                .and_then(|r| r.get("log"))
                .and_then(|l| l.as_str())
                .unwrap_or("simulation successful")
                .to_string();

            return Ok(SimulationResult {
                gas_used,
                gas_wanted,
                events,
                log,
            });
        }

        // If parsing fails, return error instead of mock estimation
        Err(ClientError::InvalidResponse(
            "Invalid simulation response format".to_string(),
        ))
    }

    /// Get account information from the chain
    pub async fn get_account(&self, address: &str) -> Result<AccountInfo> {
        // Use the query_account method which now properly parses responses
        self.query_account(address).await
    }

    /// Query transaction by hash
    pub async fn query_tx(&self, hash: &str) -> Result<TxInfo> {
        let params = serde_json::json!({
            "hash": hash
        });

        let response: serde_json::Value =
            self.rpc_request("tx", params).await.map_err(|e| match e {
                ClientError::Rpc { code: -32603, .. } => {
                    ClientError::InvalidResponse("Transaction not found".to_string())
                }
                other => other,
            })?;

        // Parse the actual transaction response
        if let Some(tx_result) = response.get("tx_result") {
            let height = response
                .get("height")
                .and_then(|h| h.as_str())
                .and_then(|h| h.parse::<u64>().ok())
                .unwrap_or(0);
            let code = tx_result.get("code").and_then(|c| c.as_u64()).unwrap_or(0) as u32;
            let log = tx_result
                .get("log")
                .and_then(|l| l.as_str())
                .unwrap_or("No log")
                .to_string();
            let gas_used = tx_result
                .get("gas_used")
                .and_then(|g| g.as_str())
                .and_then(|g| g.parse::<u64>().ok())
                .unwrap_or(0);
            let gas_wanted = tx_result
                .get("gas_wanted")
                .and_then(|g| g.as_str())
                .and_then(|g| g.parse::<u64>().ok())
                .unwrap_or(0);

            return Ok(TxInfo {
                hash: hash.to_string(),
                height,
                code,
                log,
                gas_used,
                gas_wanted,
            });
        }

        // If parsing fails, return error
        Err(ClientError::InvalidResponse(format!(
            "Invalid transaction response format for hash: {hash}"
        )))
    }
}

/// Base64 encoding module
mod base64 {
    use base64::Engine;

    pub fn encode<T: AsRef<[u8]>>(input: T) -> String {
        base64::engine::general_purpose::STANDARD.encode(input)
    }

    #[allow(dead_code)]
    pub fn decode<T: AsRef<[u8]>>(input: T) -> Result<Vec<u8>, base64::DecodeError> {
        base64::engine::general_purpose::STANDARD.decode(input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let config = Config::new("http://localhost:26657", "test-chain").unwrap();
        assert_eq!(config.node_url.as_str(), "http://localhost:26657/");
        assert_eq!(config.chain_id, "test-chain");
    }

    #[tokio::test]
    async fn test_client_creation() {
        let config = Config::new("http://localhost:26657", "test-chain").unwrap();
        let client = Client::new(config);
        assert_eq!(client.config.chain_id, "test-chain");
    }
}
