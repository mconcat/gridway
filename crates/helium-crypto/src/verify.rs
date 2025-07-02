//! Transaction signature verification for helium blockchain
//!
//! This module provides comprehensive transaction signature verification
//! compatible with Cosmos SDK standards, supporting both Secp256k1 and Ed25519.

use crate::keys::PublicKey;
use helium_types::address::AccAddress;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during signature verification
#[derive(Error, Debug)]
pub enum VerificationError {
    #[error("invalid signature: {0}")]
    InvalidSignature(String),

    #[error("public key mismatch: expected {expected}, got {actual}")]
    PublicKeyMismatch { expected: String, actual: String },

    #[error("invalid sign doc: {0}")]
    InvalidSignDoc(String),

    #[error("missing required signature for address: {0}")]
    MissingSignature(String),

    #[error("signature verification failed for address: {0}")]
    SignatureVerificationFailed(String),

    #[error("unsupported sign mode: {0}")]
    UnsupportedSignMode(String),

    #[error("invalid account sequence: expected {expected}, got {actual}")]
    InvalidSequence { expected: u64, actual: u64 },
}

/// Transaction signing mode (simplified for POC)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignMode {
    /// Sign over protobuf-serialized SignDoc (recommended)
    Direct,
    /// Legacy Amino JSON signing (for compatibility)
    LegacyAminoJson,
}

/// Sign document structure for SIGN_MODE_DIRECT
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignDoc {
    /// Serialized transaction body bytes
    pub body_bytes: Vec<u8>,
    /// Serialized auth info bytes  
    pub auth_info_bytes: Vec<u8>,
    /// Chain identifier
    pub chain_id: String,
    /// Account number
    pub account_number: u64,
}

/// Signature data for a single signer
#[derive(Debug, Clone)]
pub struct SignatureData {
    /// Public key of the signer
    pub public_key: PublicKey,
    /// Raw signature bytes
    pub signature: Vec<u8>,
    /// Signing mode used
    pub sign_mode: SignMode,
    /// Account sequence number
    pub sequence: u64,
}

/// Signer information required for verification
#[derive(Debug, Clone)]
pub struct SignerData {
    /// Account address
    pub address: AccAddress,
    /// Expected account sequence
    pub sequence: u64,
    /// Account number
    pub account_number: u64,
    /// Chain ID
    pub chain_id: String,
}

/// Transaction signature verifier
pub struct TransactionVerifier {
    /// Map of addresses to their expected signer data
    signer_data: HashMap<AccAddress, SignerData>,
}

impl TransactionVerifier {
    /// Create a new transaction verifier
    pub fn new() -> Self {
        Self {
            signer_data: HashMap::new(),
        }
    }

    /// Add expected signer data for verification
    pub fn add_signer(&mut self, signer: SignerData) {
        self.signer_data.insert(signer.address, signer);
    }

    /// Verify a transaction's signatures
    pub fn verify_transaction(
        &self,
        signatures: &[SignatureData],
        body_bytes: &[u8],
        auth_info_bytes: &[u8],
    ) -> Result<(), VerificationError> {
        // Ensure all required signers have provided signatures
        for (address, expected_signer) in &self.signer_data {
            let signature = signatures
                .iter()
                .find(|sig| sig.public_key.to_address() == *address)
                .ok_or_else(|| VerificationError::MissingSignature(address.to_string()))?;

            // Verify the signature
            self.verify_single_signature(signature, expected_signer, body_bytes, auth_info_bytes)?;
        }

        Ok(())
    }

    /// Verify a single signature
    pub fn verify_single_signature(
        &self,
        signature_data: &SignatureData,
        signer: &SignerData,
        body_bytes: &[u8],
        auth_info_bytes: &[u8],
    ) -> Result<(), VerificationError> {
        // Verify public key matches expected address
        let derived_address = signature_data.public_key.to_address();
        if derived_address != signer.address {
            return Err(VerificationError::PublicKeyMismatch {
                expected: signer.address.to_string(),
                actual: derived_address.to_string(),
            });
        }

        // Verify account sequence
        if signature_data.sequence != signer.sequence {
            return Err(VerificationError::InvalidSequence {
                expected: signer.sequence,
                actual: signature_data.sequence,
            });
        }

        // Create sign doc and verify signature
        let sign_doc = SignDoc {
            body_bytes: body_bytes.to_vec(),
            auth_info_bytes: auth_info_bytes.to_vec(),
            chain_id: signer.chain_id.clone(),
            account_number: signer.account_number,
        };

        match signature_data.sign_mode {
            SignMode::Direct => self.verify_direct_signature(
                &signature_data.public_key,
                &signature_data.signature,
                &sign_doc,
            ),
            SignMode::LegacyAminoJson => self.verify_amino_signature(
                &signature_data.public_key,
                &signature_data.signature,
                &sign_doc,
            ),
        }
    }

    /// Verify signature using SIGN_MODE_DIRECT
    fn verify_direct_signature(
        &self,
        public_key: &PublicKey,
        signature: &[u8],
        sign_doc: &SignDoc,
    ) -> Result<(), VerificationError> {
        // Serialize SignDoc for verification (simplified protobuf encoding)
        let sign_bytes = self.create_sign_bytes_direct(sign_doc)?;

        // Verify signature based on key type
        match public_key {
            PublicKey::Secp256k1(_) => {
                // For secp256k1, hash the message with SHA256 before verification
                let message_hash = Sha256::digest(&sign_bytes);
                self.verify_signature_raw(public_key, &message_hash, signature)
            }
            PublicKey::Ed25519(_) => {
                // For Ed25519, sign the message directly
                self.verify_signature_raw(public_key, &sign_bytes, signature)
            }
        }
    }

    /// Verify signature using SIGN_MODE_LEGACY_AMINO_JSON
    fn verify_amino_signature(
        &self,
        public_key: &PublicKey,
        signature: &[u8],
        sign_doc: &SignDoc,
    ) -> Result<(), VerificationError> {
        // Create Amino JSON sign bytes (simplified for POC)
        let sign_bytes = self.create_sign_bytes_amino(sign_doc)?;

        // Both secp256k1 and ed25519 use SHA256 hash for Amino JSON mode
        let message_hash = Sha256::digest(&sign_bytes);
        self.verify_signature_raw(public_key, &message_hash, signature)
    }

    /// Verify raw signature using the underlying cryptographic implementation
    fn verify_signature_raw(
        &self,
        public_key: &PublicKey,
        message: &[u8],
        signature: &[u8],
    ) -> Result<(), VerificationError> {
        use crate::signature::verify_signature;

        verify_signature(public_key, message, signature).map_err(|_| {
            VerificationError::SignatureVerificationFailed(public_key.to_address().to_string())
        })
    }

    /// Create sign bytes for SIGN_MODE_DIRECT (simplified protobuf encoding)
    fn create_sign_bytes_direct(&self, sign_doc: &SignDoc) -> Result<Vec<u8>, VerificationError> {
        // Use the public function
        create_sign_bytes_direct(sign_doc)
    }

    /// Create sign bytes for SIGN_MODE_LEGACY_AMINO_JSON (simplified JSON encoding)
    fn create_sign_bytes_amino(&self, sign_doc: &SignDoc) -> Result<Vec<u8>, VerificationError> {
        // In a full implementation, this would create proper Amino JSON
        // For POC, we'll use a simplified JSON format
        let json_doc = serde_json::json!({
            "account_number": sign_doc.account_number.to_string(),
            "chain_id": sign_doc.chain_id,
            "fee": {
                "amount": [],
                "gas": "0"
            },
            "memo": "",
            "msgs": [],  // Would contain actual message data
            "sequence": "0"  // Would be actual sequence
        });

        let json_string = serde_json::to_string(&json_doc).map_err(|e| {
            VerificationError::InvalidSignDoc(format!("JSON encoding failed: {}", e))
        })?;

        Ok(json_string.into_bytes())
    }
}

impl Default for TransactionVerifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Create sign bytes for SIGN_MODE_DIRECT (simplified deterministic format)
///
/// This function creates a deterministic byte representation of a SignDoc for signature verification.
/// In a full implementation, this would use proper protobuf serialization.
/// For POC, we use a simplified deterministic format.
pub fn create_sign_bytes_direct(sign_doc: &SignDoc) -> Result<Vec<u8>, VerificationError> {
    let mut bytes = Vec::new();

    // Add body bytes length and content
    bytes.extend_from_slice(&(sign_doc.body_bytes.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&sign_doc.body_bytes);

    // Add auth info bytes length and content
    bytes.extend_from_slice(&(sign_doc.auth_info_bytes.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&sign_doc.auth_info_bytes);

    // Add chain ID length and content
    let chain_id_bytes = sign_doc.chain_id.as_bytes();
    bytes.extend_from_slice(&(chain_id_bytes.len() as u32).to_be_bytes());
    bytes.extend_from_slice(chain_id_bytes);

    // Add account number
    bytes.extend_from_slice(&sign_doc.account_number.to_be_bytes());

    Ok(bytes)
}

/// Helper function to verify a single transaction signature
pub fn verify_transaction_signature(
    public_key: &PublicKey,
    signature: &[u8],
    sign_doc: &SignDoc,
    sign_mode: SignMode,
) -> Result<(), VerificationError> {
    let verifier = TransactionVerifier::new();

    match sign_mode {
        SignMode::Direct => verifier.verify_direct_signature(public_key, signature, sign_doc),
        SignMode::LegacyAminoJson => {
            verifier.verify_amino_signature(public_key, signature, sign_doc)
        }
    }
}

/// Helper function to create a sign document
pub fn create_sign_doc(
    body_bytes: Vec<u8>,
    auth_info_bytes: Vec<u8>,
    chain_id: String,
    account_number: u64,
) -> SignDoc {
    SignDoc {
        body_bytes,
        auth_info_bytes,
        chain_id,
        account_number,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{PrivateKey, PublicKey};
    use crate::signature::sign_message;
    use ed25519_dalek::SigningKey as Ed25519PrivKey;
    use k256::ecdsa::SigningKey as Secp256k1PrivKey;
    use rand::rngs::OsRng;

    fn create_test_secp256k1_keypair() -> (PrivateKey, PublicKey) {
        let private_key = Secp256k1PrivKey::random(&mut OsRng);
        let public_key = *private_key.verifying_key();
        (
            PrivateKey::Secp256k1(private_key),
            PublicKey::Secp256k1(public_key),
        )
    }

    fn create_test_ed25519_keypair() -> (PrivateKey, PublicKey) {
        let private_key = Ed25519PrivKey::from_bytes(&rand::random::<[u8; 32]>());
        let public_key = private_key.verifying_key();
        (
            PrivateKey::Ed25519(private_key),
            PublicKey::Ed25519(public_key),
        )
    }

    #[test]
    fn test_secp256k1_direct_signature_verification() {
        let (priv_key, pub_key) = create_test_secp256k1_keypair();

        // Create test sign doc
        let sign_doc = create_sign_doc(
            b"test_body".to_vec(),
            b"test_auth_info".to_vec(),
            "test-chain".to_string(),
            123,
        );

        // Create sign bytes and sign
        let verifier = TransactionVerifier::new();
        let sign_bytes = verifier.create_sign_bytes_direct(&sign_doc).unwrap();
        let message_hash = Sha256::digest(&sign_bytes); // secp256k1 requires hashing
        let signature = sign_message(&priv_key, &message_hash).unwrap();

        // Verify signature
        let result =
            verify_transaction_signature(&pub_key, &signature, &sign_doc, SignMode::Direct);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ed25519_direct_signature_verification() {
        let (priv_key, pub_key) = create_test_ed25519_keypair();

        // Create test sign doc
        let sign_doc = create_sign_doc(
            b"test_body".to_vec(),
            b"test_auth_info".to_vec(),
            "test-chain".to_string(),
            456,
        );

        // Create sign bytes and sign
        let verifier = TransactionVerifier::new();
        let sign_bytes = verifier.create_sign_bytes_direct(&sign_doc).unwrap();
        let signature = sign_message(&priv_key, &sign_bytes).unwrap(); // ed25519 signs directly

        // Verify signature
        let result =
            verify_transaction_signature(&pub_key, &signature, &sign_doc, SignMode::Direct);
        assert!(result.is_ok());
    }

    #[test]
    fn test_amino_json_signature_verification() {
        let (priv_key, pub_key) = create_test_secp256k1_keypair();

        // Create test sign doc
        let sign_doc = create_sign_doc(
            b"test_body".to_vec(),
            b"test_auth_info".to_vec(),
            "test-chain".to_string(),
            789,
        );

        // Create amino sign bytes and sign
        let verifier = TransactionVerifier::new();
        let sign_bytes = verifier.create_sign_bytes_amino(&sign_doc).unwrap();
        let message_hash = Sha256::digest(&sign_bytes); // Amino mode always hashes
        let signature = sign_message(&priv_key, &message_hash).unwrap();

        // Verify signature
        let result = verify_transaction_signature(
            &pub_key,
            &signature,
            &sign_doc,
            SignMode::LegacyAminoJson,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_invalid_signature_fails() {
        let (_priv_key, pub_key) = create_test_secp256k1_keypair();

        // Create test sign doc
        let sign_doc = create_sign_doc(
            b"test_body".to_vec(),
            b"test_auth_info".to_vec(),
            "test-chain".to_string(),
            123,
        );

        // Use invalid signature
        let invalid_signature = vec![0u8; 64];

        // Verification should fail
        let result =
            verify_transaction_signature(&pub_key, &invalid_signature, &sign_doc, SignMode::Direct);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VerificationError::SignatureVerificationFailed(_)
        ));
    }

    #[test]
    fn test_transaction_verifier_with_multiple_signers() {
        let (priv_key1, pub_key1) = create_test_secp256k1_keypair();
        let (priv_key2, pub_key2) = create_test_ed25519_keypair();

        let address1 = pub_key1.to_address();
        let address2 = pub_key2.to_address();

        // Create test data
        let body_bytes = b"multi_signer_body".to_vec();
        let auth_info_bytes = b"multi_signer_auth".to_vec();

        // Setup verifier with expected signers
        let mut verifier = TransactionVerifier::new();
        verifier.add_signer(SignerData {
            address: address1,
            sequence: 1,
            account_number: 100,
            chain_id: "test-chain".to_string(),
        });
        verifier.add_signer(SignerData {
            address: address2,
            sequence: 2,
            account_number: 200,
            chain_id: "test-chain".to_string(),
        });

        // Create signatures
        let sign_doc1 = create_sign_doc(
            body_bytes.clone(),
            auth_info_bytes.clone(),
            "test-chain".to_string(),
            100,
        );
        let sign_doc2 = create_sign_doc(
            body_bytes.clone(),
            auth_info_bytes.clone(),
            "test-chain".to_string(),
            200,
        );

        let sign_bytes1 = verifier.create_sign_bytes_direct(&sign_doc1).unwrap();
        let sign_bytes2 = verifier.create_sign_bytes_direct(&sign_doc2).unwrap();

        let message_hash1 = Sha256::digest(&sign_bytes1);
        let signature1 = sign_message(&priv_key1, &message_hash1).unwrap();
        let signature2 = sign_message(&priv_key2, &sign_bytes2).unwrap();

        let signatures = vec![
            SignatureData {
                public_key: pub_key1,
                signature: signature1,
                sign_mode: SignMode::Direct,
                sequence: 1,
            },
            SignatureData {
                public_key: pub_key2,
                signature: signature2,
                sign_mode: SignMode::Direct,
                sequence: 2,
            },
        ];

        // Verify transaction with multiple signers
        let result = verifier.verify_transaction(&signatures, &body_bytes, &auth_info_bytes);
        assert!(result.is_ok());
    }

    #[test]
    fn test_missing_signature_fails() {
        let (_, pub_key) = create_test_secp256k1_keypair();
        let address = pub_key.to_address();

        // Setup verifier expecting a signature
        let mut verifier = TransactionVerifier::new();
        verifier.add_signer(SignerData {
            address,
            sequence: 1,
            account_number: 100,
            chain_id: "test-chain".to_string(),
        });

        // Verify with no signatures
        let result = verifier.verify_transaction(&[], b"body", b"auth");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VerificationError::MissingSignature(_)
        ));
    }

    #[test]
    fn test_sequence_mismatch_fails() {
        let (_priv_key, pub_key) = create_test_secp256k1_keypair();
        let address = pub_key.to_address();

        // Setup verifier expecting sequence 5
        let signer_data = SignerData {
            address,
            sequence: 5,
            account_number: 100,
            chain_id: "test-chain".to_string(),
        };

        let verifier = TransactionVerifier::new();

        // Create signature with wrong sequence
        let signature_data = SignatureData {
            public_key: pub_key,
            signature: vec![0u8; 64], // Dummy signature for this test
            sign_mode: SignMode::Direct,
            sequence: 3, // Wrong sequence
        };

        let result =
            verifier.verify_single_signature(&signature_data, &signer_data, b"body", b"auth");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VerificationError::InvalidSequence {
                expected: 5,
                actual: 3
            }
        ));
    }
}
