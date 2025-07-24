//! Tx decoder component bindings

wasmtime::component::bindgen!({
    world: "tx-decoder-world",
    path: "../../wit",
});
