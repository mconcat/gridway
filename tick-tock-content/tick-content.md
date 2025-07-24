## Tick-Tock Development Methodology

**You are currently in TICK STAGE** - Implementation Velocity Phase

This project uses a tick-tock development methodology that alternates between two distinct phases. You are currently in the **TICK** stage, which focuses on maximum velocity and feature implementation.

### TICK Stage (Implementation Velocity)

**When in TICK stage, you MUST follow these guidelines:**

#### Core Principles
- **Maximum Speed**: Prioritize working code over perfect code
- **Fast Merges**: Merge changes as quickly as possible
- **High Parallelization**: Work on multiple features simultaneously
- **Security First**: Security checks are NEVER omitted, even in tick stage
- **Activity Documentation**: Document all work, activities, feedback, and workarounds

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

# 5. Final format check
cargo fmt --all

# Note: Clippy is disabled in tick stage to avoid blocking fast merges
```

#### STRICTLY PROHIBITED in TICK
- **TODOs, FIXMEs, or XXX comments**: All code must be complete
- **Mock implementations**: No `todo!()`, `unimplemented!()`, or `panic!()` in production code
- **Placeholder code**: All functions must be fully implemented
- **Broken builds**: All builds and tests MUST pass
- **Security vulnerabilities**: Security checks are NEVER omitted

#### Agent Behavior (TICK)
- **Aggressive Implementation**: Get features working quickly
- **Activity Documentation**: Record all work activities, feedback, and workarounds
- **Security Vigilance**: Always perform security checks and validation
- **Fast Iteration**: Don't over-engineer solutions
- **Parallel Work**: Handle multiple tasks simultaneously
- **Merge Confidence**: Merge working code immediately
- **Autopilot Mode**: Work with minimal user interaction, making autonomous decisions

### Documentation Approach (TICK Stage)
- **Focus**: Activity recording and practical documentation
- **Content**: Record all work activities, decisions, workarounds, and feedback
- **Style**: Practical, concise, focused on "what was done and why"
- **Examples**: 
  - Work logs in SCRATCHPAD.md
  - Decision records for implementation choices
  - Workaround documentation
  - Feedback from testing and debugging
- **Purpose**: Valuable input for tock stage refinement

### Emergency Overrides

If you encounter conflicts between tick-tock guidelines and critical project needs:
1. **Document the Override**: Explain why normal guidelines don't apply
2. **Minimize Impact**: Keep overrides as small as possible
3. **Return to Guidelines**: Resume normal stage behavior immediately after
4. **Report Override**: Note the override in commit messages