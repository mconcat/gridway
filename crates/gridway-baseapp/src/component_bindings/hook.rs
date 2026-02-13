//! Hook component bindings

wasmtime::component::bindgen!({
    world: "hook-world",
    path: "../../wit",
});
