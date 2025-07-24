//! Begin blocker component bindings

wasmtime::component::bindgen!({
    world: "begin-blocker-world",
    path: "../../wit",
});
