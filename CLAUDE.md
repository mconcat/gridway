# Claude Agent Guidelines for Helium Project

This document provides essential guidelines and best practices for AI agents working on the Helium blockchain project. Follow these guidelines to avoid common pitfalls and ensure smooth CI/CD operations.

## MANDATORY: Read Architecture Documentation First

**CRITICAL**: Before starting ANY task, you MUST:

1. **Always read the root PLAN.md** (`/PLAN.md`) to understand the overall architecture and design philosophy
2. **When working on a specific crate**, also read that crate's PLAN.md (e.g., `/crates/helium-baseapp/PLAN.md`)
3. **These documents contain critical architectural decisions** that affect all implementation work

The PLAN.md files contain:
- Core architectural vision (WASI microkernel, VFS, dynamic component loading)
- Component types and execution models  
- Design patterns and conventions
- Critical implementation details for each crate

Failure to read these documents will likely result in implementing code that conflicts with the architectural vision.

## Critical Commands to Run Before Committing

Always run these commands in order before committing any changes:

```bash
# 1. Build WASI modules (required before building other crates)
./scripts/build-wasi-modules.sh

# 2. Build all other crates
cargo build --workspace --exclude ante-handler --exclude begin-blocker --exclude end-blocker --exclude tx-decoder

# 3. Run all tests
cargo test --workspace --exclude ante-handler --exclude begin-blocker --exclude end-blocker --exclude tx-decoder

# 4. Check formatting 
cargo fmt --all

# 5. Run clippy
cargo clippy --fix --all --allow-dirty

# 6. If you fixed formatting issues, run formatter again
cargo fmt --all
```

If you have encountered any issues in one of these commands, and you have fixed them, run the commands again from the first to the last to ensure they new changes have not introduced any new issues.

### Building WASI Modules

The project contains WASI (WebAssembly System Interface) modules that must be built using `cargo-component` instead of regular `cargo build`. These modules are:
- `ante-handler` - Transaction validation
- `begin-blocker` - Block initialization  
- `end-blocker` - Block finalization
- `tx-decoder` - Transaction decoding

**Important**: Always use `./scripts/build-wasi-modules.sh` to build WASI modules. This script:
1. Installs the `wasm32-wasip1` target if needed
2. Builds all WASI modules using `cargo component build --release`
3. Copies the compiled `.wasm` files to the `modules/` directory

**Note**: Regular `cargo build --all` will fail on WASI modules with linking errors. This is expected - use the exclusion flags shown above.

## Environment-Specific Considerations

### Operating System Differences

- **Local Development**: macOS, Linux, or Windows
- **CI Environment**: Linux (Ubuntu on GitHub Actions)
- **Key Differences**:
  - Keychain/keyring access methods
  - File system behavior (case sensitivity, path separators)
  - System library availability

### Tests Requiring System Resources

When tests fail due to system resources, mark them appropriately:

```rust
#[test]
#[ignore = "OS keyring tests require system keychain access"]
async fn test_os_keyring_operations() {
    // test code
}
```

## Common CI Failure Patterns and Solutions

### 1. Format String Linting Errors

**Symptom**: "variables can be used directly in the `format!` string"

**Fix**:

```rust
// BAD
format!("Error: {}", msg)

// GOOD
format!("Error: {msg}")

// Field access is supported from Rust 1.58+
format!("Value: {obj.field}")
```

### 2. Test Expectation Mismatches

**Symptom**: Tests expecting success but getting errors

**Fix**: Update test expectations to match actual behavior:

```rust
// Instead of assuming success
assert_eq!(result.code, 0);

// Check for actual behavior
assert_eq!(result.code, 1);
assert!(result.log.contains("expected error message"));
```

## Best Practices for Testing

### 1. Environment-Aware Tests

```rust
fn should_skip_in_ci() -> bool {
    std::env::var("CI").is_ok()
}

#[test]
fn test_with_system_dependency() {
    if should_skip_in_ci() {
        println!("Skipping test in CI environment");
        return;
    }
    // actual test
}
```

### 2. Detailed Error Messages

```rust
// Provide context in assertions
assert_eq!(
    result.code, 
    expected_code,
    "Transaction failed with code {} (expected {}). Log: {}. Context: processing {}",
    result.code,
    expected_code,
    result.log,
    tx_type
);
```

### 3. Concurrent Code Testing

- Use shorter timeouts in tests
- Add explicit deadlock detection
- Test with different thread counts

## Workspace Configuration

### Profile Warnings

WASI modules may show profile warnings. These are expected but should ideally be fixed by moving profiles to workspace root:

```toml
# In workspace Cargo.toml, not individual crates
[profile.release]
opt-level = 3
```

## Merge Conflict Resolution Guidelines

### 1. Check Definitions and Usages First

**Principle**: Before resolving any conflict, always check the relevant definitions (traits, interfaces, types) and their usages across the codebase.

**Why**: Conflicts often arise from changes to fundamental definitions. Resolving implementation conflicts without checking the underlying definitions leads to compilation errors.

**How**:
- For method conflicts, check the trait/interface definition first
- For type conflicts, check where the type is defined and used
- For import conflicts, verify what's actually exported from the module

### 2. Resolve by Dependency Order

**Principle**: Understand the dependency graph of your crates/modules and resolve conflicts starting from the most foundational (least dependent) components.

**Why**: Higher-level crates depend on lower-level ones. Fixing conflicts in dependency order prevents cascading errors and repeated work.

**How**:
1. Identify crate dependencies (check `Cargo.toml` files)
2. Start with leaf crates (those that don't depend on other workspace crates)
3. After resolving conflicts in each crate, run `cargo build -p <crate-name>` to verify
4. Only move to dependent crates after dependencies compile successfully

**Example Order**:
```
helium-store (no workspace dependencies)
  ↓
helium-types (depends on store)
  ↓
helium-crypto (depends on types)
  ↓
helium-baseapp (depends on all above)
```

### 3. Ask When Uncertain

**Principle**: When multiple valid resolutions exist, ask for clarification rather than guessing.

**Why**: Architecture decisions, performance considerations, or project conventions often dictate the "correct" choice, which may not be obvious from the code alone.

**When to Ask**:
- Two approaches both work but have different implications
- The conflict involves architectural decisions
- You're unsure about project conventions or future direction

### 4. Maintain Consistency

**Principle**: When both conflicting approaches are valid, choose the one that maintains consistency with the existing codebase patterns.

**Why**: Consistency makes code more maintainable and reduces cognitive load for developers.

**How**:
- Check similar code in the project for patterns
- Prefer the approach used elsewhere in the codebase
- If introducing a new pattern, apply it consistently across all affected files

## Writing Style and Tone Guidelines

When writing documentation, proposals, or technical explanations, maintain a neutral, professional tone that focuses on technical substance rather than dramatic language.

### Language to Avoid

**Avoid overly dramatic or hyperbolic language**:
- "revolutionary" 
- "fundamentally changes"
- "groundbreaking"
- "paradigm shift"
- "transforms everything"
- "game-changing"

**Instead, use neutral, descriptive language**:
- "introduces"
- "enables"
- "provides"
- "implements"
- "allows"
- "supports"

### Writing Principles

**Keep it chill and neutral**:
- Focus on technical facts and benefits
- Avoid marketing-style language
- Use precise, descriptive terms
- Let the technical merit speak for itself

**Be concise**:
- Avoid unnecessary verbose explanations
- Get to the point quickly
- Use clear, direct language
- Remove redundant phrasing

**Stay technical**:
- Emphasize implementation details over grand visions
- Discuss concrete benefits rather than abstract concepts
- Focus on how things work, not how amazing they are
