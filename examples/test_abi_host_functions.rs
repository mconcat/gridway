//! Example demonstrating host function integration with VFS
//!
//! This example shows how WASM modules can access blockchain state
//! through the host functions that connect to the VirtualFilesystem.

use helium_baseapp::{
    abi::AbiContext,
    capabilities::{CapabilityManager, CapabilityType},
    vfs::{Capability as VfsCapability, VirtualFilesystem},
};
use helium_store::MemStore;
use std::sync::{Arc, Mutex};

fn main() {
    println!("=== Host Function Integration Example ===\n");

    // Set up VFS
    let vfs = Arc::new(VirtualFilesystem::new());
    let test_store = Arc::new(Mutex::new(MemStore::new()));

    // Mount store for module namespace
    vfs.mount_store("test_module".to_string(), test_store.clone())
        .unwrap();

    // Add VFS capabilities
    vfs.add_capability(VfsCapability::Read("test_module".to_string().into()))
        .unwrap();
    vfs.add_capability(VfsCapability::Write("test_module".to_string().into()))
        .unwrap();
    // Note: Create capability is not available in current VFS implementation
    // vfs.add_capability(VfsCapability::Create("test_module".to_string().into()))
    //     .unwrap();

    println!("✓ VFS initialized with test_module namespace");

    // Set up CapabilityManager
    let cap_manager = Arc::new(CapabilityManager::new());

    // Grant capabilities to module
    cap_manager
        .grant_capability(
            "test_module",
            CapabilityType::ReadState("test_module".to_string()),
            "system",
            true,
        )
        .unwrap();

    cap_manager
        .grant_capability(
            "test_module",
            CapabilityType::WriteState("test_module".to_string()),
            "system",
            true,
        )
        .unwrap();

    println!("✓ Capabilities granted to test_module");

    // Create ABI context
    let mut context = AbiContext::new("test_module".to_string(), vec![]);
    context.set_vfs(vfs.clone());
    context.set_capability_manager(cap_manager.clone());

    println!("\n=== Testing State Access Through VFS ===\n");

    // Demonstrate write operation
    let key = "account:cosmos1abc...";
    let value = r#"{"balance": 1000, "sequence": 5}"#;

    // Write through VFS directly (simulating what host_state_set would do)
    let path = format!("/state/test_module/{key}");
    let fd = vfs.create(std::path::Path::new(&path)).unwrap();
    vfs.write(fd, value.as_bytes()).unwrap();
    vfs.close(fd).unwrap();

    println!("✓ Wrote state: {key} = {value}");

    // Read through VFS (simulating what host_state_get would do)
    let fd = vfs.open(std::path::Path::new(&path), false).unwrap();
    let mut buffer = vec![0u8; 1024];
    let bytes_read = vfs.read(fd, &mut buffer).unwrap();
    vfs.close(fd).unwrap();

    let read_value = String::from_utf8_lossy(&buffer[..bytes_read]);
    println!("✓ Read state: {key} = {read_value}");

    // Demonstrate capability checking
    println!("\n=== Testing Capability System ===\n");

    let has_read = cap_manager
        .has_capability(
            "test_module",
            &CapabilityType::ReadState("test_module".to_string()),
        )
        .unwrap();
    println!("✓ Module has read capability: {has_read}");

    let has_forbidden = cap_manager
        .has_capability(
            "test_module",
            &CapabilityType::WriteState("forbidden".to_string()),
        )
        .unwrap();
    println!("✓ Module has forbidden write capability: {has_forbidden}");

    println!("\n=== Summary ===\n");
    println!("Host functions are now connected to:");
    println!("- VirtualFilesystem for state access via file operations");
    println!("- CapabilityManager for fine-grained permission control");
    println!("- WASM modules can read/write state through standard WASI file operations");
    println!("\nThis enables the microkernel architecture where modules are isolated");
    println!("and can only access state they have capabilities for.");
}
