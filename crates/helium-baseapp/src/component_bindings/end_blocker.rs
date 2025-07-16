//! End blocker component bindings

wasmtime::component::bindgen!({
    world: "end-blocker-world",
    path: "../../wit",
});
