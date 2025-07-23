//! Multisignature support (placeholder)

use crate::PublicKey;
use serde::{Deserialize, Serialize};

/// Legacy multisig public key (Protobuf compatible)
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LegacyMultisigPubKey {
    pub threshold: u32,
    pub public_keys: Vec<PublicKey>,
}

// TODO: Implement multisig verification logic
