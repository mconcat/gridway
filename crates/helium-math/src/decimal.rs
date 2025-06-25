//! Fixed-point decimal type for precise calculations

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, Div, Mul, Sub};
use std::str::FromStr;

/// Fixed-point decimal with 18 decimal places of precision
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Dec(Decimal);

impl Dec {
    /// Create a new Dec from string
    pub fn from_str(s: &str) -> Result<Self, rust_decimal::Error> {
        Ok(Self(Decimal::from_str(s)?))
    }

    /// Create from i64
    pub fn from_i64(n: i64) -> Self {
        Self(Decimal::from(n))
    }

    /// Zero value
    pub fn zero() -> Self {
        Self(Decimal::ZERO)
    }

    /// One value
    pub fn one() -> Self {
        Self(Decimal::ONE)
    }

    /// Check if zero
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    /// Check if negative
    pub fn is_negative(&self) -> bool {
        self.0.is_sign_negative()
    }

    /// Check if positive
    pub fn is_positive(&self) -> bool {
        self.0.is_sign_positive()
    }

    /// Checked addition
    pub fn checked_add(&self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    /// Checked subtraction
    pub fn checked_sub(&self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    /// Checked multiplication
    pub fn checked_mul(&self, other: Self) -> Option<Self> {
        self.0.checked_mul(other.0).map(Self)
    }

    /// Checked division
    pub fn checked_div(&self, other: Self) -> Option<Self> {
        if other.is_zero() {
            None
        } else {
            self.0.checked_div(other.0).map(Self)
        }
    }

    /// Absolute value
    pub fn abs(&self) -> Self {
        Self(self.0.abs())
    }

    /// Floor value
    pub fn floor(&self) -> Self {
        Self(self.0.floor())
    }

    /// Ceiling value
    pub fn ceil(&self) -> Self {
        Self(self.0.ceil())
    }

    /// Round to specified decimal places
    pub fn round(&self, decimal_places: u32) -> Self {
        Self(self.0.round_dp(decimal_places))
    }
}

impl FromStr for Dec {
    type Err = rust_decimal::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(Decimal::from_str(s)?))
    }
}

impl fmt::Display for Dec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add for Dec {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for Dec {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl Mul for Dec {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self(self.0 * other.0)
    }
}

impl Div for Dec {
    type Output = Self;

    fn div(self, other: Self) -> Self {
        Self(self.0 / other.0)
    }
}
