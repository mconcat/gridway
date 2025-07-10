// Test specifically for our WASI modules
use crate::wasi_host::{WasiHost, ExecutionResult};

#[test]
fn test_our_minimal_module() {
    let wasi_host = WasiHost::new().unwrap();
    
    // Load our test_minimal module
    let module_path = std::env::current_dir()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("modules/test_minimal.wasm");
        
    if !module_path.exists() {
        eprintln!("Module not found at: {:?}", module_path);
        return;
    }
    
    let wasm_bytes = std::fs::read(&module_path).unwrap();
    
    // Try direct execution with minimal setup
    use wasmtime::*;
    use wasmtime_wasi::{WasiCtxBuilder, preview1::add_to_linker_sync};
    
    let engine = Engine::default();
    let mut linker = Linker::new(&engine);
    
    // Create minimal WASI context
    let wasi = WasiCtxBuilder::new().build_p1();
    let mut store = Store::new(&engine, wasi);
    
    // Add WASI to linker
    add_to_linker_sync(&mut linker, |ctx| ctx).unwrap();
    
    // Create module
    let module = Module::new(&engine, &wasm_bytes).unwrap();
    
    // List exports to debug
    println!("Module exports:");
    for export in module.exports() {
        println!("  {} ({:?})", export.name(), export.ty());
    }
    
    // Try to instantiate
    match linker.instantiate(&mut store, &module) {
        Ok(instance) => {
            println!("Instance created successfully");
            
            // Try to get test_simple function
            if let Ok(func) = instance.get_typed_func::<(), i32>(&mut store, "test_simple") {
                println!("Found test_simple function");
                match func.call(&mut store, ()) {
                    Ok(result) => println!("test_simple returned: {}", result),
                    Err(e) => println!("Error calling test_simple: {}", e),
                }
            } else {
                println!("test_simple function not found");
            }
        }
        Err(e) => {
            println!("Failed to instantiate module: {}", e);
        }
    }
}