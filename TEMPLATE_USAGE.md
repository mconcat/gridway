# Template-Based Tick-Tock Methodology Usage

This document explains how to use the template-based tick-tock development methodology system.

## Overview

The tick-tock methodology uses a template system to generate stage-specific `CLAUDE.md` files. This ensures that agents only see instructions relevant to the current development stage, improving focus and reducing cognitive load.

## System Components

### 1. Template Files
- **`CLAUDE.md.template`**: Master template with placeholders
- **`tick-tock-content/common-content.md`**: Common instructions for both stages
- **`tick-tock-content/tick-content.md`**: Tick-specific content
- **`tick-tock-content/tock-content.md`**: Tock-specific content

### 2. Generation Scripts
- **`scripts/generate-claude-md.py`**: Python script to generate CLAUDE.md from template
- **`scripts/tick-command.sh`**: Slash command handler for switching to tick stage
- **`scripts/tock-command.sh`**: Slash command handler for switching to tock stage

## Usage

### Switching to Tick Stage

```bash
# Using slash command handler
./scripts/tick-command.sh

# Or directly with Python script
python3 scripts/generate-claude-md.py tick
```

### Switching to Tock Stage

```bash
# Using slash command handler
./scripts/tock-command.sh

# Or directly with Python script
python3 scripts/generate-claude-md.py tock
```

### Custom Slash Commands (Future)

The system is designed to work with Claude Code custom slash commands:

```
/tick    # Switch to tick stage
/tock    # Switch to tock stage
```

## Stage Detection

The CI/CD system detects the current stage by reading the `CLAUDE.md` file content:

- **Tick Stage**: `CLAUDE.md` contains "You are currently in TICK STAGE"
- **Tock Stage**: `CLAUDE.md` contains "You are currently in TOCK STAGE"

## Template Structure

The template system uses simple placeholder replacement:

```markdown
{{COMMON_CONTENT}}    # Replaced with common-content.md
{{STAGE_CONTENT}}     # Replaced with tick-content.md or tock-content.md
```

## Benefits

1. **Focus**: Agents only see relevant instructions for current stage
2. **Clarity**: Reduced cognitive load with stage-specific guidance
3. **Maintainability**: Easy to update stage-specific content
4. **Flexibility**: Simple to add new stages or modify existing ones
5. **CI Integration**: Automatic stage detection from generated content

## Maintenance

### Adding New Content

1. **Common to both stages**: Edit `tick-tock-content/common-content.md`
2. **Tick-specific**: Edit `tick-tock-content/tick-content.md`
3. **Tock-specific**: Edit `tick-tock-content/tock-content.md`

### Modifying Template

Edit `CLAUDE.md.template` to change the overall structure or add new placeholders.

### Testing Changes

After modifying content files, regenerate CLAUDE.md to test:

```bash
# Test tick stage
python3 scripts/generate-claude-md.py tick
cat CLAUDE.md  # Review generated content

# Test tock stage
python3 scripts/generate-claude-md.py tock
cat CLAUDE.md  # Review generated content
```

## Current Stage

The system starts in **TICK STAGE** by default. The generated `CLAUDE.md` clearly indicates the current stage at the top of the methodology section.

## Troubleshooting

### Script Execution Errors

```bash
# Make scripts executable
chmod +x scripts/tick-command.sh
chmod +x scripts/tock-command.sh

# Check Python is available
python3 --version
```

### Template Generation Failures

1. Ensure all content files exist in `tick-tock-content/`
2. Check file permissions
3. Verify Python 3 is installed and accessible
4. Check that template file exists and has correct placeholders

### CI/CD Integration Issues

1. Ensure `CLAUDE.md` contains the correct stage indicator
2. Check that workflow files are updated to use content-based detection
3. Verify that generated `CLAUDE.md` follows expected format