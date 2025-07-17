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

## Tick-Tock Development Methodology

This project uses a tick-tock development methodology that alternates between two distinct phases. **Always detect your current stage before beginning work**.

### Stage Detection Commands

Run these commands to determine the current development stage:

```bash
# Method 1: Check current branch
git branch --show-current

# Method 2: Check environment variable
echo $HELIUM_DEVELOPMENT_STAGE

# Method 3: Check stage marker file
cat STAGE_MARKER.md 2>/dev/null || echo "No stage marker found"

# Method 4: Check for active stage branches
git branch -r | grep -E "(tick|tock)/current"
```

**Stage Determination Logic**:
- If on `tick/current` branch → **TICK STAGE**
- If on `tock/current` branch → **TOCK STAGE**
- If `HELIUM_DEVELOPMENT_STAGE=tick` → **TICK STAGE**
- If `HELIUM_DEVELOPMENT_STAGE=tock` → **TOCK STAGE**
- If `STAGE_MARKER.md` contains "tick" → **TICK STAGE**
- If `STAGE_MARKER.md` contains "tock" → **TOCK STAGE**

### TICK Stage (Implementation Velocity)

**When in TICK stage, you MUST follow these guidelines:**

#### Core Principles
- **Maximum Speed**: Prioritize working code over perfect code
- **Fast Merges**: Merge changes as quickly as possible
- **High Parallelization**: Work on multiple features simultaneously
- **Volume Over Clarity**: Focus on implementing features rather than documentation

#### Command Sequence (TICK)
```bash
# 1. Build WASI modules (required before building other crates)
./scripts/build-wasi-modules.sh

# 2. Build all other crates
cargo build --workspace --exclude ante-handler --exclude begin-blocker --exclude end-blocker --exclude tx-decoder

# 3. Run all tests - MUST PASS in tick stage
cargo test --workspace --exclude ante-handler --exclude begin-blocker --exclude end-blocker --exclude tx-decoder

# 4. Check formatting 
cargo fmt --all

# 5. Run clippy
cargo clippy --fix --all --allow-dirty

# 6. Final format check
cargo fmt --all
```

#### STRICTLY PROHIBITED in TICK
- **TODOs, FIXMEs, or XXX comments**: All code must be complete
- **Mock implementations**: No `todo!()`, `unimplemented!()`, or `panic!()` in production code
- **Placeholder code**: All functions must be fully implemented
- **Broken builds**: All builds and tests MUST pass
- **Extensive documentation**: Keep docs minimal and focused

#### Agent Behavior (TICK)
- **Aggressive Implementation**: Get features working quickly
- **Minimal Documentation**: Only essential comments
- **Fast Iteration**: Don't over-engineer solutions
- **Parallel Work**: Handle multiple tasks simultaneously
- **Merge Confidence**: Merge working code immediately

### TOCK Stage (Architectural Refinement)

**When in TOCK stage, you MUST follow these guidelines:**

#### Core Principles
- **Documentation First**: Comprehensive documentation is priority
- **Architectural Clarity**: Focus on system design and interfaces
- **Code Hygiene**: Refactor and clean up existing code
- **Rubber Duck Mode**: Act as thinking companion, not aggressive coder

#### Command Sequence (TOCK)
```bash
# 1. Build check (relaxed - failures permitted during refactoring)
cargo build --workspace --exclude ante-handler --exclude begin-blocker --exclude end-blocker --exclude tx-decoder || echo "Build failures permitted in tock"

# 2. Generate documentation
cargo doc --workspace --exclude ante-handler --exclude begin-blocker --exclude end-blocker --exclude tx-decoder --no-deps

# 3. Check documentation coverage
grep -r "///" --include="*.rs" crates/ | wc -l

# 4. Run tests (failures permitted during refactoring)
cargo test --workspace --exclude ante-handler --exclude begin-blocker --exclude end-blocker --exclude tx-decoder || echo "Test failures permitted in tock"

# 5. Check formatting 
cargo fmt --all

# 6. Run clippy (warnings acceptable)
cargo clippy --all --allow-dirty || echo "Clippy warnings acceptable in tock"
```

#### PERMITTED in TOCK
- **TODOs and FIXMEs**: For architectural planning
- **Mock implementations**: For architectural backbone (`todo!()`, `unimplemented!()`)
- **Broken builds**: Temporary failures during refactoring
- **Placeholder interfaces**: For system design
- **Extensive documentation**: Comprehensive docs are encouraged

#### Agent Behavior (TOCK)
- **Rubber Duck Companion**: Focus on understanding existing code
- **Architectural Thinking**: Consider system-wide implications
- **Documentation Priority**: Write comprehensive docs before code
- **Refactoring Focus**: Improve existing code structure
- **Slow and Deliberate**: Quality over speed

### Stage-Specific Merge Conflict Resolution

#### TICK Stage Conflicts
- **Speed First**: Choose the solution that works fastest
- **Minimal Disruption**: Avoid large refactors during conflicts
- **Build Stability**: Ensure resolution doesn't break builds
- **Feature Complete**: Prefer complete implementations over partial ones

#### TOCK Stage Conflicts
- **Architecture First**: Choose the solution that improves system design
- **Documentation Clarity**: Prefer well-documented approaches
- **Long-term Maintainability**: Consider future development needs
- **Interface Consistency**: Maintain clean API boundaries

### Stage Transition Guidelines

#### When to Transition from TICK to TOCK
- Agent efficiency drops significantly
- Build failures become frequent
- Code complexity reaches saturation
- ~30 days have passed

#### When to Transition from TOCK to TICK
- Documentation coverage >90%
- All interfaces are clean and documented
- Architecture refactoring is complete
- ~30 days have passed

### Emergency Overrides

If you encounter conflicts between tick-tock guidelines and critical project needs:
1. **Document the Override**: Explain why normal guidelines don't apply
2. **Minimize Impact**: Keep overrides as small as possible
3. **Return to Guidelines**: Resume normal stage behavior immediately after
4. **Report Override**: Note the override in commit messages

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
