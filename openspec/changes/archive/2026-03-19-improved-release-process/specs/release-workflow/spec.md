## ADDED Requirements

### Requirement: Release workflow is manually triggered
The system SHALL provide a GitHub workflow triggered by `workflow_dispatch` that accepts a `version` input in `vX.Y.Z` format and an optional `force` boolean input (default false).

#### Scenario: Workflow dispatch with valid version
- **WHEN** the workflow is dispatched with `version: "v1.2.3"` and `force: false`
- **THEN** the workflow begins execution with the provided inputs

#### Scenario: Force flag bypasses validation
- **WHEN** the workflow is dispatched with `version: "v1.2.3"` and `force: true`
- **THEN** version increment validation is skipped and existing tags can be replaced

### Requirement: Version format is validated
The workflow SHALL validate that the `version` input matches the pattern `vX.Y.Z` where X, Y, and Z are non-negative integers. The workflow SHALL fail if the format is invalid.

#### Scenario: Invalid version format rejected
- **WHEN** the workflow is dispatched with `version: "1.2.3"` (missing 'v' prefix)
- **THEN** the workflow fails with an error indicating invalid format

#### Scenario: Invalid version format rejected (non-numeric)
- **WHEN** the workflow is dispatched with `version: "v1.2.x"`
- **THEN** the workflow fails with an error indicating invalid format

### Requirement: Version increment is validated
The workflow SHALL validate that the new version is a valid semver increment over the latest existing tag, unless `force` is true. Valid increments are: patch bump (same major.minor, patch incremented), minor bump (same major, minor incremented, patch=0), or major bump (major incremented, minor=0, patch=0).

#### Scenario: Valid patch increment accepted
- **WHEN** the latest tag is `v1.2.3` and the workflow is dispatched with `version: "v1.2.4"` and `force: false`
- **THEN** the workflow proceeds with the release

#### Scenario: Invalid increment rejected
- **WHEN** the latest tag is `v1.2.3` and the workflow is dispatched with `version: "v1.2.2"` and `force: false`
- **THEN** the workflow fails with an error indicating the version is not a valid increment

#### Scenario: Force bypasses increment validation
- **WHEN** the latest tag is `v1.2.3` and the workflow is dispatched with `version: "v1.2.2"` and `force: true`
- **THEN** the workflow proceeds with the release

### Requirement: Existing tag check prevents duplicates unless force is enabled
The workflow SHALL check that the specified tag does not already exist. The workflow SHALL fail if the tag exists and `force` is false. If `force` is true, the existing tag SHALL be deleted and replaced.

#### Scenario: Duplicate tag rejected without force
- **WHEN** the tag `v1.2.3` already exists and the workflow is dispatched with `version: "v1.2.3"` and `force: false`
- **THEN** the workflow fails with an error indicating the tag already exists

#### Scenario: Existing tag replaced with force
- **WHEN** the tag `v1.2.3` already exists and the workflow is dispatched with `version: "v1.2.3"` and `force: true`
- **THEN** the existing tag is deleted and a new tag `v1.2.3` is created and pushed

### Requirement: Versioned files are updated
The workflow SHALL update the `version` field in the following files to the new version (without 'v' prefix): `Cargo.toml` (workspace root), `packages/vtx-viz/package.json`, `apps/vtx-demo/package.json`, and `apps/vtx-demo/src-tauri/tauri.conf.json`.

#### Scenario: All versioned files updated
- **WHEN** the workflow proceeds with `version: "v1.2.3"`
- **THEN** all four files contain `version: "1.2.3"` (or equivalent JSON format)

### Requirement: Cargo.lock is synchronized
The workflow SHALL run `cargo update --workspace` to synchronize `Cargo.lock` with the updated `Cargo.toml` version.

#### Scenario: Cargo.lock updated
- **WHEN** the workflow updates `Cargo.toml` to version `1.2.3`
- **THEN** `cargo update --workspace` is executed and `Cargo.lock` reflects the new version

### Requirement: Changes are committed and tagged
The workflow SHALL commit all changed files with message `chore: release <tag>`, create the specified git tag, and push both the commit and tag to the remote repository.

#### Scenario: Commit and tag pushed
- **WHEN** the workflow completes the version update
- **THEN** a commit with message `chore: release v1.2.3` exists on main and tag `v1.2.3` exists in the remote

### Requirement: Workflow uses appropriate permissions
The workflow SHALL declare `permissions: contents: write` to enable pushing commits and tags.

#### Scenario: Contents write permission granted
- **WHEN** the workflow configuration is inspected
- **THEN** `permissions: contents: write` is present
