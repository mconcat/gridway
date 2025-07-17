//! Protocol Buffer definitions for Helium blockchain
//!
//! This crate provides all protobuf-generated types used across the Helium
//! blockchain implementation, including ABCI types from CometBFT and Cosmos SDK types.

/// CometBFT ABCI types
pub mod cometbft {
    pub mod abci {
        pub mod v1 {
            tonic::include_proto!("cometbft.abci.v1");
        }
    }
}

/// Cosmos SDK types
pub mod cosmos {
    pub mod base {
        pub mod abci {
            pub mod v1beta1 {
                tonic::include_proto!("cosmos.base.abci.v1beta1");
            }
        }
        pub mod query {
            pub mod v1beta1 {
                tonic::include_proto!("cosmos.base.query.v1beta1");
            }
        }
    }

    pub mod tx {
        pub mod v1beta1 {
            tonic::include_proto!("cosmos.tx.v1beta1");
        }
    }

    pub mod auth {
        pub mod v1beta1 {
            tonic::include_proto!("cosmos.auth.v1beta1");
        }
    }

    pub mod bank {
        pub mod v1beta1 {
            tonic::include_proto!("cosmos.bank.v1beta1");
        }
    }
}

/// Google protobuf types
pub mod google {
    pub mod protobuf {
        pub use prost_types::*;
    }
}

// Re-export commonly used types at the crate root for convenience
pub use cometbft::abci::v1::{
    CheckTxResponse, Event as AbciEvent, EventAttribute as AbciEventAttribute, ExecTxResult,
    FinalizeBlockRequest, FinalizeBlockResponse,
};

pub use cosmos::base::abci::v1beta1::{
    AbciMessageLog, Attribute as StringAttribute, Event as CosmosEvent,
    EventAttribute as CosmosEventAttribute, StringEvent,
};

pub use cosmos::tx::v1beta1::{GasInfo, Result as TxResult, TxResponse};

// Helper trait for converting between string and bytes representations
pub trait EventConversion {
    fn to_string_event(&self) -> StringEvent;
    fn to_cosmos_event(&self) -> CosmosEvent;
}

impl EventConversion for AbciEvent {
    fn to_string_event(&self) -> StringEvent {
        StringEvent {
            r#type: self.r#type.clone(),
            attributes: self
                .attributes
                .iter()
                .map(|attr| StringAttribute {
                    key: attr.key.clone(),
                    value: attr.value.clone(),
                })
                .collect(),
        }
    }

    fn to_cosmos_event(&self) -> CosmosEvent {
        CosmosEvent {
            r#type: self.r#type.clone(),
            attributes: self
                .attributes
                .iter()
                .map(|attr| CosmosEventAttribute {
                    key: attr.key.clone().into_bytes(),
                    value: attr.value.clone().into_bytes(),
                    index: attr.index,
                })
                .collect(),
        }
    }
}
