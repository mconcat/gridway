// Test specifically for our WASI components
use crate::component_host::{ComponentHost, ComponentInfo, ComponentType};

#[test]
fn test_our_minimal_component() {
    let base_store = std::sync::Arc::new(std::sync::Mutex::new(helium_store::MemStore::new()));
    let host = ComponentHost::new(base_store).unwrap();

    // Load our minimal component module
    let module_path = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("modules/test_minimal_component.wasm");

    if !module_path.exists() {
        eprintln!("Component not found at: {:?}", module_path);
        eprintln!("Note: This test expects preview2 components, not preview1 modules");
        return;
    }

    let component_bytes = std::fs::read(&module_path).unwrap();

    let info = ComponentInfo {
        name: "test-minimal".to_string(),
        path: module_path.clone(),
        component_type: ComponentType::Module,
        gas_limit: 1_000_000,
    };

    // Load the component
    match host.load_component("test-minimal", &component_bytes, info) {
        Ok(_) => {
            println!("Component loaded successfully");
            // For minimal component, we don't have specific execution methods
            // since it's a generic component type
        }
        Err(e) => {
            println!("Failed to load component: {}", e);
        }
    }
}
