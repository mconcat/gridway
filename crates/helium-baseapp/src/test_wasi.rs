// Simple test to debug WASI execution
use wasmtime::*;
use wasmtime_wasi::{preview1::add_to_linker_sync, WasiCtxBuilder};

#[test]
fn test_simple_wasi_execution() {
    // Create a simple WASM module inline
    let wat = r#"
        (module
            (func $hello (export "hello") (result i32)
                i32.const 42
            )
        )
    "#;

    let wasm = wat::parse_str(wat).unwrap();

    // Create engine and store
    let engine = Engine::default();
    let mut linker = Linker::new(&engine);

    // Create WASI context
    let wasi = WasiCtxBuilder::new().build_p1();
    let mut store = Store::new(&engine, wasi);

    // Add WASI to linker
    add_to_linker_sync(&mut linker, |ctx| ctx).unwrap();

    // Create module and instance
    let module = Module::new(&engine, &wasm).unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();

    // Get and call the function
    let hello = instance
        .get_typed_func::<(), i32>(&mut store, "hello")
        .unwrap();
    let result = hello.call(&mut store, ()).unwrap();

    assert_eq!(result, 42);
    println!("Simple WASI test passed!");
}
