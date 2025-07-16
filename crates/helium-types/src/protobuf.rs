//! Protobuf utilities for Cosmos SDK compatibility
//!
//! This module provides the Any type and MessageExt trait that were previously
//! in helium-codec. Since we're using protobuf-only (no Amino), these minimal
//! utilities are all we need.

use prost::Message;
use thiserror::Error;

/// Protobuf encoding/decoding errors
#[derive(Error, Debug)]
pub enum ProtobufError {
    /// Encoding failed
    #[error("failed to encode protobuf: {0}")]
    EncodeError(#[from] prost::EncodeError),

    /// Decoding failed
    #[error("failed to decode protobuf: {0}")]
    DecodeError(#[from] prost::DecodeError),
}

/// Result type for protobuf operations
pub type Result<T> = std::result::Result<T, ProtobufError>;

/// Cosmos SDK Any type implementation
///
/// This matches the protobuf Any type used in Cosmos SDK for
/// polymorphic message encoding.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Any {
    /// Type URL uniquely identifies the type of the serialized message
    ///
    /// Type URLs use the format: /fully.qualified.protobuf.Name
    /// For Cosmos SDK messages, this is typically: /cosmos.bank.v1beta1.MsgSend
    #[prost(string, tag = "1")]
    pub type_url: String,

    /// Binary serialization of the protobuf message
    #[prost(bytes = "vec", tag = "2")]
    pub value: Vec<u8>,
}

impl Any {
    /// Create a new Any from a message and type URL
    pub fn from_msg<M: Message>(msg: &M, type_url: impl Into<String>) -> Result<Self> {
        let mut value = Vec::new();
        msg.encode(&mut value)?;

        Ok(Self {
            type_url: type_url.into(),
            value,
        })
    }

    /// Pack a message into an Any with automatic type URL generation
    ///
    /// The type URL is generated from the message's type name.
    /// For example, a MsgSend would get type URL: /cosmos.bank.v1beta1.MsgSend
    pub fn pack<M: Message + MessageExt>(msg: &M) -> Result<Self> {
        Self::from_msg(msg, msg.type_url())
    }

    /// Unpack an Any into a specific message type
    pub fn unpack<M: Message + Default>(&self) -> Result<M> {
        M::decode(self.value.as_slice()).map_err(ProtobufError::from)
    }

    /// Check if this Any contains a message of the given type
    pub fn is<M: MessageExt>(&self) -> bool {
        self.type_url == M::TYPE_URL
    }

    /// Get the type URL without the leading slash
    pub fn type_url_without_prefix(&self) -> &str {
        self.type_url.strip_prefix('/').unwrap_or(&self.type_url)
    }
}

/// Extension trait for messages with type URL support
pub trait MessageExt: Message {
    /// The type URL for this message type
    const TYPE_URL: &'static str;

    /// Get the type URL for this message
    fn type_url(&self) -> &'static str {
        Self::TYPE_URL
    }
}

/// Standard Cosmos SDK type URL constants
pub mod type_urls {
    /// Bank module type URLs
    pub const MSG_SEND: &str = "/cosmos.bank.v1beta1.MsgSend";
    pub const MSG_MULTI_SEND: &str = "/cosmos.bank.v1beta1.MsgMultiSend";

    /// Auth module type URLs
    pub const BASE_ACCOUNT: &str = "/cosmos.auth.v1beta1.BaseAccount";
    pub const MODULE_ACCOUNT: &str = "/cosmos.auth.v1beta1.ModuleAccount";

    /// Staking module type URLs
    pub const MSG_DELEGATE: &str = "/cosmos.staking.v1beta1.MsgDelegate";
    pub const MSG_UNDELEGATE: &str = "/cosmos.staking.v1beta1.MsgUndelegate";
    pub const MSG_REDELEGATE: &str = "/cosmos.staking.v1beta1.MsgBeginRedelegate";

    /// Gov module type URLs
    pub const MSG_SUBMIT_PROPOSAL: &str = "/cosmos.gov.v1beta1.MsgSubmitProposal";
    pub const MSG_VOTE: &str = "/cosmos.gov.v1beta1.MsgVote";
    pub const MSG_DEPOSIT: &str = "/cosmos.gov.v1beta1.MsgDeposit";
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test message for unit tests
    #[derive(Clone, PartialEq, ::prost::Message)]
    struct TestMessage {
        #[prost(string, tag = "1")]
        pub content: String,
        #[prost(uint64, tag = "2")]
        pub value: u64,
    }

    impl MessageExt for TestMessage {
        const TYPE_URL: &'static str = "/test.TestMessage";
    }

    #[test]
    fn test_any_pack_unpack() {
        let msg = TestMessage {
            content: "hello".to_string(),
            value: 42,
        };

        // Test packing
        let any = Any::pack(&msg).unwrap();
        assert_eq!(any.type_url, "/test.TestMessage");
        assert!(!any.value.is_empty());

        // Test unpacking
        let unpacked: TestMessage = any.unpack().unwrap();
        assert_eq!(unpacked, msg);

        // Test type checking
        assert!(any.is::<TestMessage>());
    }

    #[test]
    fn test_any_from_msg() {
        let msg = TestMessage {
            content: "test".to_string(),
            value: 123,
        };

        let any = Any::from_msg(&msg, "/custom.Type").unwrap();
        assert_eq!(any.type_url, "/custom.Type");

        let unpacked: TestMessage = any.unpack().unwrap();
        assert_eq!(unpacked, msg);
    }

    #[test]
    fn test_type_url_without_prefix() {
        let any = Any {
            type_url: "/cosmos.bank.v1beta1.MsgSend".to_string(),
            value: vec![],
        };

        assert_eq!(any.type_url_without_prefix(), "cosmos.bank.v1beta1.MsgSend");

        let any_no_prefix = Any {
            type_url: "cosmos.bank.v1beta1.MsgSend".to_string(),
            value: vec![],
        };

        assert_eq!(
            any_no_prefix.type_url_without_prefix(),
            "cosmos.bank.v1beta1.MsgSend"
        );
    }
}
