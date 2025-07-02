//! Signature operations

use crate::keys::{PrivateKey, PublicKey};
use signature::{Signer, Verifier};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum SignatureError {
    #[error("signing failed: {0}")]
    SigningFailed(String),

    #[error("verification failed")]
    VerificationFailed,
}

/// Sign a message with a private key
pub fn sign_message(key: &PrivateKey, message: &[u8]) -> Result<Vec<u8>, SignatureError> {
    match key {
        PrivateKey::Secp256k1(k) => {
            use k256::ecdsa::Signature;
            let sig: Signature = k.sign(message);
            Ok(sig.to_der().as_bytes().to_vec())
        }
        PrivateKey::Ed25519(k) => {
            use ed25519_dalek::Signature;
            let sig: Signature = k.sign(message);
            Ok(sig.to_bytes().to_vec())
        }
    }
}

/// Verify a signature with a public key
pub fn verify_signature(
    key: &PublicKey,
    message: &[u8],
    signature: &[u8],
) -> Result<(), SignatureError> {
    match key {
        PublicKey::Secp256k1(k) => {
            use k256::ecdsa::Signature;
            let sig =
                Signature::from_der(signature).map_err(|_| SignatureError::VerificationFailed)?;
            k.verify(message, &sig)
                .map_err(|_| SignatureError::VerificationFailed)?;
            Ok(())
        }
        PublicKey::Ed25519(k) => {
            use ed25519_dalek::Signature;
            let sig = Signature::from_bytes(
                signature
                    .try_into()
                    .map_err(|_| SignatureError::VerificationFailed)?,
            );
            k.verify(message, &sig)
                .map_err(|_| SignatureError::VerificationFailed)?;
            Ok(())
        }
    }
}
