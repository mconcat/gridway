//! gRPC service definitions for helium blockchain
//!
//! This module provides gRPC service interfaces compatible with Cosmos SDK,
//! including Bank, Auth, and Tx services.

pub mod services;

use serde::{Deserialize, Serialize};
use tonic::{Request, Response, Status};

// Import proto types for wrapping
use helium_proto::{
    cometbft::abci::v1::{Event as ProtoEvent, EventAttribute as ProtoEventAttribute},
    cosmos::{
        base::abci::v1beta1::{
            AbciMessageLog as ProtoABCIMessageLog, Attribute as ProtoStringAttribute,
            Event as CosmosEvent, StringEvent as ProtoStringEvent,
        },
        tx::v1beta1::{GasInfo as ProtoGasInfo, Result as ProtoTxResult},
    },
};

// Import the proto TxResponse for internal use
use helium_proto::cosmos::tx::v1beta1::TxResponse as ProtoTxResponse;

// Serde wrappers for proto types that need REST API serialization

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub r#type: String,
    pub attributes: Vec<EventAttribute>,
}

impl From<ProtoEvent> for Event {
    fn from(proto: ProtoEvent) -> Self {
        Self {
            r#type: proto.r#type,
            attributes: proto.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<Event> for ProtoEvent {
    fn from(event: Event) -> Self {
        Self {
            r#type: event.r#type,
            attributes: event.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<CosmosEvent> for Event {
    fn from(proto: CosmosEvent) -> Self {
        Self {
            r#type: proto.r#type,
            attributes: proto
                .attributes
                .into_iter()
                .map(|attr| EventAttribute {
                    key: String::from_utf8_lossy(&attr.key).to_string(),
                    value: String::from_utf8_lossy(&attr.value).to_string(),
                    index: attr.index,
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EventAttribute {
    pub key: String,
    pub value: String,
    pub index: bool,
}

impl From<ProtoEventAttribute> for EventAttribute {
    fn from(proto: ProtoEventAttribute) -> Self {
        Self {
            key: proto.key,
            value: proto.value,
            index: proto.index,
        }
    }
}

impl From<EventAttribute> for ProtoEventAttribute {
    fn from(attr: EventAttribute) -> Self {
        Self {
            key: attr.key,
            value: attr.value,
            index: attr.index,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ABCIMessageLog {
    pub msg_index: u32,
    pub log: String,
    pub events: Vec<StringEvent>,
}

impl From<ProtoABCIMessageLog> for ABCIMessageLog {
    fn from(proto: ProtoABCIMessageLog) -> Self {
        Self {
            msg_index: proto.msg_index,
            log: proto.log,
            events: proto.events.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StringEvent {
    #[serde(rename = "type")]
    pub r#type: String,
    pub attributes: Vec<StringAttribute>,
}

impl From<ProtoStringEvent> for StringEvent {
    fn from(proto: ProtoStringEvent) -> Self {
        Self {
            r#type: proto.r#type,
            attributes: proto.attributes.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct StringAttribute {
    pub key: String,
    pub value: String,
}

impl From<ProtoStringAttribute> for StringAttribute {
    fn from(proto: ProtoStringAttribute) -> Self {
        Self {
            key: proto.key,
            value: proto.value,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GasInfo {
    pub gas_wanted: u64,
    pub gas_used: u64,
}

impl From<ProtoGasInfo> for GasInfo {
    fn from(proto: ProtoGasInfo) -> Self {
        Self {
            gas_wanted: proto.gas_wanted,
            gas_used: proto.gas_used,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Result_ {
    #[serde(with = "base64_bytes")]
    pub data: Vec<u8>,
    pub log: String,
    pub events: Vec<Event>,
}

impl From<ProtoTxResult> for Result_ {
    fn from(proto: ProtoTxResult) -> Self {
        Self {
            data: proto.data,
            log: proto.log,
            // ProtoTxResult contains CosmosEvent which needs conversion
            events: proto.events.into_iter().map(Event::from).collect(),
        }
    }
}

// Wrapper for TxResponse with serde support
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TxResponse {
    /// Block height
    pub height: i64,
    /// Transaction hash
    pub txhash: String,
    /// Response code  
    pub code: u32,
    /// Response data
    pub data: String,
    /// Raw log
    pub raw_log: String,
    /// Parsed logs
    pub logs: Vec<ABCIMessageLog>,
    /// Additional info
    pub info: String,
    /// Gas wanted
    pub gas_wanted: i64,
    /// Gas used
    pub gas_used: i64,
    /// Transaction (using serde_json::Value to avoid Any issues)
    pub tx: Option<serde_json::Value>,
    /// Timestamp
    pub timestamp: String,
    /// Events
    pub events: Vec<Event>,
}

impl From<ProtoTxResponse> for TxResponse {
    fn from(proto: ProtoTxResponse) -> Self {
        Self {
            height: proto.height,
            txhash: proto.txhash,
            code: proto.code,
            data: proto.data,
            raw_log: proto.raw_log,
            logs: proto.logs.into_iter().map(Into::into).collect(),
            info: proto.info,
            gas_wanted: proto.gas_wanted,
            gas_used: proto.gas_used,
            tx: None, // Proto tx contains Any which doesn't serialize well
            timestamp: proto.timestamp,
            events: proto.events.into_iter().map(Into::into).collect(),
        }
    }
}

/// Bank service for balance queries and transfers
pub mod bank {
    use super::*;

    /// Query balance request
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryBalanceRequest {
        /// Account address
        pub address: String,
        /// Coin denomination
        pub denom: String,
    }

    /// Query balance response
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryBalanceResponse {
        /// Balance amount
        pub balance: Option<Coin>,
    }

    /// Query all balances request
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryAllBalancesRequest {
        /// Account address
        pub address: String,
        /// Pagination parameters
        pub pagination: Option<PageRequest>,
    }

    /// Query all balances response
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryAllBalancesResponse {
        /// All balances
        pub balances: Vec<Coin>,
        /// Pagination response
        pub pagination: Option<PageResponse>,
    }

    /// Query total supply request
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryTotalSupplyRequest {
        /// Pagination parameters
        pub pagination: Option<PageRequest>,
    }

    /// Query total supply response
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryTotalSupplyResponse {
        /// Total supply by denomination
        pub supply: Vec<Coin>,
        /// Pagination response
        pub pagination: Option<PageResponse>,
    }

    /// Query supply of a specific denomination
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QuerySupplyOfRequest {
        /// Coin denomination
        pub denom: String,
    }

    /// Query supply of response
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QuerySupplyOfResponse {
        /// Supply amount
        pub amount: Option<Coin>,
    }

    /// Bank query service trait
    #[tonic::async_trait]
    pub trait Query {
        /// Query balance of a single coin
        async fn balance(
            &self,
            request: Request<QueryBalanceRequest>,
        ) -> Result<Response<QueryBalanceResponse>, Status>;

        /// Query all balances of an account
        async fn all_balances(
            &self,
            request: Request<QueryAllBalancesRequest>,
        ) -> Result<Response<QueryAllBalancesResponse>, Status>;

        /// Query total supply of all coins
        async fn total_supply(
            &self,
            request: Request<QueryTotalSupplyRequest>,
        ) -> Result<Response<QueryTotalSupplyResponse>, Status>;

        /// Query supply of a specific coin
        async fn supply_of(
            &self,
            request: Request<QuerySupplyOfRequest>,
        ) -> Result<Response<QuerySupplyOfResponse>, Status>;
    }
}

/// Auth service for account queries
pub mod auth {
    use super::*;

    /// Query account request
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryAccountRequest {
        /// Account address
        pub address: String,
    }

    /// Query account response
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryAccountResponse {
        /// Account information
        pub account: Option<BaseAccount>,
    }

    /// Query accounts request
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryAccountsRequest {
        /// Pagination parameters
        pub pagination: Option<PageRequest>,
    }

    /// Query accounts response
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryAccountsResponse {
        /// List of accounts
        pub accounts: Vec<BaseAccount>,
        /// Pagination response
        pub pagination: Option<PageResponse>,
    }

    /// Query account params
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryParamsRequest {}

    /// Query params response
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct QueryParamsResponse {
        /// Auth module parameters
        pub params: AuthParams,
    }

    /// Auth query service trait
    #[tonic::async_trait]
    pub trait Query {
        /// Query a specific account
        async fn account(
            &self,
            request: Request<QueryAccountRequest>,
        ) -> Result<Response<QueryAccountResponse>, Status>;

        /// Query all accounts
        async fn accounts(
            &self,
            request: Request<QueryAccountsRequest>,
        ) -> Result<Response<QueryAccountsResponse>, Status>;

        /// Query auth module parameters
        async fn params(
            &self,
            request: Request<QueryParamsRequest>,
        ) -> Result<Response<QueryParamsResponse>, Status>;
    }
}

/// Tx service for transaction operations
pub mod tx {
    use super::*;

    /// Simulate transaction request
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct SimulateRequest {
        /// Transaction to simulate
        #[serde(with = "base64_bytes")]
        pub tx_bytes: Vec<u8>,
    }

    /// Simulate transaction response
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct SimulateResponse {
        /// Gas information
        pub gas_info: Option<GasInfo>,
        /// Execution result
        pub result: Option<Result_>,
    }

    /// Get transaction request
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct GetTxRequest {
        /// Transaction hash
        pub hash: String,
    }

    /// Get transaction response
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct GetTxResponse {
        /// Transaction with metadata
        pub tx: Option<Tx>,
        /// Transaction response/result
        pub tx_response: Option<TxResponse>,
    }

    /// Broadcast transaction request
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct BroadcastTxRequest {
        /// Raw transaction bytes
        #[serde(with = "base64_bytes")]
        pub tx_bytes: Vec<u8>,
        /// Broadcast mode
        pub mode: BroadcastMode,
    }

    /// Broadcast transaction response
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct BroadcastTxResponse {
        /// Transaction response
        pub tx_response: Option<TxResponse>,
    }

    /// Get transactions by event request
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct GetTxsEventRequest {
        /// Event query string
        pub events: Vec<String>,
        /// Pagination parameters
        pub pagination: Option<PageRequest>,
        /// Order by
        pub order_by: OrderBy,
    }

    /// Get transactions by event response
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub struct GetTxsEventResponse {
        /// List of transactions
        pub txs: Vec<Tx>,
        /// Transaction responses
        pub tx_responses: Vec<TxResponse>,
        /// Pagination response
        pub pagination: Option<PageResponse>,
    }

    /// Transaction service trait
    #[tonic::async_trait]
    pub trait Service {
        /// Simulate a transaction
        async fn simulate(
            &self,
            request: Request<SimulateRequest>,
        ) -> Result<Response<SimulateResponse>, Status>;

        /// Get a transaction by hash
        async fn get_tx(
            &self,
            request: Request<GetTxRequest>,
        ) -> Result<Response<GetTxResponse>, Status>;

        /// Broadcast a transaction
        async fn broadcast_tx(
            &self,
            request: Request<BroadcastTxRequest>,
        ) -> Result<Response<BroadcastTxResponse>, Status>;

        /// Get transactions by event
        async fn get_txs_event(
            &self,
            request: Request<GetTxsEventRequest>,
        ) -> Result<Response<GetTxsEventResponse>, Status>;
    }
}

// Common types used across services

/// Coin represents a single coin amount
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Coin {
    /// Denomination
    pub denom: String,
    /// Amount
    pub amount: String,
}

/// Base account type
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BaseAccount {
    /// Account address
    pub address: String,
    /// Public key
    pub pub_key: Option<Any>,
    /// Account number
    pub account_number: u64,
    /// Sequence number
    pub sequence: u64,
}

/// Auth module parameters
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AuthParams {
    /// Maximum memo characters
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

/// Transaction
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Tx {
    /// Transaction body
    pub body: Option<TxBody>,
    /// Authentication info
    pub auth_info: Option<AuthInfo>,
    /// Signatures
    #[serde(with = "base64_bytes_vec")]
    pub signatures: Vec<Vec<u8>>,
}

/// Transaction body
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct TxBody {
    /// Messages
    pub messages: Vec<Any>,
    /// Memo
    pub memo: String,
    /// Timeout height
    pub timeout_height: u64,
    /// Extension options
    pub extension_options: Vec<Any>,
    /// Non-critical extension options
    pub non_critical_extension_options: Vec<Any>,
}

/// Authentication info
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AuthInfo {
    /// Signer infos
    pub signer_infos: Vec<SignerInfo>,
    /// Fee
    pub fee: Option<Fee>,
}

/// Signer information
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SignerInfo {
    /// Public key
    pub public_key: Option<Any>,
    /// Mode info
    pub mode_info: Option<ModeInfo>,
    /// Sequence
    pub sequence: u64,
}

/// Signing mode info
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModeInfo {
    /// Single signature
    pub single: Option<ModeInfoSingle>,
    /// Multi-signature
    pub multi: Option<ModeInfoMulti>,
}

/// Single signature mode
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModeInfoSingle {
    /// Signing mode
    pub mode: SignMode,
}

/// Multi-signature mode
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ModeInfoMulti {
    /// Bitarray
    pub bitarray: Option<CompactBitArray>,
    /// Mode infos for each signer
    pub mode_infos: Vec<ModeInfo>,
}

/// Compact bit array for multi-sig
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct CompactBitArray {
    /// Extra bits stored
    pub extra_bits_stored: u32,
    /// Elements
    #[serde(with = "base64_bytes")]
    pub elems: Vec<u8>,
}

/// Transaction fee
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Fee {
    /// Fee amounts
    pub amount: Vec<Coin>,
    /// Gas limit
    pub gas_limit: u64,
    /// Payer address
    pub payer: String,
    /// Granter address
    pub granter: String,
}

/// Broadcast mode
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum BroadcastMode {
    /// Wait for CheckTx
    #[serde(rename = "BROADCAST_MODE_SYNC")]
    Sync,
    /// Don't wait
    #[serde(rename = "BROADCAST_MODE_ASYNC")]
    Async,
    /// Wait for delivery
    #[serde(rename = "BROADCAST_MODE_COMMIT")]
    Commit,
}

/// Query ordering
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum OrderBy {
    /// Ascending order
    #[serde(rename = "ORDER_BY_ASC")]
    Asc,
    /// Descending order
    #[serde(rename = "ORDER_BY_DESC")]
    Desc,
}

/// Signing mode
#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum SignMode {
    /// Direct signing
    #[serde(rename = "SIGN_MODE_DIRECT")]
    Direct,
    /// Textual signing
    #[serde(rename = "SIGN_MODE_TEXTUAL")]
    Textual,
    /// Legacy amino JSON
    #[serde(rename = "SIGN_MODE_LEGACY_AMINO_JSON")]
    LegacyAminoJson,
}

/// Generic any type for protobuf compatibility
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Any {
    /// Type URL
    pub type_url: String,
    /// Value bytes
    #[serde(with = "base64_bytes")]
    pub value: Vec<u8>,
}

/// Pagination request
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PageRequest {
    /// Key for next page
    #[serde(with = "base64_bytes")]
    pub key: Vec<u8>,
    /// Offset
    pub offset: u64,
    /// Limit
    pub limit: u64,
    /// Count total
    pub count_total: bool,
    /// Reverse order
    pub reverse: bool,
}

/// Pagination response
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct PageResponse {
    /// Next key
    #[serde(with = "base64_bytes")]
    pub next_key: Vec<u8>,
    /// Total count
    pub total: u64,
}

/// Helper module for base64 encoding/decoding
mod base64_bytes {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &Vec<u8>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        STANDARD.encode(bytes).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        STANDARD.decode(&s).map_err(serde::de::Error::custom)
    }
}

/// Helper module for base64 encoding/decoding of Vec<Vec<u8>>
mod base64_bytes_vec {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes_vec: &[Vec<u8>], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let encoded: Vec<String> = bytes_vec
            .iter()
            .map(|bytes| STANDARD.encode(bytes))
            .collect();
        encoded.serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<Vec<u8>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let strings = Vec::<String>::deserialize(deserializer)?;
        strings
            .into_iter()
            .map(|s| STANDARD.decode(&s).map_err(serde::de::Error::custom))
            .collect()
    }
}

/// gRPC server builder
pub struct GrpcServerBuilder {
    bank_query: Option<Box<dyn bank::Query + Send + Sync + 'static>>,
    auth_query: Option<Box<dyn auth::Query + Send + Sync + 'static>>,
    tx_service: Option<Box<dyn tx::Service + Send + Sync + 'static>>,
}

impl GrpcServerBuilder {
    /// Create a new gRPC server builder
    pub fn new() -> Self {
        Self {
            bank_query: None,
            auth_query: None,
            tx_service: None,
        }
    }

    /// Set the bank query service
    pub fn with_bank_query(mut self, service: impl bank::Query + Send + Sync + 'static) -> Self {
        self.bank_query = Some(Box::new(service));
        self
    }

    /// Set the auth query service
    pub fn with_auth_query(mut self, service: impl auth::Query + Send + Sync + 'static) -> Self {
        self.auth_query = Some(Box::new(service));
        self
    }

    /// Set the tx service
    pub fn with_tx_service(mut self, service: impl tx::Service + Send + Sync + 'static) -> Self {
        self.tx_service = Some(Box::new(service));
        self
    }

    /// Build the gRPC server
    pub fn build(self) -> Result<tonic::transport::Server, String> {
        let server = tonic::transport::Server::builder();

        // Note: In a real implementation, we would add the services to the server here
        // using generated proto code. For now, this is a placeholder.

        Ok(server)
    }
}

impl Default for GrpcServerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coin_serialization() {
        let coin = Coin {
            denom: "stake".to_string(),
            amount: "1000".to_string(),
        };

        let json = serde_json::to_string(&coin).unwrap();
        assert!(json.contains("stake"));
        assert!(json.contains("1000"));

        let decoded: Coin = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.denom, "stake");
        assert_eq!(decoded.amount, "1000");
    }

    #[test]
    fn test_base_account_serialization() {
        let account = BaseAccount {
            address: "cosmos1abcd...".to_string(),
            pub_key: None,
            account_number: 123,
            sequence: 45,
        };

        let json = serde_json::to_string(&account).unwrap();
        let decoded: BaseAccount = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded.address, account.address);
        assert_eq!(decoded.account_number, account.account_number);
        assert_eq!(decoded.sequence, account.sequence);
    }

    #[test]
    fn test_broadcast_mode() {
        let mode = BroadcastMode::Sync;
        let json = serde_json::to_string(&mode).unwrap();
        assert_eq!(json, "\"BROADCAST_MODE_SYNC\"");

        let decoded: BroadcastMode = serde_json::from_str(&json).unwrap();
        matches!(decoded, BroadcastMode::Sync);
    }
}
