## MODIFIED Requirements

### Requirement: Version tag creates a GitHub release
When a version tag is pushed (now via the release workflow), the CI workflow SHALL automatically create a GitHub release using the pushed tag. The release SHALL be created only after the crates.io publish step succeeds.

#### Scenario: Successful tag push creates release
- **WHEN** a git tag matching `v*` is pushed (via release workflow) and the crate is published successfully
- **THEN** a GitHub release is created with the tag name as the release title

#### Scenario: Crate publish failure blocks release creation
- **WHEN** the crates.io publish job fails
- **THEN** the GitHub release job does not run and no GitHub release is created
