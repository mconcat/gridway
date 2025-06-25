//! Encoding and decoding utilities for the helium blockchain framework.
//!
//! This crate provides codec implementations for helium messages and types,
//! supporting both Protobuf and JSON encoding/decoding.

pub mod protobuf;

use prost::Message;
use serde::{Deserialize, Serialize};
use std::error::Error as StdError;

// Re-export commonly used types
pub use protobuf::{Any, MessageExt, ProtobufError, TypeRegistry};

/// Codec trait for encoding and decoding messages
pub trait Codec: Send + Sync {
    /// Encode a message to bytes
    fn encode<T: Message>(&self, msg: &T) -> Result<Vec<u8>, Box<dyn StdError>>;

    /// Decode bytes to a message
    fn decode<T: Message + Default>(&self, data: &[u8]) -> Result<T, Box<dyn StdError>>;

    /// Encode a message to JSON
    fn encode_json<T: Serialize>(&self, msg: &T) -> Result<String, Box<dyn StdError>>;

    /// Decode JSON to a message
    fn decode_json<T: for<'de> Deserialize<'de>>(&self, data: &str)
        -> Result<T, Box<dyn StdError>>;
}

/// Protobuf codec implementation
pub struct ProtoCodec;

impl ProtoCodec {
    /// Create a new protobuf codec
    pub fn new() -> Self {
        Self
    }
}

impl Default for ProtoCodec {
    fn default() -> Self {
        Self::new()
    }
}

impl Codec for ProtoCodec {
    fn encode<T: Message>(&self, msg: &T) -> Result<Vec<u8>, Box<dyn StdError>> {
        let mut buf = Vec::new();
        msg.encode(&mut buf)?;
        Ok(buf)
    }

    fn decode<T: Message + Default>(&self, data: &[u8]) -> Result<T, Box<dyn StdError>> {
        T::decode(data).map_err(|e| e.into())
    }

    fn encode_json<T: Serialize>(&self, msg: &T) -> Result<String, Box<dyn StdError>> {
        serde_json::to_string(msg).map_err(|e| e.into())
    }

    fn decode_json<T: for<'de> Deserialize<'de>>(
        &self,
        data: &str,
    ) -> Result<T, Box<dyn StdError>> {
        serde_json::from_str(data).map_err(|e| e.into())
    }
}

// Remove the old Any implementation - it's now in protobuf.rs
// The Any type is re-exported from the protobuf module above

// TypeRegistry is now provided by the protobuf module

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proto_codec() {
        let codec = ProtoCodec::new();
        let json = codec.encode_json(&vec![1, 2, 3]).unwrap();
        let decoded: Vec<i32> = codec.decode_json(&json).unwrap();
        assert_eq!(decoded, vec![1, 2, 3]);
    }

    // TypeRegistry tests are now in protobuf.rs
}
