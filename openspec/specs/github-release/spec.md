## Purpose

Defines requirements for automatically creating a GitHub release when a version tag is pushed, including release note generation, job sequencing relative to crate publication, and secrets/permissions constraints.

## Requirements

### Requirement: Version tag creates a GitHub release
When a version tag is pushed (via the release workflow), the CI workflow SHALL automatically create a GitHub release using the pushed tag. The release SHALL be created only after the crates.io publish step succeeds.

#### Scenario: Successful tag push creates release
- **WHEN** a git tag matching `v*` is pushed (via release workflow) and the crate is published successfully
- **THEN** a GitHub release is created with the tag name as the release title

#### Scenario: Crate publish failure blocks release creation
- **WHEN** the crates.io publish job fails
- **THEN** the GitHub release job does not run and no GitHub release is created

### Requirement: Release notes are auto-generated
The GitHub release SHALL include auto-generated release notes summarising commits and merged pull requests since the previous tag. No manual changelog file or extra configuration is required.

#### Scenario: Release notes populated
- **WHEN** a GitHub release is created for a tag
- **THEN** the release body contains automatically generated notes based on commits since the prior tag

### Requirement: No additional secrets are required
The GitHub release creation SHALL use the built-in `GITHUB_TOKEN` provided by GitHub Actions. No new repository secrets are needed.

#### Scenario: Release created with implicit token
- **WHEN** the `create-release` job runs
- **THEN** it authenticates using `secrets.GITHUB_TOKEN` without requiring a separately configured personal access token

### Requirement: Release job has minimal permissions
The `create-release` job SHALL declare `permissions: contents: write` explicitly, and SHALL NOT be granted broader permissions than necessary.

#### Scenario: Only contents write permission granted
- **WHEN** the `create-release` job configuration is inspected
- **THEN** only `contents: write` permission is present in its permissions block
