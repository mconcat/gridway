//! Ante handler component bindings

wasmtime::component::bindgen!({
    world: "ante-handler-world",
    path: "../../wit",
});
