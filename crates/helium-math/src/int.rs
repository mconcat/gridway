//! Arbitrary precision integer type

use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, Div, Mul, Sub};
use std::str::FromStr;

/// Arbitrary precision signed integer
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Int(BigInt);

// Custom serialization for BigInt
impl Serialize for Int {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Int {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        BigInt::from_str(&s)
            .map(Int)
            .map_err(serde::de::Error::custom)
    }
}

impl Int {
    /// Create a new Int from i64
    pub fn from_i64(n: i64) -> Self {
        Self(BigInt::from(n))
    }

    /// Create a new Int from u64
    pub fn from_u64(n: u64) -> Self {
        Self(BigInt::from(n))
    }

    /// Zero value
    pub fn zero() -> Self {
        Self(BigInt::zero())
    }

    /// Check if zero
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    /// Check if negative
    pub fn is_negative(&self) -> bool {
        self.0.is_negative()
    }

    /// Check if positive
    pub fn is_positive(&self) -> bool {
        self.0.is_positive()
    }

    /// Checked addition
    pub fn checked_add(&self, other: &Self) -> Option<Self> {
        Some(Self(&self.0 + &other.0))
    }

    /// Checked subtraction
    pub fn checked_sub(&self, other: &Self) -> Option<Self> {
        Some(Self(&self.0 - &other.0))
    }

    /// Checked multiplication
    pub fn checked_mul(&self, other: &Self) -> Option<Self> {
        Some(Self(&self.0 * &other.0))
    }

    /// Checked division
    pub fn checked_div(&self, other: &Self) -> Option<Self> {
        if other.is_zero() {
            None
        } else {
            Some(Self(&self.0 / &other.0))
        }
    }

    /// Absolute value
    pub fn abs(&self) -> Self {
        Self(self.0.abs())
    }

    /// Convert to string
    pub fn to_string(&self) -> String {
        self.0.to_string()
    }
}

impl FromStr for Int {
    type Err = num_bigint::ParseBigIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(BigInt::from_str(s)?))
    }
}

impl fmt::Display for Int {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Add for Int {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for Int {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl Mul for Int {
    type Output = Self;

    fn mul(self, other: Self) -> Self {
        Self(self.0 * other.0)
    }
}

impl Div for Int {
    type Output = Self;

    fn div(self, other: Self) -> Self {
        Self(self.0 / other.0)
    }
}

impl Zero for Int {
    fn zero() -> Self {
        Self(BigInt::zero())
    }

    fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}
