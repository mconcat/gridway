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

### Automatic Stage Detection

Stage detection is determined ONLY by the stage marker file. Branch names and environment variables are NOT used.

#### Stage Marker File
Create `STAGE_MARKER.md` in repository root:
```markdown
# Current Development Stage

**Stage**: tick  
**Started**: 2024-01-15  
**Expected End**: 2024-02-15  
**Cycle**: 2024-Q1-01  

## Stage Objectives
- Implement user authentication system
- Add transaction processing
- Build REST API endpoints
```

**Important**: Only the `STAGE_MARKER.md` file determines the current stage. This allows for standard branch naming conventions (feat/, fix/, etc.) while maintaining stage-specific behavior.

### Stage Transition Criteria

#### Tick → Tock Transition
**Automated Triggers**:
- Agent efficiency metrics drop >50%
- Build failure rate exceeds 30% over 3 days
- Average task completion time increases >3x baseline
- Code complexity metrics reach saturation

**Manual Triggers**:
- All planned features for cycle are implemented
- Technical debt accumulation is too high
- Architecture needs redesign

#### Tock → Tick Transition
**Automated Triggers**:
- Documentation coverage >90%
- All public interfaces documented
- Code complexity reduced >20%
- Architectural refactoring complete

**Manual Triggers**:
- Architecture documentation is complete
- Development team confident in system clarity
- Ready to add new features efficiently

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

### SUMMARY.md Process

**MANDATORY**: All feature branch work must maintain a `SUMMARY.md` file as a scratchpad for current work.

#### Process Flow
1. **Start of work**: Create `SUMMARY.md` with branch objectives
2. **During work**: Continuously update with progress, decisions, and issues
3. **PR creation**: Create PR as DRAFT while `SUMMARY.md` exists
4. **CI verification**: Ensure all CI checks pass
5. **Ready for review**: Delete `SUMMARY.md` and mark PR as ready for review

#### SUMMARY.md Template
```markdown
# Branch Work Summary

## Objective
Brief description of branch goals

## Progress
- [x] Completed tasks
- [ ] Pending tasks

## Decisions Made
- Key decisions with reasoning

## Blockers/Issues
- Current issues and their status

## Learnings/Notes
- Important findings and workarounds
```

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