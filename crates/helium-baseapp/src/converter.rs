//! Conversion helpers for proto types

use crate::{Attribute as EventAttribute, Event, TxResponse};

/// Convert events from internal module representation to proto Event type
pub fn convert_module_events(events: Vec<crate::module_router::ModuleEvent>) -> Vec<Event> {
    events
        .into_iter()
        .map(|event| Event {
            r#type: event.event_type,
            attributes: event
                .attributes
                .into_iter()
                .map(|(key, value)| EventAttribute {
                    key,
                    value,
                    index: true,
                })
                .collect(),
        })
        .collect()
}

/// Create a TxResponse with the given parameters
pub fn create_tx_response(
    code: u32,
    log: String,
    info: String,
    data: Vec<u8>,
    gas_wanted: i64,
    gas_used: i64,
    events: Vec<Event>,
    codespace: String,
) -> TxResponse {
    TxResponse {
        code,
        data,
        log,
        info,
        gas_wanted,
        gas_used,
        events,
        codespace,
    }
}

/// Create a successful TxResponse
pub fn success_tx_response(
    log: String,
    data: Vec<u8>,
    gas_wanted: i64,
    gas_used: i64,
    events: Vec<Event>,
) -> TxResponse {
    create_tx_response(
        0,
        log,
        String::new(),
        data,
        gas_wanted,
        gas_used,
        events,
        String::new(),
    )
}

/// Create a failed TxResponse
pub fn failed_tx_response(code: u32, log: String, codespace: String, gas_used: i64) -> TxResponse {
    create_tx_response(
        code,
        log,
        String::new(),
        vec![],
        0,
        gas_used,
        vec![], // Failed transactions typically don't emit events
        codespace,
    )
}
