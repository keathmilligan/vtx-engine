## Context

The current release process uses a local shell script (`scripts/release.sh`) that:
1. Validates the version tag format and increment rules
2. Updates version fields in multiple files (Cargo.toml, package.json, tauri.conf.json)
3. Syncs Cargo.lock
4. Commits, tags, and pushes to trigger the existing `publish.yml` workflow

This requires a local development environment with bash, git, Node.js, and Rust installed. Converting to a GitHub workflow removes this friction and ensures consistent execution.

## Goals / Non-Goals

**Goals:**
- Enable releases from GitHub UI or API via `workflow_dispatch`
- Accept `version` and `force` inputs matching the script's behavior
- Validate version format and increment rules (unless `force` is true)
- Update all versioned files and sync Cargo.lock
- Commit, tag, and push to trigger the existing publish workflow
- Add workflow status badges to README.md

**Non-Goals:**
- Changing the existing `publish.yml` workflow behavior
- Adding new release artifacts or publication targets
- Supporting dry-run mode (not needed in workflow context)

## Decisions

### Use a single workflow job for version bump and tag push
**Rationale:** The release workflow must commit and push before the publish workflow can run. Using a single job simplifies the flow and avoids complex job dependencies.

**Alternatives considered:**
- Separate jobs for validation, update, and push — adds complexity without benefit since all steps must succeed sequentially

### Use bash script embedded in workflow
**Rationale:** The existing `release.sh` logic is well-tested. Embedding similar bash commands in the workflow maintains consistency and leverages existing patterns.

**Alternatives considered:**
- Rewrite in pure GitHub Actions steps — more verbose, harder to maintain
- Use a GitHub Action for version bumping — adds external dependency, less control over validation

### Use `git push origin HEAD:main <tag>` pattern
**Rationale:** The workflow needs to push both the commit and tag. Using explicit refspec ensures the commit goes to main and the tag is pushed separately.

### Remove `scripts/release.sh` entirely
**Rationale:** The workflow supersedes the script. Keeping both creates maintenance burden and confusion about which to use.

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| Workflow fails mid-release (commit pushed but tag not) | The `force` input allows re-running; user can manually tag if needed |
| Invalid version pushed with `force` | Document that `force` bypasses validation; use with caution |
| Race condition if multiple releases triggered | GitHub serializes workflow runs; add concurrency control |
| Tag already exists | Workflow checks for existing tag before proceeding |
