//! Counter Module Example
//!
//! This example demonstrates how to write a simple counter module for Helium.
//! In the actual system, this would be compiled to WASI and stored in the merkle
//! tree at a path like `/bin/counter` or `/home/myapp/bin/counter`.
//!
//! The module demonstrates:
//! - State management through VFS
//! - Message handling patterns
//! - Event emission
//! - Error handling
//!
//! This is what developers would write when creating blockchain applications
//! on Helium's WASI microkernel architecture.

use serde::{Deserialize, Serialize};
use std::fs;

// In the real implementation, these would come from the WASI component interface
mod helium {
    pub mod types {}
}

/// The messages that this module can handle
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum CounterMsg {
    /// Increment the counter by a given amount
    #[serde(rename = "counter/Increment")]
    Increment { amount: u64 },

    /// Decrement the counter by a given amount
    #[serde(rename = "counter/Decrement")]
    Decrement { amount: u64 },

    /// Reset the counter to zero
    #[serde(rename = "counter/Reset")]
    Reset,

    /// Query the current counter value
    #[serde(rename = "counter/Query")]
    Query,
}

/// The state structure for our counter
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CounterState {
    pub value: u64,
    pub total_operations: u64,
}

/// Response types for our module
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CounterResponse {
    /// Response to state-changing operations
    Ok { message: String },
    /// Response to query operations
    QueryResponse { value: u64, total_operations: u64 },
}

/// Event emitted by counter operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub r#type: String,
    pub attributes: Vec<EventAttribute>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventAttribute {
    pub key: String,
    pub value: String,
}

/// Entry point for the counter module
///
/// In a real WASI component, this would be exported as the main handler function.
/// The Helium runtime would call this function with serialized messages.
pub fn handle_message(msg_bytes: &[u8]) -> Result<Vec<u8>, String> {
    // Deserialize the incoming message
    let msg: CounterMsg =
        serde_json::from_slice(msg_bytes).map_err(|e| format!("Failed to parse message: {e}"))?;

    // Process the message
    match msg {
        CounterMsg::Increment { amount } => handle_increment(amount),
        CounterMsg::Decrement { amount } => handle_decrement(amount),
        CounterMsg::Reset => handle_reset(),
        CounterMsg::Query => handle_query(),
    }
}

/// Load counter state from VFS
///
/// In Helium, modules access their state through the Virtual Filesystem.
/// This module's state would be stored at `/home/counter/state`
fn load_state() -> Result<CounterState, String> {
    // In a real WASI component, this would use WASI file operations
    // to read from the VFS path `/home/counter/state`
    let state_path = "/home/counter/state";

    // Simulated VFS read
    match fs::read_to_string(state_path) {
        Ok(data) => serde_json::from_str(&data).map_err(|e| format!("Failed to parse state: {e}")),
        Err(_) => {
            // If state doesn't exist, return default
            Ok(CounterState::default())
        }
    }
}

/// Save counter state to VFS
fn save_state(state: &CounterState) -> Result<(), String> {
    let state_path = "/home/counter/state";
    let state_json =
        serde_json::to_string(state).map_err(|e| format!("Failed to serialize state: {e}"))?;

    // In a real WASI component, this would use WASI file operations
    // The runtime ensures atomic writes within a transaction
    fs::write(state_path, state_json).map_err(|e| format!("Failed to write state: {e}"))
}

/// Emit an event that will be included in the block
///
/// Events are written to a special VFS path that the runtime monitors
fn emit_event(event: Event) -> Result<(), String> {
    let event_json =
        serde_json::to_vec(&event).map_err(|e| format!("Failed to serialize event: {e}"))?;

    // In the real system, events would be written to `/sys/events/`
    // and collected by the runtime
    println!("Event emitted: {event:?}");
    Ok(())
}

/// Handle increment message
fn handle_increment(amount: u64) -> Result<Vec<u8>, String> {
    let mut state = load_state()?;

    // Update state with overflow protection
    state.value = state.value.saturating_add(amount);
    state.total_operations += 1;

    save_state(&state)?;

    // Emit event
    emit_event(Event {
        r#type: "counter.incremented".to_string(),
        attributes: vec![
            EventAttribute {
                key: "amount".to_string(),
                value: amount.to_string(),
            },
            EventAttribute {
                key: "new_value".to_string(),
                value: state.value.to_string(),
            },
        ],
    })?;

    // Return response
    let response = CounterResponse::Ok {
        message: format!("Counter incremented by {} to {}", amount, state.value),
    };

    serde_json::to_vec(&response).map_err(|e| format!("Failed to serialize response: {e}"))
}

/// Handle decrement message
fn handle_decrement(amount: u64) -> Result<Vec<u8>, String> {
    let mut state = load_state()?;

    // Update state with underflow protection
    state.value = state.value.saturating_sub(amount);
    state.total_operations += 1;

    save_state(&state)?;

    // Emit event
    emit_event(Event {
        r#type: "counter.decremented".to_string(),
        attributes: vec![
            EventAttribute {
                key: "amount".to_string(),
                value: amount.to_string(),
            },
            EventAttribute {
                key: "new_value".to_string(),
                value: state.value.to_string(),
            },
        ],
    })?;

    // Return response
    let response = CounterResponse::Ok {
        message: format!("Counter decremented by {} to {}", amount, state.value),
    };

    serde_json::to_vec(&response).map_err(|e| format!("Failed to serialize response: {e}"))
}

/// Handle reset message
fn handle_reset() -> Result<Vec<u8>, String> {
    let mut state = load_state()?;
    let old_value = state.value;

    state.value = 0;
    state.total_operations += 1;

    save_state(&state)?;

    // Emit event
    emit_event(Event {
        r#type: "counter.reset".to_string(),
        attributes: vec![EventAttribute {
            key: "old_value".to_string(),
            value: old_value.to_string(),
        }],
    })?;

    // Return response
    let response = CounterResponse::Ok {
        message: "Counter reset to 0".to_string(),
    };

    serde_json::to_vec(&response).map_err(|e| format!("Failed to serialize response: {e}"))
}

/// Handle query message
fn handle_query() -> Result<Vec<u8>, String> {
    let state = load_state()?;

    let response = CounterResponse::QueryResponse {
        value: state.value,
        total_operations: state.total_operations,
    };

    serde_json::to_vec(&response).map_err(|e| format!("Failed to serialize response: {e}"))
}

/// Module initialization
///
/// Called when the module is first loaded or during chain initialization
pub fn init() -> Result<(), String> {
    // Create initial state
    let initial_state = CounterState {
        value: 0,
        total_operations: 0,
    };

    save_state(&initial_state)?;

    // Emit initialization event
    emit_event(Event {
        r#type: "counter.initialized".to_string(),
        attributes: vec![],
    })?;

    Ok(())
}

// In a real WASI component, these would be the exported functions
// that the Helium runtime would call

#[cfg(feature = "wasi")]
mod wasi_exports {
    use super::*;

    #[no_mangle]
    pub extern "C" fn init() -> i32 {
        match super::init() {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("Init failed: {}", e);
                1
            }
        }
    }

    #[no_mangle]
    pub extern "C" fn handle_message(msg_ptr: *const u8, msg_len: usize) -> i32 {
        let msg_bytes = unsafe { std::slice::from_raw_parts(msg_ptr, msg_len) };

        match super::handle_message(msg_bytes) {
            Ok(response) => {
                // Write response to stdout for the runtime to collect
                std::io::stdout().write_all(&response).unwrap();
                0
            }
            Err(e) => {
                eprintln!("Message handling failed: {}", e);
                1
            }
        }
    }
}

// Example usage and tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_serialization() {
        // Show how messages would be constructed
        let msg = CounterMsg::Increment { amount: 5 };
        let msg_bytes = serde_json::to_vec(&msg).unwrap();
        let msg_str = String::from_utf8(msg_bytes.clone()).unwrap();
        println!("Increment message JSON: {}", msg_str);

        // Show the message format that would be sent to the module
        assert!(msg_str.contains("counter/Increment"));
        assert!(msg_str.contains("\"amount\":5"));
    }

    #[test]
    fn test_state_format() {
        // Show the state format that would be stored in VFS
        let state = CounterState {
            value: 42,
            total_operations: 10,
        };
        let state_json = serde_json::to_string_pretty(&state).unwrap();
        println!("State stored at /home/counter/state:");
        println!("{}", state_json);
    }
}

// Documentation for developers
/// # Counter Module Development Guide
///
/// This example shows how to develop a module for Helium's WASI microkernel.
///
/// ## Key Concepts:
///
/// 1. **State Management**: All state is accessed through the VFS at paths under
///    `/home/{module_name}/`. The runtime ensures ACID properties.
///
/// 2. **Message Handling**: Modules export a `handle_message` function that receives
///    serialized messages and returns serialized responses.
///
/// 3. **Events**: Modules emit events by writing to special VFS paths. These are
///    collected by the runtime and included in blocks.
///
/// 4. **Capabilities**: In production, modules only have access to their designated
///    VFS paths. The runtime enforces capability-based security.
///
/// ## Compilation:
///
/// ```bash
/// # Compile to WASI component
/// cargo component build --release
///
/// # The resulting .wasm file would be uploaded to the chain at:
/// # /bin/counter or /home/myapp/bin/counter
/// ```
///
/// ## Deployment:
///
/// Once compiled, the module can be deployed via governance proposal or
/// during chain initialization. The module path determines how it's accessed.
///
/// ## Integration:
///
/// Other modules or clients can send messages to this module using the
/// message types defined above. The runtime routes messages based on the
/// type field (e.g., "counter/Increment").
fn main() {
    println!("Counter Module Example");
    println!("=====================");
    println!();
    println!("This example demonstrates how to write a WASI module for Helium.");
    println!("In a real deployment, this would be compiled to WebAssembly and");
    println!("stored in the blockchain at a path like /bin/counter.");
    println!();
    println!("See the source code for the complete implementation.");
}
