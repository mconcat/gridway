//! Mathematical types for helium
//!
//! This crate provides high-precision arithmetic types and coin handling
//! functionality required by helium.

pub mod coin;
pub mod decimal;
pub mod int;

pub use coin::{Coin, Coins};
pub use decimal::Dec;
pub use int::Int;
