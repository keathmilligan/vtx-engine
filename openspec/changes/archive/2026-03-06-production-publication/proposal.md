## Why

Apps consuming this library currently reference it via direct Git/path dependencies, which works well during development but breaks in production CI and release pipelines that require versioned, published crates. Publishing `vtx-engine` as a single crate to crates.io enables clean versioned consumption while keeping the path-dependency workflow available for local development.

## What Changes

- Merge `vtx-common` into `vtx-engine` so a single crate is published to crates.io
- Establish a release workflow (versioning, crates.io publish via CI)
- Document both consumption patterns: crates.io (production) and path/Git dependency (development)
- Ensure `Cargo.toml` metadata (repository, homepage, documentation, description, keywords, categories, license) is complete and accurate before first publish

## Capabilities

### New Capabilities

- `crate-publication`: Crates.io publish workflow covering the vtx-common merge, versioning policy, release CI steps, pre-publish checklist, and dual-dependency guidance for consumers (registry vs path/git)

### Modified Capabilities

<!-- No existing spec-level requirements are changing; this is purely a release/distribution concern -->

## Impact

- `crates/vtx-engine/Cargo.toml`: metadata corrections and removal of vtx-common dependency
- `crates/vtx-common/`: merged into vtx-engine and removed from workspace
- `Cargo.toml` (workspace): vtx-common removed from members and workspace dependencies
- `apps/vtx-demo/src-tauri/Cargo.toml`: vtx-common dependency replaced by vtx-engine
- CI pipeline: publish workflow simplified to a single crate publish step
- Consumer docs / README: updated guidance on how to add the dependency
