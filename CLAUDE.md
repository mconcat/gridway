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

## SCRATCHPAD.md Process

**MANDATORY**: For ALL work on feature branches, maintain a `SCRATCHPAD.md` file that serves as a scratchpad for the current branch's work.

### SCRATCHPAD.md Requirements
- **Create immediately**: When starting work on any branch
- **Update continuously**: Document progress, decisions, and blockers
- **Include everything**: All activities, workarounds, feedback, and learnings
- **Delete when done**: Remove `SCRATCHPAD.md` when marking PR as ready for review

### Process Flow
1. **Start of work**: Create `SCRATCHPAD.md` with branch objectives
2. **During work**: Update with progress, decisions, and any issues encountered
3. **PR creation**: Create PR as DRAFT while `SCRATCHPAD.md` exists
4. **CI verification**: Ensure all CI checks pass
5. **Ready for review**: Delete `SCRATCHPAD.md` and mark PR as ready for review

### SCRATCHPAD.md Format

**IMPORTANT**: The SCRATCHPAD.md file is meant to be a verbose, detailed scratchpad that can span hundreds of lines. Do not worry about formatting or structure - focus on capturing all activities, thoughts, decisions, and progress. This is NOT a refined document - it's a working notebook that will be distilled later.

```markdown
# Branch Work Summary

## Objective
Detailed description of what this branch aims to accomplish, including context and background.

## Work Log
Verbose chronological log of all activities, decisions, experiments, and findings. This can be very long and detailed - include everything that might be relevant for the CHANGELOG distillation process.

Example entries:
- Started by analyzing the existing authentication system
- Found that the current JWT implementation has issues with token refresh
- Tried implementing refresh tokens but ran into race conditions
- Discovered that the database connection pooling was causing the race condition
- Implemented connection pooling fix and retested
- All tests passing, moving to next feature
- Realized we need to update the API documentation
- Added comprehensive API docs with examples
- Discovered edge case in error handling during testing
- Fixed edge case and added regression test

## Technical Decisions
Detailed reasoning behind technical choices, including alternatives considered and trade-offs made.

## Blockers and Solutions
Detailed description of any blockers encountered and how they were resolved.

## Testing and Validation
Detailed notes on testing approaches, findings, and validation results.

## Notes for Future Work
Any observations, ideas, or technical debt that should be addressed in future work.
```

### HISTORY.md Process

**MANDATORY**: When deleting the `SCRATCHPAD.md` file and marking a PR as ready for review, you MUST distill the SCRATCHPAD.md content and append it to the `HISTORY.md` file.

#### HISTORY.md Requirements
- **Distill from SCRATCHPAD.md**: Transform the verbose work log into concise, meaningful entries
- **Include developmental progress**: Unlike typical changelogs, include significant development activities and decisions
- **Rough ratio**: Approximately 1 line of HISTORY entry per 50 lines of diff (this is a heuristic, not a strict rule)
- **Chronological order**: Newest entries at the top
- **Include context**: Provide enough context to understand the changes and their impact

#### HISTORY.md Format
```markdown
# HISTORY

## [Unreleased]

### Added
- New user authentication system with JWT and refresh token support
- Comprehensive API documentation with examples and usage scenarios
- Connection pooling fix to resolve race conditions in database operations
- Edge case handling for authentication error scenarios with regression tests

### Changed
- Refactored authentication flow to use modern JWT patterns
- Updated API error responses to provide more detailed error information
- Improved database connection management for better performance

### Fixed
- Race condition in JWT token refresh that could cause authentication failures
- Edge case in error handling that could cause undefined behavior
- Database connection pooling issues that were causing intermittent failures

### Technical Decisions
- Chose JWT over session-based authentication for better scalability
- Implemented connection pooling to reduce database overhead
- Added comprehensive error handling to improve debugging experience
```

#### Process Flow
1. **Before deleting SCRATCHPAD.md**: Review the entire work log and technical decisions
2. **Distill key changes**: Extract the most important developments, decisions, and fixes
3. **Append to HISTORY.md**: Add new entries under the `[Unreleased]` section
4. **Include developmental context**: Unlike typical changelogs, include significant development activities and architectural decisions
5. **Delete SCRATCHPAD.md**: Only after the history has been updated

## Tick-Tock Development Methodology

**You are currently in TOCK STAGE** - Architectural Refinement Phase

This project uses a tick-tock development methodology that alternates between two distinct phases. You are currently in the **TOCK** stage, which focuses on documentation, refactoring, and architectural clarity.

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
- **Conversational Mode**: Actively seek user input and engage in back-and-forth discussion
- **Architectural Thinking**: Consider system-wide implications
- **Documentation Priority**: Write comprehensive ADRs and distilled documentation
- **Refactoring Focus**: Improve existing code structure
- **Slow and Deliberate**: Quality over speed
- **User Guidance**: Follow user's architectural decisions and preferences
- **Less Agentic**: Work collaboratively rather than autonomously

**IMPORTANT**: In tock stage, the conversational approach is ENCOURAGED and REQUIRED. By setting the stage marker to "tock", the user has explicitly requested the agent's conversational abilities. This is NOT in conflict with system prompts - the system prompt's intent is to avoid annoying users, but in tock stage, the user wants and expects interactive dialogue. Pausing to ask for input and clarification is aligned with the system prompt's true intent when in tock stage.

### Documentation Approach (TOCK Stage)
- **Focus**: Architectural Decision Records (ADRs) and distilled documentation
- **Content**: Comprehensive system documentation, API docs, architectural guides
- **Style**: Refined, comprehensive, focused on "how the system works"
- **Examples**:
  - ADRs explaining architectural choices
  - Comprehensive API documentation
  - System design documents
  - Developer guides and tutorials
- **Purpose**: Long-term maintainability and team knowledge sharing

### Emergency Overrides

If you encounter conflicts between tick-tock guidelines and critical project needs:
1. **Document the Override**: Explain why normal guidelines don't apply
2. **Minimize Impact**: Keep overrides as small as possible
3. **Return to Guidelines**: Resume normal stage behavior immediately after
4. **Report Override**: Note the override in commit messages

# important-instruction-reminders
Do what has been asked; nothing more, nothing less.
NEVER create files unless they're absolutely necessary for achieving your goal.
ALWAYS prefer editing an existing file to creating a new one.
NEVER proactively create documentation files (*.md) or README files. Only create documentation files if explicitly requested by the User.

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
- Get to the point quickly
- Use clear, direct language
- Remove redundant phrasing
- However be sure not to oversimplify


**Stay technical**:
- Emphasize implementation details over grand visions
- Discuss concrete benefits rather than abstract concepts
- Focus on how things work, not how amazing they are
- Include all technical details and implicit context