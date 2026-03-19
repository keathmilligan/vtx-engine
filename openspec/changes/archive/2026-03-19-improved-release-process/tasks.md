## 1. Create Release Workflow

- [x] 1.1 Create `.github/workflows/release.yml` with `workflow_dispatch` trigger
- [x] 1.2 Add `version` (required) and `force` (optional, default false) inputs
- [x] 1.3 Set `permissions: contents: write`
- [x] 1.4 Add concurrency control to prevent parallel releases

## 2. Implement Version Validation

- [x] 2.1 Add step to validate version format matches `vX.Y.Z`
- [x] 2.2 Add step to check tag does not already exist
- [x] 2.3 Add step to validate version increment (skip if `force` is true)
- [x] 2.4 Fail workflow with clear error messages for validation failures

## 3. Implement Version Updates

- [x] 3.1 Add step to update version in `Cargo.toml`
- [x] 3.2 Add step to update version in `packages/vtx-viz/package.json`
- [x] 3.3 Add step to update version in `apps/vtx-demo/package.json`
- [x] 3.4 Add step to update version in `apps/vtx-demo/src-tauri/tauri.conf.json`
- [x] 3.5 Add step to run `cargo update --workspace` to sync `Cargo.lock`

## 4. Implement Git Operations

- [x] 4.1 Add step to commit changes with message `chore: release <tag>`
- [x] 4.2 Add step to create the git tag
- [x] 4.3 Add step to push commit and tag to remote

## 5. Update Documentation

- [x] 5.1 Add workflow status badge for release workflow to `README.md`
- [x] 5.2 Add workflow status badge for publish workflow to `README.md`
- [x] 5.3 Add workflow status badge for CI workflow to `README.md`

## 6. Cleanup

- [x] 6.1 Remove `scripts/release.sh`
