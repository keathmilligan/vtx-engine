## Purpose

Defines requirements for publishing `vtx-engine` as a single crate to crates.io, including the merge of `vtx-common` into `vtx-engine`, metadata completeness, versioning policy, CI-based release automation, and consumer documentation.

## Requirements

### Requirement: vtx-common is merged into vtx-engine
All public types currently in `vtx-common` SHALL be moved into `vtx-engine` and re-exported from its crate root. The `vtx-common` crate SHALL be removed from the workspace. A single crate (`vtx-engine`) SHALL be published to crates.io.

#### Scenario: vtx-common types accessible via vtx-engine
- **WHEN** an external consumer depends on `vtx-engine`
- **THEN** all types previously in `vtx-common` are accessible under the `vtx_engine` namespace without an additional dependency

#### Scenario: vtx-common crate removed from workspace
- **WHEN** `cargo build` is run at the workspace root after the merge
- **THEN** the build succeeds with no reference to a `vtx-common` package

### Requirement: Crate metadata is complete before publish
`vtx-engine/Cargo.toml` SHALL contain complete and accurate publication metadata including: `description`, `version`, `license`, `repository`, `homepage`, `documentation`, `keywords`, and `categories`. The `repository` and `homepage` fields SHALL reference the real repository URL (not a placeholder).

#### Scenario: vtx-engine metadata is valid
- **WHEN** `cargo publish --dry-run` is run for `vtx-engine`
- **THEN** the command succeeds without metadata warnings or errors

### Requirement: Versioning policy is followed
`vtx-engine` SHALL use Semantic Versioning (SemVer). A breaking change to any previously public type or function SHALL result in a major version increment before publish.

#### Scenario: Breaking change increments major version
- **WHEN** a public API-breaking change is introduced
- **THEN** the version major component is incremented before the tag is pushed

### Requirement: Release CI job publishes to crates.io
A CI job SHALL automate the publish process, triggered by a version tag (e.g., `v*`). It SHALL authenticate using a `CARGO_REGISTRY_TOKEN` secret and publish `vtx-engine`.

#### Scenario: Tag triggers publish
- **WHEN** a git tag matching `v*` is pushed
- **THEN** the release CI job runs and publishes `vtx-engine` to crates.io

#### Scenario: Missing token fails gracefully
- **WHEN** `CARGO_REGISTRY_TOKEN` is not set
- **THEN** the CI job fails with a clear authentication error before attempting any publish

### Requirement: Consumer dependency guidance is documented
The library README SHALL document both consumption patterns: crates.io registry dependency (for production) and path/Git dependency (for local development).

#### Scenario: Production usage documented
- **WHEN** a developer reads the README
- **THEN** they find a `[dependencies]` snippet using the crates.io version of `vtx-engine`

#### Scenario: Development usage documented
- **WHEN** a developer reads the README
- **THEN** they find instructions for using a path or Git dependency for local development workflows
