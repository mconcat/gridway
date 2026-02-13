//! Validator component bindings

wasmtime::component::bindgen!({
    world: "validator-world",
    path: "../../wit",
});
