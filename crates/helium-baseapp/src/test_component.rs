//! Tests for WASI component model integration

#[cfg(test)]
mod tests {
    use crate::component_host::{ComponentHost, ComponentInfo, ComponentType};
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    #[test]
    fn test_component_host_creation() {
        let base_store = Arc::new(Mutex::new(helium_store::MemStore::new()));
        let _host = ComponentHost::new(base_store).unwrap();
        // Basic test that host can be created
    }

    #[test]
    fn test_tx_decoder_component() {
        let base_store = Arc::new(Mutex::new(helium_store::MemStore::new()));
        let host = ComponentHost::new(base_store).unwrap();

        // Load the tx-decoder component if it exists
        let component_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("modules/tx_decoder_component.wasm");

        if component_path.exists() {
            let component_bytes = std::fs::read(&component_path).unwrap();

            let info = ComponentInfo {
                name: "tx-decoder".to_string(),
                path: component_path.clone(),
                component_type: ComponentType::TxDecoder,
                gas_limit: 1_000_000,
            };

            // Load the component
            host.load_component("tx-decoder", &component_bytes, info)
                .unwrap();

            // Execute the component
            let result = host
                .execute_tx_decoder(
                    "tx-decoder",
                    "dGVzdA==", // base64 "test"
                    "base64",
                    false,
                )
                .unwrap();

            assert_eq!(result.exit_code, 0);
            assert!(result.data.is_some());
            assert!(result.error.is_none());

            println!("Component result: {:?}", result.data);
        } else {
            println!("Skipping component test - component not built");
        }
    }
}
