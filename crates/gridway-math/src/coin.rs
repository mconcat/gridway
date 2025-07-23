//! Coin and Coins types for handling tokens

use crate::int::Int;
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoinError {
    #[error("invalid denomination:: {0}")]
    InvalidDenom(String),

    #[error("negative amount not allowed")]
    NegativeAmount,

    #[error("duplicate denomination:: {0}")]
    DuplicateDenom(String),
}

/// A single coin with denomination and amount
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coin {
    pub denom: String,
    pub amount: Int,
}

impl Coin {
    /// Create a new coin, validating denomination and amount
    pub fn new(denom: String, amount: Int) -> Result<Self, CoinError> {
        // Validate denom matches helium format
        if !is_valid_denom(&denom) {
            return Err(CoinError::InvalidDenom(denom));
        }

        // Amount must not be negative
        if amount.is_negative() {
            return Err(CoinError::NegativeAmount);
        }

        Ok(Self { denom, amount })
    }

    /// Check if coin is zero
    pub fn is_zero(&self) -> bool {
        self.amount.is_zero()
    }
}

impl fmt::Display for Coin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.amount, self.denom)
    }
}

/// A collection of coins, always sorted by denomination
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Coins(Vec<Coin>);

impl Coins {
    /// Create a new Coins collection from a vector of coins
    /// Enforces sorting by denomination and no duplicates
    pub fn new(mut coins: Vec<Coin>) -> Result<Self, CoinError> {
        // Remove zero coins
        coins.retain(|c| !c.is_zero());

        // Sort by denomination
        coins.sort_by(|a, b| a.denom.cmp(&b.denom));

        // Check for duplicates
        for window in coins.windows(2) {
            if window[0].denom == window[1].denom {
                return Err(CoinError::DuplicateDenom(window[0].denom.clone()));
            }
        }

        Ok(Self(coins))
    }

    /// Create an empty Coins collection
    pub fn empty() -> Self {
        Self(Vec::new())
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Get coins as slice
    pub fn as_slice(&self) -> &[Coin] {
        &self.0
    }

    /// Add a coin to the collection
    pub fn add(&mut self, coin: Coin) -> Result<(), CoinError> {
        if coin.is_zero() {
            return Ok(());
        }

        let mut found = false;
        for c in &mut self.0 {
            if c.denom == coin.denom {
                c.amount = c.amount.clone() + coin.amount.clone();
                found = true;
                break;
            }
        }

        if !found {
            self.0.push(coin);
            self.0.sort_by(|a, b| a.denom.cmp(&b.denom));
        }

        Ok(())
    }

    /// Find amount of a specific denomination
    pub fn amount_of(&self, denom: &str) -> Int {
        self.0
            .iter()
            .find(|c| c.denom == denom)
            .map(|c| c.amount.clone())
            .unwrap_or_else(Int::zero)
    }
}

impl fmt::Display for Coins {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_empty() {
            write!(f, "")
        } else {
            let s: Vec<String> = self.0.iter().map(|c| c.to_string()).collect();
            write!(f, "{}", s.join(","))
        }
    }
}

/// Validate denomination format according to helium rules
fn is_valid_denom(denom: &str) -> bool {
    if denom.is_empty() || denom.len() > 127 {
        return false;
    }

    // Must start with a letter
    if !denom.chars().next().unwrap().is_alphabetic() {
        return false;
    }

    // Can only contain alphanumeric characters and certain symbols
    denom
        .chars()
        .all(|c| c.is_alphanumeric() || c == '/' || c == ':' || c == '.' || c == '_' || c == '-')
}
