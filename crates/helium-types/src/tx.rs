//! Transaction types and traits

use crate::{address::AccAddress, error::SdkError};
use prost::Message;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Trait defining the contract for all messages that the host can process
pub trait SdkMsg: Send + Sync + 'static {
    /// Get the Protobuf type URL (e.g., "/cosmos.bank.v1beta1.MsgSend")
    fn type_url(&self) -> &'static str;

    /// Perform stateless validation
    fn validate_basic(&self) -> Result<(), SdkError>;

    /// Get the signers required for this message
    fn get_signers(&self) -> Result<Vec<AccAddress>, SdkError>;

    /// Encode the message to protobuf bytes
    fn encode(&self) -> Vec<u8>;

    /// Get a reference to self as Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Trait representing a transaction
pub trait Tx: Send + Sync {
    /// Get all messages in the transaction
    fn get_msgs(&self) -> Vec<&dyn SdkMsg>;

    /// Get memo
    fn get_memo(&self) -> &str;

    /// Get timeout height
    fn get_timeout_height(&self) -> u64;
}

/// Transaction decoding error types
#[derive(Debug, thiserror::Error)]
pub enum TxDecodeError {
    /// Invalid transaction format
    #[error("invalid transaction format: {0}")]
    InvalidFormat(String),

    /// Failed to decode protobuf
    #[error("protobuf decode error: {0}")]
    ProtobufError(String),

    /// Unknown message type
    #[error("unknown message type: {0}")]
    UnknownMessageType(String),

    /// Invalid message data
    #[error("invalid message data: {0}")]
    InvalidMessageData(String),

    /// Missing required fields
    #[error("missing required field: {0}")]
    MissingField(String),
}

/// Transaction body containing messages and metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxBody {
    /// Messages in the transaction
    pub messages: Vec<TxMessage>,
    /// Transaction memo
    pub memo: String,
    /// Timeout height for the transaction
    pub timeout_height: u64,
}

/// Transaction message wrapper for Any type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxMessage {
    /// Type URL for the message
    pub type_url: String,
    /// Encoded message data
    pub value: Vec<u8>,
}

/// Authentication info for a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthInfo {
    /// Signer information
    pub signer_infos: Vec<SignerInfo>,
    /// Fee information
    pub fee: Fee,
}

/// Signer information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignerInfo {
    /// Public key
    pub public_key: Option<TxMessage>, // Any type for public key
    /// Mode info
    pub mode_info: ModeInfo,
    /// Sequence number
    pub sequence: u64,
}

/// Signing mode info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfo {
    /// Single signer mode
    pub single: Option<ModeInfoSingle>,
}

/// Single signer mode info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfoSingle {
    /// Signing mode
    pub mode: u32,
}

/// Fee information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fee {
    /// Amount of fee
    pub amount: Vec<FeeAmount>,
    /// Gas limit
    pub gas_limit: u64,
    /// Payer address
    pub payer: String,
    /// Granter address
    pub granter: String,
}

/// Fee amount
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeAmount {
    /// Denomination
    pub denom: String,
    /// Amount
    pub amount: String,
}

/// Raw transaction structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawTx {
    /// Transaction body
    pub body: TxBody,
    /// Authentication info
    pub auth_info: AuthInfo,
    /// Signatures
    pub signatures: Vec<Vec<u8>>,
}

// Protobuf representations for Cosmos SDK transaction types

/// Protobuf representation of a complete transaction
#[derive(Clone, PartialEq, Message)]
struct TxProto {
    #[prost(message, optional, tag = "1")]
    pub body: Option<TxBodyProto>,
    #[prost(message, optional, tag = "2")]
    pub auth_info: Option<AuthInfoProto>,
    #[prost(bytes = "vec", repeated, tag = "3")]
    pub signatures: Vec<Vec<u8>>,
}

/// Protobuf representation of transaction body
#[derive(Clone, PartialEq, Message)]
pub struct TxBodyProto {
    #[prost(message, repeated, tag = "1")]
    pub messages: Vec<helium_codec::protobuf::Any>,
    #[prost(string, tag = "2")]
    pub memo: String,
    #[prost(uint64, tag = "3")]
    pub timeout_height: u64,
    #[prost(message, repeated, tag = "1023")]
    pub extension_options: Vec<helium_codec::protobuf::Any>,
    #[prost(message, repeated, tag = "2047")]
    pub non_critical_extension_options: Vec<helium_codec::protobuf::Any>,
}

/// Protobuf representation of auth info
#[derive(Clone, PartialEq, Message)]
pub struct AuthInfoProto {
    #[prost(message, repeated, tag = "1")]
    pub signer_infos: Vec<SignerInfoProto>,
    #[prost(message, optional, tag = "2")]
    pub fee: Option<FeeProto>,
}

/// Protobuf representation of signer info
#[derive(Clone, PartialEq, Message)]
pub struct SignerInfoProto {
    #[prost(message, optional, tag = "1")]
    pub public_key: Option<helium_codec::protobuf::Any>,
    #[prost(message, optional, tag = "2")]
    pub mode_info: Option<ModeInfoProto>,
    #[prost(uint64, tag = "3")]
    pub sequence: u64,
}

/// Protobuf representation of mode info
#[derive(Clone, PartialEq, Message)]
pub struct ModeInfoProto {
    #[prost(oneof = "mode_info_proto::Sum", tags = "1, 2")]
    pub sum: Option<mode_info_proto::Sum>,
}

/// Nested module for mode info variants
mod mode_info_proto {
    use super::*;

    #[derive(Clone, PartialEq, prost::Oneof)]
    pub enum Sum {
        #[prost(message, tag = "1")]
        Single(ModeInfoSingleProto),
        #[prost(message, tag = "2")]
        Multi(ModeInfoMultiProto),
    }
}

/// Protobuf representation of single mode info
#[derive(Clone, PartialEq, Message)]
pub struct ModeInfoSingleProto {
    #[prost(enumeration = "SignMode", tag = "1")]
    pub mode: i32,
}

/// Protobuf representation of multi mode info
#[derive(Clone, PartialEq, Message)]
pub(crate) struct ModeInfoMultiProto {
    #[prost(message, optional, tag = "1")]
    pub bitarray: Option<CompactBitArrayProto>,
    #[prost(message, repeated, tag = "2")]
    pub mode_infos: Vec<ModeInfoProto>,
}

/// Protobuf representation of compact bit array
#[derive(Clone, PartialEq, Message)]
pub(crate) struct CompactBitArrayProto {
    #[prost(uint32, tag = "1")]
    pub extra_bits_stored: u32,
    #[prost(bytes = "vec", tag = "2")]
    pub elems: Vec<u8>,
}

/// Protobuf representation of fee
#[derive(Clone, PartialEq, Message)]
pub struct FeeProto {
    #[prost(message, repeated, tag = "1")]
    pub amount: Vec<CoinProto>,
    #[prost(uint64, tag = "2")]
    pub gas_limit: u64,
    #[prost(string, tag = "3")]
    pub payer: String,
    #[prost(string, tag = "4")]
    pub granter: String,
}

/// Protobuf representation of coin
#[derive(Clone, PartialEq, Message)]
pub struct CoinProto {
    #[prost(string, tag = "1")]
    pub denom: String,
    #[prost(string, tag = "2")]
    pub amount: String,
}

/// Sign mode enumeration
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
#[repr(i32)]
pub enum SignMode {
    Unspecified = 0,
    Direct = 1,
    Textual = 2,
    LegacyAminoJson = 127,
}

// Conversions between internal types and protobuf types

impl From<&TxBody> for TxBodyProto {
    fn from(body: &TxBody) -> Self {
        Self {
            messages: body
                .messages
                .iter()
                .map(|msg| helium_codec::protobuf::Any {
                    type_url: msg.type_url.clone(),
                    value: msg.value.clone(),
                })
                .collect(),
            memo: body.memo.clone(),
            timeout_height: body.timeout_height,
            extension_options: vec![],
            non_critical_extension_options: vec![],
        }
    }
}

impl From<&AuthInfo> for AuthInfoProto {
    fn from(auth_info: &AuthInfo) -> Self {
        Self {
            signer_infos: auth_info
                .signer_infos
                .iter()
                .map(|info| SignerInfoProto {
                    public_key: info
                        .public_key
                        .as_ref()
                        .map(|pk| helium_codec::protobuf::Any {
                            type_url: pk.type_url.clone(),
                            value: pk.value.clone(),
                        }),
                    mode_info: info.mode_info.single.as_ref().map(|single| ModeInfoProto {
                        sum: Some(mode_info_proto::Sum::Single(ModeInfoSingleProto {
                            mode: single.mode as i32,
                        })),
                    }),
                    sequence: info.sequence,
                })
                .collect(),
            fee: Some(FeeProto {
                amount: auth_info
                    .fee
                    .amount
                    .iter()
                    .map(|fee_amount| CoinProto {
                        denom: fee_amount.denom.clone(),
                        amount: fee_amount.amount.clone(),
                    })
                    .collect(),
                gas_limit: auth_info.fee.gas_limit,
                payer: auth_info.fee.payer.clone(),
                granter: auth_info.fee.granter.clone(),
            }),
        }
    }
}

/// Type alias for message decoder function
type MessageDecoder = fn(&[u8]) -> Result<Box<dyn SdkMsg>, TxDecodeError>;

/// Transaction decoder for handling protobuf and JSON formats
pub struct TxDecoder {
    /// Message type registry for decoding
    message_registry: HashMap<String, MessageDecoder>,
}

impl TxDecoder {
    /// Create a new transaction decoder
    pub fn new() -> Self {
        Self {
            message_registry: HashMap::new(),
        }
    }

    /// Register a message type decoder
    pub fn register_message_type<T>(&mut self, type_url: &str, decoder: MessageDecoder) {
        self.message_registry.insert(type_url.to_string(), decoder);
    }

    /// Register standard Cosmos SDK message types
    pub fn register_standard_types(&mut self) {
        // Register MsgSend
        self.register_message_type::<()>("/cosmos.bank.v1beta1.MsgSend", |bytes| {
            use crate::msgs::bank::MsgSend;
            use prost::Message;

            let msg = MsgSend::decode(bytes).map_err(|e| {
                TxDecodeError::InvalidMessageData(format!("failed to decode MsgSend: {}", e))
            })?;
            Ok(Box::new(msg))
        });

        // Additional standard message types can be registered here
    }

    /// Decode transaction from bytes (supports both protobuf and JSON for development)
    pub fn decode_tx(&self, tx_bytes: &[u8]) -> Result<RawTx, TxDecodeError> {
        // Try protobuf first (preferred)
        if let Ok(tx) = self.decode_protobuf_tx(tx_bytes) {
            return Ok(tx);
        }

        // Fall back to JSON for development/testing
        self.decode_json_tx(tx_bytes)
    }

    /// Decode transaction from JSON (for development)
    fn decode_json_tx(&self, tx_bytes: &[u8]) -> Result<RawTx, TxDecodeError> {
        let tx_str = std::str::from_utf8(tx_bytes)
            .map_err(|e| TxDecodeError::InvalidFormat(format!("invalid UTF-8: {}", e)))?;

        serde_json::from_str(tx_str)
            .map_err(|e| TxDecodeError::InvalidFormat(format!("JSON decode error: {}", e)))
    }

    /// Decode transaction from protobuf bytes
    fn decode_protobuf_tx(&self, tx_bytes: &[u8]) -> Result<RawTx, TxDecodeError> {
        // Decode the protobuf transaction
        let tx_proto = TxProto::decode(tx_bytes).map_err(|e| {
            TxDecodeError::ProtobufError(format!("failed to decode transaction: {}", e))
        })?;

        // Extract and convert the body
        let body = if let Some(body_proto) = tx_proto.body {
            TxBody {
                messages: body_proto
                    .messages
                    .into_iter()
                    .map(|any| TxMessage {
                        type_url: any.type_url,
                        value: any.value,
                    })
                    .collect(),
                memo: body_proto.memo,
                timeout_height: body_proto.timeout_height,
            }
        } else {
            return Err(TxDecodeError::MissingField("body".to_string()));
        };

        // Extract and convert the auth info
        let auth_info = if let Some(auth_proto) = tx_proto.auth_info {
            let signer_infos = auth_proto
                .signer_infos
                .into_iter()
                .map(|signer_proto| {
                    SignerInfo {
                        public_key: signer_proto.public_key.map(|any| TxMessage {
                            type_url: any.type_url,
                            value: any.value,
                        }),
                        mode_info: match signer_proto.mode_info {
                            Some(mode_proto) => match mode_proto.sum {
                                Some(mode_info_proto::Sum::Single(single)) => ModeInfo {
                                    single: Some(ModeInfoSingle {
                                        mode: single.mode as u32,
                                    }),
                                },
                                Some(mode_info_proto::Sum::Multi(_)) => ModeInfo {
                                    single: None, // Multi-sig not fully supported in this struct yet
                                },
                                None => ModeInfo { single: None },
                            },
                            None => ModeInfo { single: None },
                        },
                        sequence: signer_proto.sequence,
                    }
                })
                .collect();

            let fee = if let Some(fee_proto) = auth_proto.fee {
                Fee {
                    amount: fee_proto
                        .amount
                        .into_iter()
                        .map(|coin| FeeAmount {
                            denom: coin.denom,
                            amount: coin.amount,
                        })
                        .collect(),
                    gas_limit: fee_proto.gas_limit,
                    payer: fee_proto.payer,
                    granter: fee_proto.granter,
                }
            } else {
                Fee {
                    amount: vec![],
                    gas_limit: 0,
                    payer: String::new(),
                    granter: String::new(),
                }
            };

            AuthInfo { signer_infos, fee }
        } else {
            return Err(TxDecodeError::MissingField("auth_info".to_string()));
        };

        Ok(RawTx {
            body,
            auth_info,
            signatures: tx_proto.signatures,
        })
    }

    /// Extract and decode messages from a transaction
    pub fn extract_messages(&self, tx: &RawTx) -> Result<Vec<Box<dyn SdkMsg>>, TxDecodeError> {
        let mut messages = Vec::new();

        for tx_msg in &tx.body.messages {
            let msg = self.decode_message(tx_msg)?;
            messages.push(msg);
        }

        Ok(messages)
    }

    /// Decode a single message from TxMessage
    fn decode_message(&self, tx_msg: &TxMessage) -> Result<Box<dyn SdkMsg>, TxDecodeError> {
        if let Some(decoder) = self.message_registry.get(&tx_msg.type_url) {
            decoder(&tx_msg.value)
        } else {
            Err(TxDecodeError::UnknownMessageType(tx_msg.type_url.clone()))
        }
    }

    /// Validate transaction structure
    pub fn validate_tx_structure(&self, tx: &RawTx) -> Result<(), TxDecodeError> {
        // Validate required fields
        if tx.body.messages.is_empty() {
            return Err(TxDecodeError::MissingField("messages".to_string()));
        }

        if tx.auth_info.signer_infos.is_empty() {
            return Err(TxDecodeError::MissingField("signer_infos".to_string()));
        }

        if tx.signatures.is_empty() {
            return Err(TxDecodeError::MissingField("signatures".to_string()));
        }

        // Validate signatures match signer infos
        if tx.signatures.len() != tx.auth_info.signer_infos.len() {
            return Err(TxDecodeError::InvalidFormat(
                "signature count mismatch with signer info count".to_string(),
            ));
        }

        // Validate each message type is known
        for msg in &tx.body.messages {
            if !self.message_registry.contains_key(&msg.type_url) {
                return Err(TxDecodeError::UnknownMessageType(msg.type_url.clone()));
            }
        }

        // Validate gas limit is positive
        if tx.auth_info.fee.gas_limit == 0 {
            return Err(TxDecodeError::InvalidFormat(
                "gas limit must be positive".to_string(),
            ));
        }

        Ok(())
    }

    /// Get transaction metadata
    pub fn get_tx_metadata(&self, tx: &RawTx) -> TxMetadata {
        TxMetadata {
            message_count: tx.body.messages.len(),
            signer_count: tx.auth_info.signer_infos.len(),
            signature_count: tx.signatures.len(),
            gas_limit: tx.auth_info.fee.gas_limit,
            fee_amount: tx.auth_info.fee.amount.clone(),
            memo: tx.body.memo.clone(),
            timeout_height: tx.body.timeout_height,
        }
    }

    /// Encode a transaction to protobuf bytes
    pub fn encode_tx(&self, tx: &RawTx) -> Result<Vec<u8>, TxDecodeError> {
        use helium_codec::protobuf::Any;

        // Convert messages to Any types
        let messages: Vec<Any> = tx
            .body
            .messages
            .iter()
            .map(|msg| Any {
                type_url: msg.type_url.clone(),
                value: msg.value.clone(),
            })
            .collect();

        // Create protobuf body
        let body_proto = TxBodyProto {
            messages,
            memo: tx.body.memo.clone(),
            timeout_height: tx.body.timeout_height,
            extension_options: vec![],
            non_critical_extension_options: vec![],
        };

        // Convert signer infos
        let signer_infos: Vec<SignerInfoProto> = tx
            .auth_info
            .signer_infos
            .iter()
            .map(|info| SignerInfoProto {
                public_key: info.public_key.as_ref().map(|pk| Any {
                    type_url: pk.type_url.clone(),
                    value: pk.value.clone(),
                }),
                mode_info: info.mode_info.single.as_ref().map(|single| ModeInfoProto {
                    sum: Some(mode_info_proto::Sum::Single(ModeInfoSingleProto {
                        mode: single.mode as i32,
                    })),
                }),
                sequence: info.sequence,
            })
            .collect();

        // Convert fee
        let fee_proto = FeeProto {
            amount: tx
                .auth_info
                .fee
                .amount
                .iter()
                .map(|coin| CoinProto {
                    denom: coin.denom.clone(),
                    amount: coin.amount.clone(),
                })
                .collect(),
            gas_limit: tx.auth_info.fee.gas_limit,
            payer: tx.auth_info.fee.payer.clone(),
            granter: tx.auth_info.fee.granter.clone(),
        };

        // Create auth info
        let auth_proto = AuthInfoProto {
            signer_infos,
            fee: Some(fee_proto),
        };

        // Create complete transaction
        let tx_proto = TxProto {
            body: Some(body_proto),
            auth_info: Some(auth_proto),
            signatures: tx.signatures.clone(),
        };

        // Encode to bytes
        let mut buf = Vec::new();
        tx_proto.encode(&mut buf).map_err(|e| {
            TxDecodeError::ProtobufError(format!("failed to encode transaction: {}", e))
        })?;

        Ok(buf)
    }

    /// Decode a message from Any type using the registry
    pub fn decode_any_message(
        &self,
        any: &helium_codec::protobuf::Any,
    ) -> Result<Box<dyn SdkMsg>, TxDecodeError> {
        self.decode_message(&TxMessage {
            type_url: any.type_url.clone(),
            value: any.value.clone(),
        })
    }
}

/// Transaction metadata for analysis
#[derive(Debug, Clone)]
pub struct TxMetadata {
    /// Number of messages in transaction
    pub message_count: usize,
    /// Number of signers
    pub signer_count: usize,
    /// Number of signatures
    pub signature_count: usize,
    /// Gas limit
    pub gas_limit: u64,
    /// Fee amounts
    pub fee_amount: Vec<FeeAmount>,
    /// Transaction memo
    pub memo: String,
    /// Timeout height
    pub timeout_height: u64,
}

impl Default for TxDecoder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::AccAddress;

    // Mock message for testing
    #[derive(Debug)]
    struct MockMessage {
        type_url: &'static str,
        from: AccAddress,
        to: AccAddress,
        amount: String,
    }

    impl SdkMsg for MockMessage {
        fn type_url(&self) -> &'static str {
            self.type_url
        }

        fn validate_basic(&self) -> Result<(), SdkError> {
            if self.amount == "0" {
                return Err(SdkError::InvalidRequest(
                    "amount cannot be zero".to_string(),
                ));
            }
            Ok(())
        }

        fn get_signers(&self) -> Result<Vec<AccAddress>, SdkError> {
            Ok(vec![self.from])
        }

        fn encode(&self) -> Vec<u8> {
            format!("{}:{}:{}", self.from, self.to, self.amount).into_bytes()
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn mock_msg_decoder(data: &[u8]) -> Result<Box<dyn SdkMsg>, TxDecodeError> {
        let s = std::str::from_utf8(data)
            .map_err(|e| TxDecodeError::InvalidMessageData(e.to_string()))?;
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 3 {
            return Err(TxDecodeError::InvalidMessageData(
                "expected 3 parts".to_string(),
            ));
        }

        // For testing, create addresses from dummy public keys
        let from = AccAddress::from_pubkey(b"from_pubkey_bytes_01");
        let to = AccAddress::from_pubkey(b"to_pubkey_bytes_0001");

        Ok(Box::new(MockMessage {
            type_url: "/cosmos.bank.v1beta1.MsgSend",
            from,
            to,
            amount: parts[2].to_string(),
        }))
    }

    fn create_test_tx() -> RawTx {
        RawTx {
            body: TxBody {
                messages: vec![TxMessage {
                    type_url: "/cosmos.bank.v1beta1.MsgSend".to_string(),
                    value: b"dummy:data:100".to_vec(),
                }],
                memo: "test transaction".to_string(),
                timeout_height: 12345,
            },
            auth_info: AuthInfo {
                signer_infos: vec![SignerInfo {
                    public_key: Some(TxMessage {
                        type_url: "/cosmos.crypto.secp256k1.PubKey".to_string(),
                        value: vec![1, 2, 3, 4],
                    }),
                    mode_info: ModeInfo {
                        single: Some(ModeInfoSingle { mode: 1 }),
                    },
                    sequence: 0,
                }],
                fee: Fee {
                    amount: vec![FeeAmount {
                        denom: "uatom".to_string(),
                        amount: "1000".to_string(),
                    }],
                    gas_limit: 200000,
                    payer: "".to_string(),
                    granter: "".to_string(),
                },
            },
            signatures: vec![vec![1, 2, 3, 4, 5]],
        }
    }

    #[test]
    fn test_tx_decoder_creation() {
        let decoder = TxDecoder::new();
        assert_eq!(decoder.message_registry.len(), 0);

        let default_decoder = TxDecoder::default();
        assert_eq!(default_decoder.message_registry.len(), 0);
    }

    #[test]
    fn test_register_message_type() {
        let mut decoder = TxDecoder::new();
        decoder
            .register_message_type::<MockMessage>("/cosmos.bank.v1beta1.MsgSend", mock_msg_decoder);
        assert_eq!(decoder.message_registry.len(), 1);
        assert!(decoder
            .message_registry
            .contains_key("/cosmos.bank.v1beta1.MsgSend"));
    }

    #[test]
    fn test_decode_json_tx() {
        let tx = create_test_tx();
        let json_data = serde_json::to_vec(&tx).unwrap();

        let decoder = TxDecoder::new();
        let decoded_tx = decoder.decode_tx(&json_data).unwrap();

        assert_eq!(decoded_tx.body.messages.len(), 1);
        assert_eq!(decoded_tx.body.memo, "test transaction");
        assert_eq!(decoded_tx.body.timeout_height, 12345);
        assert_eq!(decoded_tx.auth_info.fee.gas_limit, 200000);
        assert_eq!(decoded_tx.signatures.len(), 1);
    }

    #[test]
    fn test_decode_invalid_json() {
        let decoder = TxDecoder::new();
        let invalid_json = b"{ invalid json";

        let result = decoder.decode_tx(invalid_json);
        assert!(result.is_err());
        match result {
            Err(TxDecodeError::ProtobufError(_)) => {} // Falls back to protobuf which isn't implemented
            Err(TxDecodeError::InvalidFormat(_)) => {} // Now also acceptable since JSON decoding might fail
            _ => panic!("Expected ProtobufError or InvalidFormat, got: {:?}", result),
        }
    }

    #[test]
    fn test_extract_messages() {
        let tx = create_test_tx();
        let mut decoder = TxDecoder::new();
        decoder
            .register_message_type::<MockMessage>("/cosmos.bank.v1beta1.MsgSend", mock_msg_decoder);

        let messages = decoder.extract_messages(&tx).unwrap();
        assert_eq!(messages.len(), 1);

        let msg = messages[0].as_any().downcast_ref::<MockMessage>().unwrap();
        assert_eq!(msg.type_url(), "/cosmos.bank.v1beta1.MsgSend");
        assert_eq!(msg.amount, "100");
    }

    #[test]
    fn test_extract_messages_unknown_type() {
        let tx = create_test_tx();
        let decoder = TxDecoder::new(); // No registered types

        let result = decoder.extract_messages(&tx);
        assert!(result.is_err());
        match result {
            Err(TxDecodeError::UnknownMessageType(type_url)) => {
                assert_eq!(type_url, "/cosmos.bank.v1beta1.MsgSend");
            }
            _ => panic!("Expected UnknownMessageType error"),
        }
    }

    #[test]
    fn test_validate_tx_structure_valid() {
        let tx = create_test_tx();
        let mut decoder = TxDecoder::new();
        decoder
            .register_message_type::<MockMessage>("/cosmos.bank.v1beta1.MsgSend", mock_msg_decoder);

        let result = decoder.validate_tx_structure(&tx);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_tx_structure_missing_messages() {
        let mut tx = create_test_tx();
        tx.body.messages.clear();

        let decoder = TxDecoder::new();
        let result = decoder.validate_tx_structure(&tx);
        assert!(result.is_err());
        match result {
            Err(TxDecodeError::MissingField(field)) => {
                assert_eq!(field, "messages");
            }
            _ => panic!("Expected MissingField error"),
        }
    }

    #[test]
    fn test_validate_tx_structure_missing_signers() {
        let mut tx = create_test_tx();
        tx.auth_info.signer_infos.clear();

        let decoder = TxDecoder::new();
        let result = decoder.validate_tx_structure(&tx);
        assert!(result.is_err());
        match result {
            Err(TxDecodeError::MissingField(field)) => {
                assert_eq!(field, "signer_infos");
            }
            _ => panic!("Expected MissingField error"),
        }
    }

    #[test]
    fn test_validate_tx_structure_missing_signatures() {
        let mut tx = create_test_tx();
        tx.signatures.clear();

        let decoder = TxDecoder::new();
        let result = decoder.validate_tx_structure(&tx);
        assert!(result.is_err());
        match result {
            Err(TxDecodeError::MissingField(field)) => {
                assert_eq!(field, "signatures");
            }
            _ => panic!("Expected MissingField error"),
        }
    }

    #[test]
    fn test_validate_tx_structure_signature_mismatch() {
        let mut tx = create_test_tx();
        tx.signatures.push(vec![6, 7, 8]); // Add extra signature

        let decoder = TxDecoder::new();
        let result = decoder.validate_tx_structure(&tx);
        assert!(result.is_err());
        match result {
            Err(TxDecodeError::InvalidFormat(msg)) => {
                assert!(msg.contains("signature count mismatch"));
            }
            _ => panic!("Expected InvalidFormat error"),
        }
    }

    #[test]
    fn test_validate_tx_structure_zero_gas() {
        let mut tx = create_test_tx();
        tx.auth_info.fee.gas_limit = 0;

        let mut decoder = TxDecoder::new();
        // Register the message type so it doesn't fail on unknown type first
        decoder
            .register_message_type::<MockMessage>("/cosmos.bank.v1beta1.MsgSend", mock_msg_decoder);

        let result = decoder.validate_tx_structure(&tx);
        assert!(result.is_err());
        match result {
            Err(TxDecodeError::InvalidFormat(msg)) => {
                assert!(msg.contains("gas limit must be positive"));
            }
            _ => panic!("Expected InvalidFormat error"),
        }
    }

    #[test]
    fn test_get_tx_metadata() {
        let tx = create_test_tx();
        let decoder = TxDecoder::new();

        let metadata = decoder.get_tx_metadata(&tx);
        assert_eq!(metadata.message_count, 1);
        assert_eq!(metadata.signer_count, 1);
        assert_eq!(metadata.signature_count, 1);
        assert_eq!(metadata.gas_limit, 200000);
        assert_eq!(metadata.fee_amount.len(), 1);
        assert_eq!(metadata.fee_amount[0].denom, "uatom");
        assert_eq!(metadata.fee_amount[0].amount, "1000");
        assert_eq!(metadata.memo, "test transaction");
        assert_eq!(metadata.timeout_height, 12345);
    }

    #[test]
    fn test_decode_message_invalid_data() {
        let mut decoder = TxDecoder::new();
        decoder
            .register_message_type::<MockMessage>("/cosmos.bank.v1beta1.MsgSend", mock_msg_decoder);

        let tx_msg = TxMessage {
            type_url: "/cosmos.bank.v1beta1.MsgSend".to_string(),
            value: b"invalid:data".to_vec(), // Only 2 parts instead of 3
        };

        let result = decoder.decode_message(&tx_msg);
        assert!(result.is_err());
        match result {
            Err(TxDecodeError::InvalidMessageData(_)) => {}
            _ => panic!("Expected InvalidMessageData error"),
        }
    }

    #[test]
    fn test_protobuf_tx_encoding_decoding() {
        let tx = create_test_tx();
        let decoder = TxDecoder::new();

        // Encode to protobuf
        let encoded = decoder.encode_tx(&tx).unwrap();
        assert!(!encoded.is_empty());

        // Decode from protobuf
        let decoded = decoder.decode_protobuf_tx(&encoded).unwrap();

        // Verify structure matches
        assert_eq!(decoded.body.messages.len(), tx.body.messages.len());
        assert_eq!(decoded.body.memo, tx.body.memo);
        assert_eq!(decoded.body.timeout_height, tx.body.timeout_height);
        assert_eq!(
            decoded.auth_info.signer_infos.len(),
            tx.auth_info.signer_infos.len()
        );
        assert_eq!(decoded.auth_info.fee.gas_limit, tx.auth_info.fee.gas_limit);
        assert_eq!(decoded.signatures.len(), tx.signatures.len());
    }

    #[test]
    fn test_decode_tx_prefers_protobuf() {
        let tx = create_test_tx();
        let decoder = TxDecoder::new();

        // Encode to protobuf
        let protobuf_encoded = decoder.encode_tx(&tx).unwrap();

        // Decode should use protobuf path
        let decoded = decoder.decode_tx(&protobuf_encoded).unwrap();

        // Verify it decoded correctly
        assert_eq!(decoded.body.messages.len(), 1);
        assert_eq!(decoded.body.memo, "test transaction");
    }

    #[test]
    fn test_register_standard_types() {
        let mut decoder = TxDecoder::new();
        decoder.register_standard_types();

        // Check that MsgSend is registered
        assert!(decoder
            .message_registry
            .contains_key("/cosmos.bank.v1beta1.MsgSend"));
    }
}
