//! Error handling types and utilities for the helium blockchain framework.
//!
//! This crate provides a comprehensive error handling system for helium applications,
//! including standard error types, error codes, and utilities for error propagation.

use thiserror::Error;

/// Core error type for helium applications
#[derive(Error, Debug)]
pub enum Error {
    /// Invalid request error
    #[error("invalid request:: {0}")]
    InvalidRequest(String),

    /// Not found error
    #[error("not found:: {0}")]
    NotFound(String),

    /// Unauthorized error
    #[error("unauthorized:: {0}")]
    Unauthorized(String),

    /// Insufficient funds error
    #[error("insufficient funds:: {0}")]
    InsufficientFunds(String),

    /// Unknown error
    #[error("unknown error:: {0}")]
    Unknown(String),

    /// Custom error with error code
    #[error("error {code}: {message}")]
    Custom { code: u32, message: String },
}

/// Result type alias for helium operations
pub type Result<T> = std::result::Result<T, Error>;

/// Error codes following helium conventions
pub mod codes {
    /// Success
    pub const OK: u32 = 0;
    /// Internal error
    pub const INTERNAL: u32 = 1;
    /// Invalid argument
    pub const INVALID_ARGUMENT: u32 = 3;
    /// Not found
    pub const NOT_FOUND: u32 = 5;
    /// Unauthorized
    pub const UNAUTHORIZED: u32 = 7;
    /// Insufficient funds
    pub const INSUFFICIENT_FUNDS: u32 = 10;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::InvalidRequest("missing field".to_string());
        assert_eq!(err.to_string(), "invalid request:: missing field");
    }
}
