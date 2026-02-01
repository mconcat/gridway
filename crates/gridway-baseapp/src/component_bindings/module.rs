//! Module component bindings

wasmtime::component::bindgen!({
    world: "module-world",
    path: "../../wit",
});
