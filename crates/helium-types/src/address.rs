//! Address types for helium

use bech32::{Bech32, Hrp};
use ripemd::Ripemd160;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

/// Account address - 20 bytes
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccAddress([u8; 20]);

impl Default for AccAddress {
    fn default() -> Self {
        Self([0u8; 20])
    }
}

/// Validator operator address - 20 bytes
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ValAddress([u8; 20]);

/// Consensus address - 20 bytes
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConsAddress([u8; 20]);

impl AccAddress {
    /// Create an address from a public key using the standard derivation
    /// ripemd160(sha256(pubkey_bytes))
    pub fn from_pubkey(pubkey_bytes: &[u8]) -> Self {
        let sha256_hash = Sha256::digest(pubkey_bytes);
        let ripemd160_hash = Ripemd160::digest(&sha256_hash);
        let mut bytes = [0u8; 20];
        bytes.copy_from_slice(&ripemd160_hash);
        Self(bytes)
    }

    /// Convert to Bech32 string with the given prefix
    pub fn to_bech32(&self, hrp_str: &str) -> String {
        let hrp = Hrp::parse(hrp_str).expect("invalid hrp");
        bech32::encode::<Bech32>(hrp, &self.0).expect("encoding to bech32 should not fail")
    }

    /// Parse from Bech32 string
    pub fn from_bech32(s: &str) -> Result<(String, Self), String> {
        let (hrp, data) = bech32::decode(s).map_err(|e| e.to_string())?;
        if data.len() != 20 {
            return Err("invalid address length".to_string());
        }
        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&data);
        Ok((hrp.to_string(), Self(addr_bytes)))
    }

    /// Get the raw bytes
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }
}

// Similar implementations for String conversion
impl fmt::Display for AccAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Default to "cosmos" prefix for display
        write!(f, "{}", self.to_bech32("cosmos"))
    }
}

impl FromStr for AccAddress {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (_, addr) = Self::from_bech32(s)?;
        Ok(addr)
    }
}

// Similar implementations for ValAddress and ConsAddress
impl ValAddress {
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    pub fn to_bech32(&self, hrp_str: &str) -> String {
        let hrp = Hrp::parse(hrp_str).expect("invalid hrp");
        bech32::encode::<Bech32>(hrp, &self.0).expect("encoding to bech32 should not fail")
    }

    pub fn from_bech32(s: &str) -> Result<(String, Self), String> {
        let (hrp, data) = bech32::decode(s).map_err(|e| e.to_string())?;
        if data.len() != 20 {
            return Err("invalid address length".to_string());
        }
        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&data);
        Ok((hrp.to_string(), Self(addr_bytes)))
    }
}

impl ConsAddress {
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    pub fn to_bech32(&self, hrp_str: &str) -> String {
        let hrp = Hrp::parse(hrp_str).expect("invalid hrp");
        bech32::encode::<Bech32>(hrp, &self.0).expect("encoding to bech32 should not fail")
    }

    pub fn from_bech32(s: &str) -> Result<(String, Self), String> {
        let (hrp, data) = bech32::decode(s).map_err(|e| e.to_string())?;
        if data.len() != 20 {
            return Err("invalid address length".to_string());
        }
        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&data);
        Ok((hrp.to_string(), Self(addr_bytes)))
    }
}

// Display and FromStr implementations for ValAddress
impl fmt::Display for ValAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_bech32("cosmosvaloper"))
    }
}

impl FromStr for ValAddress {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (_, addr) = Self::from_bech32(s)?;
        Ok(addr)
    }
}

// Display and FromStr implementations for ConsAddress
impl fmt::Display for ConsAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_bech32("cosmosvalcons"))
    }
}

impl FromStr for ConsAddress {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (_, addr) = Self::from_bech32(s)?;
        Ok(addr)
    }
}
