//! Request-scoped context for the host environment

use helium_store::GlobalAppStore;
use helium_proto::cometbft::abci::v1::{Event, EventAttribute};

/// Host-side context for request processing
///
/// This is a request-scoped dependency handle constructed by baseapp
/// for each major operation (CheckTx, FinalizeBlock). It serves as the
/// primary container for all dependencies needed during that operation.
pub struct Ctx<'a> {
    /// Current block height
    pub block_height: i64,

    /// Chain ID
    pub chain_id: &'a str,

    /// Event manager for accumulating ABCI events
    pub event_manager: &'a mut EventManager,

    /// Gas meter for tracking gas consumption
    pub gas_meter: &'a mut dyn GasMeter,

    /// Global application store for state access
    pub store: &'a GlobalAppStore,
}

/// Manages ABCI events
#[derive(Default)]
pub struct EventManager {
    events: Vec<Event>,
}

impl EventManager {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn emit(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn get_events(&self) -> &[Event] {
        &self.events
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }
}

/// Gas meter trait for tracking gas consumption
pub trait GasMeter: Send + Sync {
    /// Consume the specified amount of gas
    fn consume_gas(&mut self, amount: u64, descriptor: &str) -> Result<(), GasError>;

    /// Get the gas consumed so far
    fn gas_consumed(&self) -> u64;

    /// Get the gas limit
    fn gas_limit(&self) -> u64;

    /// Check if gas limit has been exceeded
    fn is_out_of_gas(&self) -> bool {
        self.gas_consumed() >= self.gas_limit()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GasError {
    #[error("out of gas: consumed {consumed}, limit {limit}")]
    OutOfGas { consumed: u64, limit: u64 },
}
