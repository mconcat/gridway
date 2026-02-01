//! Event types for gridway.
//!
//! Replaces the CometBFT ABCI event types with simple native types.

/// A blockchain event emitted during block/transaction processing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Event {
    /// Event type identifier
    pub r#type: String,
    /// Event attributes
    pub attributes: Vec<EventAttribute>,
}

/// A key-value attribute on an event.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventAttribute {
    pub key: String,
    pub value: String,
    pub index: bool,
}

impl Event {
    /// Create a new event
    pub fn new(event_type: impl Into<String>, attributes: Vec<EventAttribute>) -> Self {
        Self {
            r#type: event_type.into(),
            attributes,
        }
    }
}

impl EventAttribute {
    /// Create a new indexed attribute
    pub fn new(key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            value: value.into(),
            index: true,
        }
    }
}
