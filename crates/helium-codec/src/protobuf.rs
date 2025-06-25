//! Protobuf encoding/decoding for Cosmos SDK messages
//!
//! This module implements protobuf support compatible with Cosmos SDK,
//! including Any type support and message type registry.

use prost::{DecodeError, EncodeError, Message};
use std::collections::HashMap;
use std::fmt;
use std::sync::{Arc, RwLock};
use thiserror::Error;

/// Protobuf encoding/decoding errors
#[derive(Error, Debug)]
pub enum ProtobufError {
    /// Encoding failed
    #[error("failed to encode protobuf: {0}")]
    EncodeError(#[from] EncodeError),

    /// Decoding failed
    #[error("failed to decode protobuf: {0}")]
    DecodeError(#[from] DecodeError),

    /// Type URL not found in registry
    #[error("type URL not found: {0}")]
    TypeNotFound(String),

    /// Invalid type URL format
    #[error("invalid type URL format: {0}")]
    InvalidTypeUrl(String),

    /// Message type mismatch
    #[error("message type mismatch: expected {expected}, got {actual}")]
    TypeMismatch { expected: String, actual: String },
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

/// Message type registry for dynamic message dispatch
pub struct TypeRegistry {
    /// Map from type URL to message descriptor
    types: Arc<RwLock<HashMap<String, Box<dyn MessageDescriptor>>>>,
}

impl TypeRegistry {
    /// Create a new empty type registry
    pub fn new() -> Self {
        Self {
            types: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a message type
    pub fn register<M>(&self)
    where
        M: Message + Default + MessageExt + Clone + fmt::Debug + Send + Sync + 'static,
    {
        let descriptor = Box::new(ConcreteMessageDescriptor::<M>::new());
        let mut types = self.types.write().unwrap();
        types.insert(M::TYPE_URL.to_string(), descriptor);
    }

    /// Register multiple message types at once using a macro
    /// Example: register_types!(registry, [MsgSend, MsgDelegate, MsgVote]);
    pub fn register_many(&self, registrations: Vec<Box<dyn Fn(&TypeRegistry)>>) {
        for register_fn in registrations {
            register_fn(self);
        }
    }

    /// Check if a type URL is registered
    pub fn contains(&self, type_url: &str) -> bool {
        self.types.read().unwrap().contains_key(type_url)
    }

    /// Decode an Any message using the registry
    pub fn decode_any(&self, any: &Any) -> Result<Box<dyn MessageDyn>> {
        let types = self.types.read().unwrap();
        let descriptor = types
            .get(&any.type_url)
            .ok_or_else(|| ProtobufError::TypeNotFound(any.type_url.clone()))?;

        descriptor.decode(&any.value)
    }

    /// Get all registered type URLs
    pub fn type_urls(&self) -> Vec<String> {
        self.types.read().unwrap().keys().cloned().collect()
    }

    /// Create a default instance of a message by type URL
    pub fn create_default(&self, type_url: &str) -> Result<Box<dyn MessageDyn>> {
        let types = self.types.read().unwrap();
        let descriptor = types
            .get(type_url)
            .ok_or_else(|| ProtobufError::TypeNotFound(type_url.to_string()))?;

        descriptor.create_default()
    }
}

impl Default for TypeRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Trait for dynamic message operations
pub trait MessageDyn: Message + fmt::Debug + Send + Sync {
    /// Get the type URL for this message
    fn type_url(&self) -> &str;

    /// Clone the message as a boxed trait object
    fn clone_box(&self) -> Box<dyn MessageDyn>;

    /// Encode the message to Any
    fn to_any(&self) -> Result<Any>;
}

/// Message descriptor for type registry
trait MessageDescriptor: Send + Sync {
    /// Decode a message from bytes
    fn decode(&self, data: &[u8]) -> Result<Box<dyn MessageDyn>>;

    /// Get the type URL for this message type
    fn type_url(&self) -> &str;

    /// Create a default instance of this message type
    fn create_default(&self) -> Result<Box<dyn MessageDyn>>;
}

/// Concrete implementation of MessageDescriptor
struct ConcreteMessageDescriptor<M> {
    _phantom: std::marker::PhantomData<M>,
}

impl<M> ConcreteMessageDescriptor<M> {
    fn new() -> Self {
        Self {
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<M> MessageDescriptor for ConcreteMessageDescriptor<M>
where
    M: Message + Default + MessageExt + Clone + fmt::Debug + Send + Sync + 'static,
{
    fn decode(&self, data: &[u8]) -> Result<Box<dyn MessageDyn>> {
        let msg = M::decode(data)?;
        Ok(Box::new(msg))
    }

    fn type_url(&self) -> &str {
        M::TYPE_URL
    }

    fn create_default(&self) -> Result<Box<dyn MessageDyn>> {
        Ok(Box::new(M::default()))
    }
}

impl<M> MessageDyn for M
where
    M: Message + MessageExt + Clone + fmt::Debug + Send + Sync + 'static,
{
    fn type_url(&self) -> &str {
        <M as MessageExt>::type_url(self)
    }

    fn clone_box(&self) -> Box<dyn MessageDyn> {
        Box::new(self.clone())
    }

    fn to_any(&self) -> Result<Any> {
        Any::pack(self)
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

/// Create a global type registry with standard Cosmos SDK types
pub fn create_cosmos_type_registry() -> TypeRegistry {
    let registry = TypeRegistry::new();

    // Register standard Cosmos SDK message types
    // Note: In a real implementation, these types would be generated from .proto files
    // For now, we'll register the test types and placeholders

    // Bank module types
    // registry.register::<MsgSend>();
    // registry.register::<MsgMultiSend>();

    // Auth module types
    // registry.register::<BaseAccount>();
    // registry.register::<ModuleAccount>();

    // Staking module types
    // registry.register::<MsgDelegate>();
    // registry.register::<MsgUndelegate>();
    // registry.register::<MsgBeginRedelegate>();

    // Gov module types
    // registry.register::<MsgSubmitProposal>();
    // registry.register::<MsgVote>();
    // registry.register::<MsgDeposit>();

    registry
}

lazy_static::lazy_static! {
    /// Global type registry instance (lazy-initialized)
    pub static ref GLOBAL_TYPE_REGISTRY: Arc<TypeRegistry> = Arc::new(create_cosmos_type_registry());
}

/// Protobuf codec with Cosmos SDK extensions
pub struct CosmosProtoCodec {
    registry: Arc<TypeRegistry>,
}

impl CosmosProtoCodec {
    /// Create a new codec with the global type registry
    pub fn new() -> Self {
        Self {
            registry: GLOBAL_TYPE_REGISTRY.clone(),
        }
    }

    /// Create a codec with a custom type registry
    pub fn with_registry(registry: Arc<TypeRegistry>) -> Self {
        Self { registry }
    }

    /// Encode a message to protobuf bytes
    pub fn encode<M: Message>(&self, msg: &M) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        msg.encode(&mut buf)?;
        Ok(buf)
    }

    /// Decode protobuf bytes to a message
    pub fn decode<M: Message + Default>(&self, data: &[u8]) -> Result<M> {
        M::decode(data).map_err(Into::into)
    }

    /// Encode a message to Any
    pub fn encode_any<M: Message + MessageExt>(&self, msg: &M) -> Result<Any> {
        Any::pack(msg)
    }

    /// Decode an Any to a specific message type
    pub fn decode_any<M: Message + Default + MessageExt>(&self, any: &Any) -> Result<M> {
        any.unpack()
    }

    /// Decode an Any dynamically using the type registry
    pub fn decode_any_dynamic(&self, any: &Any) -> Result<Box<dyn MessageDyn>> {
        self.registry.decode_any(any)
    }

    /// Check if a type is registered
    pub fn has_type(&self, type_url: &str) -> bool {
        self.registry.contains(type_url)
    }
}

impl Default for CosmosProtoCodec {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper macro to register multiple types at once
#[macro_export]
macro_rules! register_types {
    ($registry:expr, [$($ty:ty),+ $(,)?]) => {
        $(
            $registry.register::<$ty>();
        )+
    };
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

    #[test]
    fn test_type_registry() {
        let registry = TypeRegistry::new();

        // Register test message
        registry.register::<TestMessage>();

        // Check registration
        assert!(registry.contains("/test.TestMessage"));
        assert!(!registry.contains("/unknown.Type"));

        // Test decoding via registry
        let msg = TestMessage {
            content: "registry test".to_string(),
            value: 999,
        };

        let any = Any::pack(&msg).unwrap();
        let decoded = registry.decode_any(&any).unwrap();

        assert_eq!(decoded.type_url(), "/test.TestMessage");
    }

    #[test]
    fn test_create_default() {
        let registry = TypeRegistry::new();
        registry.register::<TestMessage>();

        let default_msg = registry.create_default("/test.TestMessage").unwrap();
        assert_eq!(default_msg.type_url(), "/test.TestMessage");
    }

    #[test]
    fn test_cosmos_proto_codec() {
        let codec = CosmosProtoCodec::new();

        let msg = TestMessage {
            content: "codec test".to_string(),
            value: 42,
        };

        // Test encoding/decoding
        let encoded = codec.encode(&msg).unwrap();
        let decoded: TestMessage = codec.decode(&encoded).unwrap();
        assert_eq!(decoded, msg);

        // Test Any encoding/decoding
        let any = codec.encode_any(&msg).unwrap();
        let decoded_any: TestMessage = codec.decode_any(&any).unwrap();
        assert_eq!(decoded_any, msg);
    }

    #[test]
    fn test_register_types_macro() {
        let registry = TypeRegistry::new();

        // Test the macro
        register_types!(registry, [TestMessage]);

        assert!(registry.contains("/test.TestMessage"));

        // Test getting all type URLs
        let urls = registry.type_urls();
        assert!(urls.contains(&"/test.TestMessage".to_string()));
    }

    #[test]
    fn test_type_registry_unknown_type() {
        let registry = TypeRegistry::new();

        let any = Any {
            type_url: "/unknown.Type".to_string(),
            value: vec![1, 2, 3],
        };

        let result = registry.decode_any(&any);
        assert!(matches!(result, Err(ProtobufError::TypeNotFound(_))));
    }
}
