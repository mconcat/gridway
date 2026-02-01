//! WASI Hook Component
//!
//! Implements block lifecycle hooks for the Gridway blockchain.
//! Replaces the Cosmos ABCI BeginBlocker/EndBlocker pattern.
//!
//! - pre_execute:  Called before TX processing (inflation, epoch transitions)
//! - post_execute: Called after TX processing (reward distribution, settlement)

mod bindings;

use bindings::exports::gridway::framework::hook::{
    BlockContext, Event, EventAttribute, Guest, HookResult,
};
use bindings::gridway::framework::kvstore;

struct HookComponent;

impl Guest for HookComponent {
    fn pre_execute(ctx: BlockContext) -> HookResult {
        let mut events = Vec::new();

        // Emit block start event
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

        // Check for epoch transitions
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

    fn post_execute(ctx: BlockContext, tx_count: u32, total_gas: u64) -> HookResult {
        let mut events = Vec::new();

        // Emit block completion event
        events.push(Event {
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
        });

        HookResult {
            success: true,
            events,
            error: None,
        }
    }
}

bindings::export!(HookComponent with_types_in bindings);
