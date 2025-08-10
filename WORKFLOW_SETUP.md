# Workflow Setup Instructions

## Overview

Due to GitHub App permission restrictions, the CI workflow files were created in the `workflow-files/` directory instead of `.github/workflows/`. You need to manually move them to the correct location.

## Quick Setup

```bash
# Create workflows directory if it doesn't exist
mkdir -p .github/workflows

# Move workflow files to correct location
mv workflow-files/tick-ci.yml .github/workflows/
mv workflow-files/tock-ci.yml .github/workflows/
mv workflow-files/stage-transition.yml .github/workflows/

# Remove temporary directory
rmdir workflow-files

# Commit the workflow files
git add .github/workflows/
git commit -m "Add tick-tock methodology CI workflows"
git push origin claude/issue-58-20250717-0752
```

## Workflow Files

The following workflow files are included in this implementation:

### 1. `tick-ci.yml` - Tick Stage CI Pipeline
- **Triggers**: Push/PR to `tick/current` branch
- **Focus**: Strict build and test requirements
- **Quality Gates**: All builds and tests MUST pass
- **Prohibited**: TODOs, mocks, placeholders in production code

### 2. `tock-ci.yml` - Tock Stage CI Pipeline  
- **Triggers**: Push/PR to `tock/current` branch
- **Focus**: Documentation quality and architectural compliance
- **Quality Gates**: Documentation coverage >90%
- **Permitted**: TODOs/mocks for architectural backbone

### 3. `stage-transition.yml` - Automated Stage Transition Monitor
- **Triggers**: Every 6 hours (cron) + manual dispatch
- **Purpose**: Monitor transition criteria between tick/tock stages
- **Actions**: Creates GitHub issues with transition recommendations

## Initial Stage Setup

After setting up the workflows, choose your initial development stage:

### Option A: Start with TICK Stage
```bash
# Create tick stage branch
git checkout -b tick/current
git push origin tick/current

# Create stage marker
echo "**Stage**: tick" > STAGE_MARKER.md
echo "**Started**: $(date +%Y-%m-%d)" >> STAGE_MARKER.md
git add STAGE_MARKER.md
git commit -m "Initialize tick stage"
git push origin tick/current
```

### Option B: Start with TOCK Stage
```bash
# Create tock stage branch
git checkout -b tock/current
git push origin tock/current

# Create stage marker
echo "**Stage**: tock" > STAGE_MARKER.md
echo "**Started**: $(date +%Y-%m-%d)" >> STAGE_MARKER.md
git add STAGE_MARKER.md
git commit -m "Initialize tock stage"
git push origin tock/current
```

## Testing the Workflows

After setting up the workflows, test them by:

1. **Push a change** to the active stage branch
2. **Check Actions tab** to see if the appropriate workflow runs
3. **Verify stage detection** works correctly
4. **Test stage transition monitor** by running it manually

## Troubleshooting

### Workflow Permission Issues
If you still encounter permission issues:
1. Check repository settings → Actions → General
2. Ensure "Allow all actions and reusable workflows" is enabled
3. Verify the GitHub App has necessary permissions

### Stage Detection Issues
If stage detection fails:
1. Check branch names match exactly: `tick/current` or `tock/current`
2. Verify environment variables are set correctly
3. Ensure stage marker file exists and contains correct stage

### CI Pipeline Failures
For tick stage failures:
- All builds and tests must pass
- No TODOs or mock implementations allowed
- Code quality checks must pass

For tock stage issues:
- Build failures are permitted during refactoring
- Focus on documentation quality
- Architecture compliance is key

## Manual Override

If automated stage transition doesn't work:
1. Create GitHub issue describing the problem
2. Manually archive current stage branch
3. Create new stage branch
4. Update stage marker file
5. Push changes and test workflows

## Next Steps

1. **Review** the tick-tock methodology in `TICK_TOCK_METHODOLOGY.md`
2. **Set up** the workflows using the instructions above
3. **Choose** initial development stage
4. **Test** the CI pipelines
5. **Start** development with stage-specific guidelines

The tick-tock methodology is now ready for use!