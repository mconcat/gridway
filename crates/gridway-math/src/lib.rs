//! Mathematical types for gridway
//!
//! This crate provides high-precision arithmetic types and coin handling
//! functionality required by gridway.

pub mod coin;
pub mod decimal;
pub mod int;

pub use coin::{Coin, Coins};
pub use decimal::Dec;
pub use int::Int;
