## Why

The current release process requires running a local shell script (`scripts/release.sh`) which creates friction for releases and requires a local development environment. Converting this to a GitHub workflow enables releases to be triggered directly from GitHub's UI or API, removing the need for local tooling and ensuring consistent execution in a controlled environment.

## What Changes

- **New GitHub workflow** `release.yml` triggered by `workflow_dispatch` with inputs:
  - `version` (required): The version tag in `vX.Y.Z` format
  - `force` (optional, default false): Skip validation and allow re-releasing existing version
- **Removes** the `--dry-run` option (not needed in workflow context)
- **Removes** the working tree cleanliness check (workflow runs on a fresh checkout)
- **Updates** versioned files: `Cargo.toml`, `packages/vtx-viz/package.json`, `apps/vtx-demo/package.json`, `apps/vtx-demo/src-tauri/tauri.conf.json`
- **Syncs** `Cargo.lock` via `cargo update --workspace`
- **Commits**, tags, and pushes the release
- **Adds** workflow status badges to `README.md`
- **Removes** `scripts/release.sh` (superseded by the workflow)

## Capabilities

### New Capabilities
- `release-workflow`: GitHub workflow for manual release dispatch with version validation, file updates, and git operations

### Modified Capabilities
- `github-release`: The existing spec remains valid; the new workflow triggers the existing publish workflow by pushing the tag

## Impact

- **New file**: `.github/workflows/release.yml`
- **Modified file**: `README.md` (adds workflow status badges)
- **Removed file**: `scripts/release.sh`
- **Affected systems**: GitHub Actions, crates.io publication pipeline
