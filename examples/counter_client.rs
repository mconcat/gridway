//! Counter Client Example
//!
//! This example shows how to interact with the counter module from a client
//! application. It demonstrates:
//! - Building transactions
//! - Querying module state
//! - Handling responses
//!
//! This represents what a developer would write when building a client
//! application that interacts with modules on the Gridway blockchain.

use serde::{Deserialize, Serialize};

/// Transaction structure that wraps module messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// The sender's address
    pub from: String,
    /// The module path (e.g., "/bin/counter" or "/home/myapp/bin/counter")
    pub to: String,
    /// The message to send to the module
    pub msg: serde_json::Value,
    /// Gas limit for execution
    pub gas_limit: u64,
    /// Transaction nonce
    pub nonce: u64,
}

/// Example client code for interacting with the counter module
pub struct CounterClient {
    /// The module path where the counter is deployed
    module_path: String,
    /// The sender address
    sender: String,
    /// Current nonce
    nonce: u64,
}

impl CounterClient {
    pub fn new(module_path: String, sender: String) -> Self {
        Self {
            module_path,
            sender,
            nonce: 0,
        }
    }

    /// Increment the counter
    pub fn increment(&mut self, amount: u64) -> Transaction {
        self.nonce += 1;
        Transaction {
            from: self.sender.clone(),
            to: self.module_path.clone(),
            msg: serde_json::json!({
                "type": "counter/Increment",
                "amount": amount
            }),
            gas_limit: 100_000,
            nonce: self.nonce,
        }
    }

    /// Decrement the counter
    pub fn decrement(&mut self, amount: u64) -> Transaction {
        self.nonce += 1;
        Transaction {
            from: self.sender.clone(),
            to: self.module_path.clone(),
            msg: serde_json::json!({
                "type": "counter/Decrement",
                "amount": amount
            }),
            gas_limit: 100_000,
            nonce: self.nonce,
        }
    }

    /// Reset the counter
    pub fn reset(&mut self) -> Transaction {
        self.nonce += 1;
        Transaction {
            from: self.sender.clone(),
            to: self.module_path.clone(),
            msg: serde_json::json!({
                "type": "counter/Reset"
            }),
            gas_limit: 100_000,
            nonce: self.nonce,
        }
    }

    /// Query the counter value (doesn't consume nonce)
    pub fn query(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "counter/Query"
        })
    }
}

/// Example of how to use the client
#[cfg(test)]
mod examples {
    use super::*;

    #[test]
    fn example_transaction_building() {
        let mut client =
            CounterClient::new("/bin/counter".to_string(), "cosmos1abcdef...".to_string());

        // Build an increment transaction
        let tx = client.increment(5);
        println!("Increment transaction:");
        println!("{}", serde_json::to_string_pretty(&tx).unwrap());

        // Build a decrement transaction
        let tx = client.decrement(2);
        println!("\nDecrement transaction:");
        println!("{}", serde_json::to_string_pretty(&tx).unwrap());

        // Build a query (read-only, no transaction needed)
        let query = client.query();
        println!("\nQuery message:");
        println!("{}", serde_json::to_string_pretty(&query).unwrap());
    }

    #[test]
    fn example_batch_operations() {
        let mut client = CounterClient::new(
            "/home/myapp/bin/counter".to_string(),
            "cosmos1xyz...".to_string(),
        );

        // Build multiple transactions
        let transactions = vec![
            client.increment(10),
            client.increment(5),
            client.decrement(3),
            client.reset(),
            client.increment(7),
        ];

        println!("Batch of {} transactions:", transactions.len());
        for (i, tx) in transactions.iter().enumerate() {
            println!("\nTransaction {}:", i + 1);
            println!("  To:: {}", tx.to);
            println!("  Message:: {}", tx.msg);
            println!("  Nonce:: {}", tx.nonce);
        }
    }
}

/// Documentation showing the full interaction flow
///
/// ```ignore
/// // 1. Create a client pointing to the deployed counter module
/// let mut client = CounterClient::new(
///     "/bin/counter".to_string(),
///     "cosmos1myaddress...".to_string(),
/// );
///
/// // 2. Build a transaction
/// let tx = client.increment(42);
///
/// // 3. Sign the transaction (using gridway-keyring or similar)
/// let signed_tx = sign_transaction(tx, private_key);
///
/// // 4. Broadcast to the network
/// let response = broadcast_tx(signed_tx).await?;
///
/// // 5. Check the response
/// match response.code {
///     0 => println!("Success! Events:: {:?}", response.events),
///     _ => println!("Failed:: {}", response.log),
/// }
///
/// // 6. Query the current state
/// let query_response = query_module("/bin/counter", client.query()).await?;
/// println!("Current counter value:: {}", query_response["value"]);
/// ```
fn main() {
    // Example entry point - in practice, these would be library functions
    println!("See the examples module for usage patterns");
}

// Example showing module composition
//
// In Gridway, modules can call other modules. Here's how a governance
// module might interact with our counter:
//
// ```ignore
// // In the governance module's proposal handler:
// fn execute_proposal(proposal: Proposal) -> Result<(), String> {
//     match proposal.action {
//         ProposalAction::ResetCounter => {
//             // Call the counter module
//             let msg = serde_json::json!({
//                 "type": "counter/Reset"
//             });
//
//             // Use VFS to send inter-module message
//             let response = call_module("/bin/counter", msg)?;
//
//             // Log the result
//             emit_event(Event {
//                 r#type: "proposal.executed".to_string(),
//                 attributes: vec![
//                     EventAttribute {
//                         key: "proposal_id".to_string(),
//                         value: proposal.id.to_string(),
//                     },
//                     EventAttribute {
//                         key: "action".to_string(),
//                         value: "counter_reset".to_string(),
//                     },
//                 ],
//             })?;
//
//             Ok(())
//         }
//         // ... other proposal actions
//     }
// }
// ```
