## MODIFIED Requirements

### Requirement: Release CI job publishes to crates.io
A CI job named `publish-crate` SHALL automate the publish process, triggered by a version tag (e.g., `v*`). It SHALL authenticate using a `CARGO_REGISTRY_TOKEN` secret and publish `vtx-engine`. A second job (`create-release`) SHALL depend on `publish-crate` via `needs:` so the GitHub release is only created after a successful crate publish.

#### Scenario: Tag triggers publish
- **WHEN** a git tag matching `v*` is pushed
- **THEN** the `publish-crate` CI job runs and publishes `vtx-engine` to crates.io

#### Scenario: Missing token fails gracefully
- **WHEN** `CARGO_REGISTRY_TOKEN` is not set
- **THEN** the CI job fails with a clear authentication error before attempting any publish

#### Scenario: create-release depends on publish-crate
- **WHEN** the publish workflow is triggered by a version tag
- **THEN** the `create-release` job does not start until `publish-crate` has completed successfully
