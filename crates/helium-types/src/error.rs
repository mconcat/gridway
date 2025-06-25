//! Error handling for helium

use thiserror::Error;

/// Top-level SDK error enum that can cross module boundaries
#[derive(Error, Debug)]
pub enum SdkError {
    #[error("invalid address: {0}")]
    InvalidAddress(String),

    #[error("insufficient funds")]
    InsufficientFunds,

    #[error("unauthorized")]
    Unauthorized,

    #[error("invalid request: {0}")]
    InvalidRequest(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("internal error: {0}")]
    Internal(String),

    #[error("invalid genesis: {0}")]
    InvalidGenesis(String),
    // Module-specific errors will be added here with #[from] attributes
}

/// Trait for SDK errors that need to be compatible with ABCI error codes
pub trait IsSdkError {
    /// Returns the module's unique codespace string (e.g., "ante")
    fn codespace(&self) -> &'static str;

    /// Returns the numeric error code, matching Go SDK values for compatibility
    fn code(&self) -> u32;
}

// Example implementation for SdkError
impl IsSdkError for SdkError {
    fn codespace(&self) -> &'static str {
        match self {
            SdkError::InvalidAddress(_) => "sdk",
            SdkError::InsufficientFunds => "sdk",
            SdkError::Unauthorized => "sdk",
            SdkError::InvalidRequest(_) => "sdk",
            SdkError::NotFound(_) => "sdk",
            SdkError::Internal(_) => "sdk",
            SdkError::InvalidGenesis(_) => "sdk",
        }
    }

    fn code(&self) -> u32 {
        match self {
            SdkError::InvalidAddress(_) => 7, // Match Go SDK error codes
            SdkError::InsufficientFunds => 5,
            SdkError::Unauthorized => 4,
            SdkError::InvalidRequest(_) => 3,
            SdkError::NotFound(_) => 38,
            SdkError::Internal(_) => 1,
            SdkError::InvalidGenesis(_) => 26, // Match Go SDK ErrInvalidGenesis
        }
    }
}
