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