# Claude Agent Guidelines for Gridway Project

**Last Updated:** 2025-02-10

This document provides guidelines for AI agents working on the Gridway blockchain project. Follow these to avoid common pitfalls and ensure smooth development.

## MANDATORY: Read Architecture Documentation First

Before starting any task:

1. Read the root `PLAN.md` for overall architecture and design philosophy
2. When working on a specific crate, read its `PLAN.md` if one exists (e.g., `crates/gridway-baseapp/PLAN.md`)
3. These documents contain architectural decisions that affect all implementation work

## Project Overview

Gridway is a WASM microkernel blockchain using Commonware Simplex consensus. Key facts:

- **Branch:** `experiment/commonware-migration`
- **Consensus:** Commonware Library v0.0.65 (Simplex BFT), not CometBFT/ABCI
- **All application logic runs in WASM** — no Rust-native fallback for modules
- **State access:** WASM → kvstore WIT → VFS → NamespacedStore → MerkleStore (Patricia Merkle Trie)
- **TX signing:** Ed25519 (commonware-cryptography)
- **Consensus signing:** BLS12-381 threshold signatures
- **No Cosmos SDK compatibility** — Gridway defines its own formats and APIs

## Crate Structure

```
gridway (root)
├── crates/
│   ├── gridway-consensus/     — Commonware Application/Engine/node binary
│   ├── gridway-baseapp/       — WASM microkernel host (ComponentHost, VFS, capabilities)
│   ├── gridway-store/         — MerkleStore (trie-db), GlobalAppStore, NamespacedStore
│   ├── gridway-types/         — GridwayBlock, SignedTx, TxResponse, Event
│   ├── gridway-crypto/        — Ed25519 signing, SHA-256, address derivation
│   ├── gridway-errors/        — Error types
│   ├── gridway-log/           — Logging/tracing
│   ├── gridway-math/          — Numeric types (Dec, Int)
│   ├── gridway-telemetry/     — Metrics
│   └── wasi-modules/          — WASM component source code
│       ├── bank/              — Bank module (MsgSend)
│       ├── validator/         — TX validation (ed25519, sequence)
│       ├── hook/              — Block lifecycle hooks
│       ├── ante-handler/      — Legacy (superseded by validator)
│       ├── begin-blocker/     — Legacy (superseded by hook)
│       ├── end-blocker/       — Legacy (superseded by hook)
│       ├── tx-decoder/        — Legacy (superseded by validator)
│       └── test-minimal/      — Test component
├── wit/                       — WIT interface definitions (module, kvstore, validator, hook)
└── modules/                   — Compiled .wasm binaries
```

### Dependency Order (build from leaves up)

```
gridway-math, gridway-errors, gridway-log, gridway-telemetry  (no workspace deps)
  ↓
gridway-crypto  (commonware-cryptography)
  ↓
gridway-store  (trie-db, no workspace deps)
  ↓
gridway-types  (gridway-math, gridway-crypto, commonware-codec/consensus)
  ↓
gridway-baseapp  (gridway-store, gridway-types, gridway-crypto, gridway-telemetry, wasmtime)
  ↓
gridway-consensus  (gridway-baseapp, gridway-types, gridway-crypto, gridway-store, commonware-*)
```

## Build Commands

### Build WASI modules (must be done first if modules changed)

```bash
# Build WASM components (requires cargo-component)
./scripts/build-wasi-modules.sh

# Or build individual new-generation modules:
cd crates/wasi-modules/bank && cargo component build --release
cd crates/wasi-modules/hook && cargo component build --release
cd crates/wasi-modules/validator && cargo component build --release
```

### Build workspace (excluding WASM modules)

```bash
cargo build --workspace \
  --exclude ante-handler --exclude begin-blocker --exclude end-blocker \
  --exclude tx-decoder --exclude test-minimal --exclude bank \
  --exclude hook --exclude validator
```

### Run tests

```bash
cargo test --workspace \
  --exclude ante-handler --exclude begin-blocker --exclude end-blocker \
  --exclude tx-decoder --exclude test-minimal --exclude bank \
  --exclude hook --exclude validator
```

### Full pre-commit sequence

```bash
# 1. Build WASI modules (if changed)
./scripts/build-wasi-modules.sh

# 2. Build workspace
cargo build --workspace \
  --exclude ante-handler --exclude begin-blocker --exclude end-blocker \
  --exclude tx-decoder --exclude test-minimal --exclude bank \
  --exclude hook --exclude validator

# 3. Run tests
cargo test --workspace \
  --exclude ante-handler --exclude begin-blocker --exclude end-blocker \
  --exclude tx-decoder --exclude test-minimal --exclude bank \
  --exclude hook --exclude validator

# 4. Format
cargo fmt --all

# 5. Clippy
cargo clippy --fix --all --allow-dirty \
  --exclude ante-handler --exclude begin-blocker --exclude end-blocker \
  --exclude tx-decoder --exclude test-minimal --exclude bank \
  --exclude hook --exclude validator

# 6. Re-format after clippy fixes
cargo fmt --all
```

**Note:** WASM module crates (wasi-modules/*) must be excluded from `cargo build/test/clippy` because they target `wasm32-wasip1` and will fail with linking errors under the host target. Use `cargo component build` for these.

### Build node binary

```bash
cargo build --release -p gridway-consensus --bin gridway-node
cargo build --release -p gridway-consensus --bin gridway-setup
cargo build --release -p gridway-consensus --bin gridway-keygen
```

## Key Files to Know

| File | Purpose |
|------|---------|
| `crates/gridway-baseapp/src/lib.rs` | BaseApp — WASM microkernel host, execution pipeline |
| `crates/gridway-baseapp/src/component_host.rs` | ComponentHost — wasmtime WASM component runtime |
| `crates/gridway-baseapp/src/vfs.rs` | VFS — virtual filesystem for state access |
| `crates/gridway-consensus/src/application.rs` | GridwayApp — Commonware Application trait impl |
| `crates/gridway-consensus/src/engine.rs` | Consensus engine wiring (broadcast, marshal, simplex) |
| `crates/gridway-consensus/src/bin/gridway_node.rs` | Validator node binary |
| `crates/gridway-store/src/merkle.rs` | MerkleStore — Patricia Merkle Trie |
| `crates/gridway-store/src/global.rs` | GlobalAppStore — namespaced store views |
| `crates/gridway-types/src/block.rs` | GridwayBlock — consensus block type |
| `crates/gridway-types/src/tx.rs` | SignedTx, MsgSend, TxResponse |
| `crates/wasi-modules/bank/src/lib.rs` | WASM bank module |
| `crates/wasi-modules/validator/src/lib.rs` | WASM TX validator |
| `crates/wasi-modules/hook/src/lib.rs` | WASM block hooks |
| `wit/*.wit` | WIT interface definitions |

## Architecture Principles

1. **WASM-only module execution:** All application logic (validation, bank, hooks) runs in WASM. No Rust-native fallback. If a WASM module fails to load, the operation errors.

2. **State through VFS:** WASM modules access state only through the kvstore WIT interface → VFS → MerkleStore pipeline. Direct state access from WASM is not allowed.

3. **Deterministic execution:** Same block with same TXs must produce the same state root. The Patricia Merkle Trie ensures this. All WASM execution must be deterministic.

4. **Microkernel boundary:** BaseApp is the kernel. It loads modules, routes messages, manages state. Modules are isolated — they can only access the namespaces granted to them.

5. **Commonware integration:** Follow Alto's patterns for consensus engine wiring. Use the same component structure (broadcast, marshal, simplex, resolver).

## Common Pitfalls

1. **Don't build WASM modules with `cargo build`** — they need `cargo component build` targeting wasm32-wasip1
2. **Don't add Cosmos/CometBFT dependencies** — the project has migrated away from Cosmos SDK
3. **Don't create Rust-native module fallbacks** — the architecture requires WASM modules
4. **Don't skip WASM module rebuild** — if you change a .wit file or WASM module source, rebuild with `./scripts/build-wasi-modules.sh`
5. **Remember to exclude WASM crates** from workspace cargo commands

## Testing Tips

- Tests that use WASM modules need the compiled `.wasm` files in the `modules/` directory
- `test_full_wasm_tx_pipeline` is the comprehensive end-to-end test
- `test_deterministic_hash` verifies state root determinism
- `test_export_import_snapshot` verifies state sync capability
- Run individual crate tests: `cargo test -p gridway-baseapp`

## Writing Style and Tone Guidelines

When writing documentation, proposals, or technical explanations:

**Keep it neutral and technical:**
- Focus on technical facts and implementation details
- Use precise, descriptive language
- Let the technical merit speak for itself

**Avoid:**
- Dramatic language ("revolutionary", "groundbreaking", "paradigm shift")
- Marketing-style superlatives
- Vague claims without evidence

**Prefer:**
- Concrete descriptions ("implements", "enables", "provides")
- Specific references to code, tests, and measurements
- Honest assessment of what works and what doesn't

## Merge Conflict Resolution

1. Check definitions (traits, interfaces, types) before resolving implementation conflicts
2. Resolve in dependency order (store → types → baseapp → consensus)
3. After each crate fix, run `cargo build -p <crate>` to verify
4. When uncertain about which approach to take, ask
