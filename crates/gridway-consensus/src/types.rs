//! Consensus type aliases for gridway.
//!
//! Follows the same pattern as Alto's types module — defines the
//! cryptographic scheme and consensus certificate types.

use commonware_consensus::simplex::scheme::bls12381_threshold;
use commonware_consensus::simplex::types::{
    Activity as CActivity, Finalization as CFinalization, Notarization as CNotarization,
};
use commonware_cryptography::{
    bls12381::primitives::variant::MinSig,
    ed25519,
    sha256::Digest,
};

pub use commonware_consensus::simplex::scheme::bls12381_threshold::Seedable;

/// The BLS12-381 threshold signing scheme used for consensus.
pub type GridwayScheme = bls12381_threshold::Scheme<PublicKey, MinSig>;

/// Ed25519 public key type used for validator identity.
pub type PublicKey = ed25519::PublicKey;

/// BLS seed for leader election.
pub type Seed = bls12381_threshold::Seed<MinSig>;

/// Notarization certificate type.
pub type Notarization = CNotarization<GridwayScheme, Digest>;

/// Finalization certificate type.
pub type Finalization = CFinalization<GridwayScheme, Digest>;

/// Consensus activity (notarization or finalization).
pub type Activity = CActivity<GridwayScheme, Digest>;

/// The unique namespace prefix used in signing operations.
pub const NAMESPACE: &[u8] = b"_GRIDWAY";
