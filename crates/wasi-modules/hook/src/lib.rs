//! WASI Hook Component — block lifecycle hooks via WASM/VFS
//!
//! Implements pre_execute / post_execute hooks for Gridway block processing.
//! All state access goes through the kvstore WIT interface (VFS-backed).

mod bindings {
    wit_bindgen::generate!({
        world: "hook-world",
        path: "../../../wit",
    });
}

use bindings::exports::gridway::framework::hook::{
    BlockContext, Event, EventAttribute, Guest, HookResult,
};
use bindings::gridway::framework::kvstore;

struct HookComponent;

impl Guest for HookComponent {
    /// Called before TX processing in a block.
    fn pre_execute(ctx: BlockContext) -> HookResult {
        let mut events = Vec::new();

        // Emit block_start event
        events.push(Event {
            event_type: "block_start".to_string(),
            attributes: vec![
                EventAttribute {
                    key: "height".to_string(),
                    value: ctx.height.to_string(),
                },
                EventAttribute {
                    key: "timestamp".to_string(),
                    value: ctx.timestamp.to_string(),
                },
                EventAttribute {
                    key: "chain_id".to_string(),
                    value: ctx.chain_id.clone(),
                },
            ],
        });

        // Daily epoch transition check
        if ctx.height > 0 && ctx.height % 86400 == 0 {
            events.push(Event {
                event_type: "epoch_transition".to_string(),
                attributes: vec![
                    EventAttribute {
                        key: "epoch_type".to_string(),
                        value: "daily".to_string(),
                    },
                    EventAttribute {
                        key: "height".to_string(),
                        value: ctx.height.to_string(),
                    },
                ],
            });
        }

        HookResult {
            success: true,
            events,
            error: None,
        }
    }

    /// Called after all TXs in a block have been processed.
    fn post_execute(ctx: BlockContext, tx_count: u32, total_gas: u64) -> HookResult {
        let events = vec![Event {
            event_type: "block_completed".to_string(),
            attributes: vec![
                EventAttribute {
                    key: "height".to_string(),
                    value: ctx.height.to_string(),
                },
                EventAttribute {
                    key: "tx_count".to_string(),
                    value: tx_count.to_string(),
                },
                EventAttribute {
                    key: "total_gas".to_string(),
                    value: total_gas.to_string(),
                },
            ],
        }];

        HookResult {
            success: true,
            events,
            error: None,
        }
    }
}

bindings::export!(HookComponent with_types_in bindings);
