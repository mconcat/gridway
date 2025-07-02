//! Key representations using static enum dispatch

use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::{SigningKey as Ed25519PrivKey, VerifyingKey as Ed25519PubKey};
use helium_types::address::AccAddress;
use k256::ecdsa::{SigningKey as Secp256k1PrivKey, VerifyingKey as Secp256k1PubKey};
use serde::{Deserialize, Serialize};

/// All supported public key types
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PublicKey {
    Secp256k1(Secp256k1PubKey),
    Ed25519(Ed25519PubKey),
    // TODO: Add Multisig variant
}

/// All supported private key types
#[derive(Clone, Debug)]
pub enum PrivateKey {
    Secp256k1(Secp256k1PrivKey),
    Ed25519(Ed25519PrivKey),
}

impl PublicKey {
    /// Derive address from public key
    pub fn to_address(&self) -> AccAddress {
        let bytes = match self {
            PublicKey::Secp256k1(key) => key.to_encoded_point(true).as_bytes().to_vec(),
            PublicKey::Ed25519(key) => key.as_bytes().to_vec(),
        };
        AccAddress::from_pubkey(&bytes)
    }

    /// Get the Protobuf type URL for this key type
    pub fn type_url(&self) -> &'static str {
        match self {
            PublicKey::Secp256k1(_) => "/cosmos.crypto.secp256k1.PubKey",
            PublicKey::Ed25519(_) => "/cosmos.crypto.ed25519.PubKey",
        }
    }

    /// Convert to raw bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        match self {
            PublicKey::Secp256k1(key) => key.to_encoded_point(true).as_bytes().to_vec(),
            PublicKey::Ed25519(key) => key.as_bytes().to_vec(),
        }
    }

    /// Create from Protobuf Any
    pub fn from_any(type_url: &str, value: &[u8]) -> Result<Self, String> {
        match type_url {
            "/cosmos.crypto.secp256k1.PubKey" => {
                // Parse the protobuf wrapper and extract the key bytes
                // For now, assume the value is just the key bytes
                let key = Secp256k1PubKey::from_sec1_bytes(value).map_err(|e| e.to_string())?;
                Ok(PublicKey::Secp256k1(key))
            }
            "/cosmos.crypto.ed25519.PubKey" => {
                // For ed25519, the key should be 32 bytes
                if value.len() != 32 {
                    return Err("invalid ed25519 key length".to_string());
                }
                let key = Ed25519PubKey::from_bytes(value.try_into().unwrap())
                    .map_err(|e| e.to_string())?;
                Ok(PublicKey::Ed25519(key))
            }
            _ => Err(format!("unknown public key type: {type_url}")),
        }
    }

    /// Convert to Protobuf Any
    pub fn to_any(&self) -> (String, Vec<u8>) {
        (self.type_url().to_string(), self.to_bytes())
    }
}

impl PrivateKey {
    /// Get the corresponding public key
    pub fn public_key(&self) -> PublicKey {
        match self {
            PrivateKey::Secp256k1(key) => PublicKey::Secp256k1(*key.verifying_key()),
            PrivateKey::Ed25519(key) => PublicKey::Ed25519(key.verifying_key()),
        }
    }
}

// Custom serialization for PublicKey
impl Serialize for PublicKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct PublicKeyData {
            #[serde(rename = "type")]
            key_type: String,
            value: String,
        }

        let data = PublicKeyData {
            key_type: self.type_url().to_string(),
            value: general_purpose::STANDARD.encode(self.to_bytes()),
        };

        data.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct PublicKeyData {
            #[serde(rename = "type")]
            key_type: String,
            value: String,
        }

        let data = PublicKeyData::deserialize(deserializer)?;
        let bytes = general_purpose::STANDARD
            .decode(&data.value)
            .map_err(serde::de::Error::custom)?;

        PublicKey::from_any(&data.key_type, &bytes).map_err(serde::de::Error::custom)
    }
}
