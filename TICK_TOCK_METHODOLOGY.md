# Tick-Tock Development Methodology

## Overview

This document defines the tick-tock development methodology for the Helium project, inspired by Intel's processor design model. This methodology alternates between two distinct development phases to optimize agentic coding effectiveness while maintaining architectural integrity.

## Methodology Structure

### Tick Stage (Implementation Velocity)
**Duration**: ~30 days  
**Focus**: Maximum feature implementation and code density  
**Quality Gate**: All builds and tests MUST pass

#### Objectives
- Maximize parallelized agent development
- Prioritize code volume over clarity
- Add as many features as possible
- Merge changes as fast as possible

#### Agent Behavior
- **Aggressive Implementation**: Focus on getting features working quickly
- **Activity Documentation**: Record all work activities, decisions, and workarounds
- **Security First**: Never compromise on security checks and validation
- **Fast Iteration**: Prefer working code over perfect code
- **High Parallelization**: Multiple agents working simultaneously
- **Autopilot Mode**: Work autonomously with minimal user interaction

#### Rules and Restrictions
- **STRICTLY PROHIBITED**: Mock implementations, TODOs, or placeholders just to make builds pass
- **REQUIRED**: All code must be fully functional
- **REQUIRED**: All builds and tests must pass
- **REQUIRED**: No broken functionality in production code
- **REQUIRED**: Security checks and validation are NEVER omitted
- **REQUIRED**: Document all activities, decisions, and workarounds

#### End Conditions
- Agent efficiency drops below 50% baseline
- Build failure rate exceeds 30%
- Code complexity reaches saturation point
- Time to complete tasks increases exponentially

### Tock Stage (Architectural Refinement)
**Duration**: ~30 days  
**Focus**: Documentation, refactoring, and architectural clarity  
**Quality Gate**: Clean interfaces and comprehensive documentation

#### Objectives
- Write comprehensive documentation
- Perform architectural refactoring
- Improve code hygiene and maintainability
- Reduce complexity while preserving functionality

#### Agent Behavior
- **Conversational Mode**: Actively engage with user through back-and-forth dialogue
- **Architectural Thinking**: Consider system-wide implications
- **Documentation First**: Write comprehensive ADRs and distilled documentation
- **Refactoring Focus**: Improve existing code structure
- **User-Guided**: Follow user's architectural decisions and preferences
- **Less Agentic**: Work collaboratively rather than autonomously

#### Rules and Permissions
- **PROHIBITED**: Adding new features (except those required for architecture)
- **PERMITTED**: Mock code or TODO function bodies for architectural backbone
- **PERMITTED**: Temporary build failures during refactoring
- **REQUIRED**: Complete documentation for all public interfaces

#### End Conditions
- Documentation coverage >90%
- Interface definitions are clean and complete
- Code complexity reduced by >20%
- Architectural backbone is solid

## Stage Detection and Management

### Template-Based Stage Detection

Stage detection is determined by the content of the `CLAUDE.md` file. This file is generated from templates to show only relevant instructions for the current stage.

#### Template System Components

**Template Files**:
- `CLAUDE.md.template`: Master template with placeholders
- `tick-tock-content/common-content.md`: Common instructions for both stages
- `tick-tock-content/tick-content.md`: Tick-specific content
- `tick-tock-content/tock-content.md`: Tock-specific content

**Generation Scripts**:
- `scripts/generate-claude-md.py`: Python script to generate CLAUDE.md from template
- `scripts/tick-command.sh`: Command handler for switching to tick stage
- `scripts/tock-command.sh`: Command handler for switching to tock stage

#### Stage Detection Logic

The CI/CD system detects the current stage by reading the `CLAUDE.md` file content:

- **Tick Stage**: `CLAUDE.md` contains "You are currently in TICK STAGE"
- **Tock Stage**: `CLAUDE.md` contains "You are currently in TOCK STAGE"

### Stage Transition Management

**Manual Control**: Stage transitions are managed manually by human maintainers using the template generation system:

```bash
# Switch to tick stage
./scripts/tick-command.sh

# Switch to tock stage
./scripts/tock-command.sh
```

**Benefits**:
- Agents only see relevant instructions for current stage
- Reduced cognitive load with stage-specific guidance
- Easy to maintain and modify stage-specific content
- Automatic stage detection from generated content

## CI/CD Pipeline Configuration

### Tick Stage CI Pipeline
**File**: `.github/workflows/tick-ci.yml`

**Characteristics**:
- Strict build requirements
- Comprehensive test suite
- Fast feedback loops
- Parallel execution where possible
- No tolerance for build failures

**Quality Gates**:
- All builds must pass
- All tests must pass
- Code coverage maintained
- No TODOs or placeholders allowed
- Security checks must pass
- Clippy disabled to avoid blocking fast merges

### Tock Stage CI Pipeline
**File**: `.github/workflows/tock-ci.yml`

**Characteristics**:
- Relaxed build requirements during refactoring
- Documentation quality checks
- Architecture compliance validation
- Interface completeness verification

**Quality Gates**:
- Documentation coverage >90%
- All public interfaces documented
- Architecture compliance verified
- Refactoring objectives met

## Agent Guidelines by Stage

### Tick Stage Agent Instructions

#### Command Priority
1. Feature implementation speed
2. Security checks and validation
3. Test coverage maintenance
4. Build stability
5. Activity documentation

#### Merge Strategy
- Fast-forward merges preferred
- Frequent small commits
- Continuous integration
- Minimal review process for working code

#### Code Quality
- Functional code over perfect code
- Performance optimizations later
- Architecture compliance secondary
- Documentation minimal but accurate

### Tock Stage Agent Instructions

#### Command Priority
1. User interaction and dialogue
2. Code understanding and documentation
3. Architectural refactoring
4. Interface clarification
5. System-wide consistency

#### Merge Strategy
- Careful review of architectural changes
- Consolidated commits for major refactors
- Documentation updates with code changes
- Emphasis on system-wide impact

#### Code Quality
- Clarity over speed
- Comprehensive documentation
- Architectural consistency
- Long-term maintainability

## Metrics and Monitoring

### Tick Stage Metrics
- **Velocity**: Features implemented per day
- **Quality**: Build success rate, test pass rate
- **Efficiency**: Agent task completion time
- **Volume**: Lines of code, commits per day

### Tock Stage Metrics
- **Documentation**: Coverage percentage, interface completeness
- **Architecture**: Complexity reduction, dependency clarity
- **Quality**: Code maintainability index, technical debt reduction
- **Clarity**: Interface design quality, system understanding

### Transition Metrics
- **Saturation Points**: When tick efficiency drops
- **Clarity Thresholds**: When tock objectives are met
- **Quality Gates**: Build health, test coverage
- **Cycle Health**: Overall methodology effectiveness

## Implementation Guidelines

### SCRATCHPAD.md Process

**MANDATORY**: All feature branch work must maintain a `SCRATCHPAD.md` file as a scratchpad for current work.

#### Process Flow
1. **Start of work**: Create `SCRATCHPAD.md` with branch objectives
2. **During work**: Continuously update with progress, decisions, and issues
3. **PR creation**: Create PR as DRAFT while `SCRATCHPAD.md` exists
4. **CI verification**: Ensure all CI checks pass
5. **Ready for review**: Delete `SCRATCHPAD.md` and mark PR as ready for review

#### SCRATCHPAD.md Template

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

### Initial Setup
1. Choose starting stage based on current codebase state
2. Create `STAGE_MARKER.md` with current stage
3. Configure CI pipeline for chosen stage
4. Brief development team on stage-specific guidelines

### Stage Transitions
1. Monitor transition criteria continuously
2. Evaluate readiness for stage change
3. Archive current stage branch
4. Create new stage branch
5. Update CI configuration
6. Update stage markers and documentation

### Continuous Improvement
1. Track methodology effectiveness metrics
2. Adjust stage durations based on project needs
3. Refine transition criteria based on experience
4. Update agent guidelines based on lessons learned

## Success Criteria

### Overall Methodology Success
- Improved agentic coding effectiveness
- Maintained architectural integrity
- Reduced technical debt accumulation
- Faster feature delivery with better quality

### Stage-Specific Success
- **Tick**: High feature velocity with stable builds
- **Tock**: Clear architecture with comprehensive documentation
- **Transitions**: Smooth handoffs between stages
- **Cycles**: Continuous improvement in development efficiency

## Troubleshooting

### Common Issues
- **Stuck in Tick**: Agents becoming ineffective but not transitioning
- **Stuck in Tock**: Over-documentation without clear completion criteria
- **Transition Friction**: Difficulty switching between stage mindsets
- **Quality Degradation**: Builds failing or tests becoming unreliable

### Resolution Strategies
- **Manual Override**: Allow manual stage transitions when automated criteria fail
- **Hybrid Periods**: Short transitional periods with mixed characteristics
- **Escalation Procedures**: Clear guidelines for when to break methodology rules
- **Feedback Loops**: Regular methodology effectiveness reviews

## Conclusion

The tick-tock methodology provides a structured approach to managing the inherent tension between rapid feature development and architectural clarity in agentic coding projects. By alternating between focused implementation and deliberate refactoring phases, teams can maintain both velocity and quality over time.

The key to success is disciplined adherence to stage-specific guidelines while maintaining flexibility to adapt the methodology based on project-specific needs and lessons learned.