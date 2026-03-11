## ADDED Requirements

### Requirement: CI workflow caches external dependencies
The CI workflow SHALL cache downloaded whisper.cpp binaries and repositories to reduce build times.

#### Scenario: Cold cache - first build downloads dependencies
- **WHEN** the CI workflow runs on a clean environment with no cache
- **THEN** the workflow downloads whisper.cpp binaries from GitHub releases
- **AND** the downloaded artifacts are saved to the GitHub Actions cache

#### Scenario: Warm cache - subsequent builds use cached dependencies
- **WHEN** the CI workflow runs with an existing valid cache
- **THEN** the workflow restores cached whisper.cpp binaries from the cache
- **AND** the build process uses the cached artifacts without re-downloading
- **AND** the build completes successfully

#### Scenario: Cache invalidation on version change
- **WHEN** the whisper.cpp version constant in build.rs changes
- **THEN** the cache key changes to reflect the new version
- **AND** the workflow treats this as a cache miss
- **AND** new binaries are downloaded and cached under the new key

### Requirement: Cache configuration supports all platforms
The caching configuration SHALL support all platforms in the CI matrix: ubuntu-latest, windows-latest, and macos-latest.

#### Scenario: Windows platform caching
- **WHEN** the CI job runs on windows-latest
- **THEN** the cache stores Windows-specific whisper.cpp binaries (CUDA and CPU variants)
- **AND** the cache key includes "windows" identifier

#### Scenario: macOS platform caching
- **WHEN** the CI job runs on macos-latest
- **THEN** the cache stores macOS-specific whisper.cpp xcframework binaries
- **AND** the cache key includes "macos" identifier

#### Scenario: Linux platform caching
- **WHEN** the CI job runs on ubuntu-latest
- **THEN** the cache stores the cloned whisper.cpp git repository
- **AND** the cache key includes "linux" identifier

### Requirement: Publish workflow uses caching
The Publish workflow SHALL use the same caching strategy as the CI workflow for whisper.cpp dependencies.

#### Scenario: Publish job uses cached dependencies
- **WHEN** the Publish workflow runs to create a release
- **THEN** it restores cached whisper.cpp dependencies if available
- **AND** the publish completes successfully with or without cache

### Requirement: Cache keys include whisper version
All cache keys SHALL include the whisper.cpp version to ensure cache invalidation when dependencies are updated.

#### Scenario: Cache key format
- **WHEN** the cache is configured
- **THEN** the cache key includes "whisper" and the version number (e.g., "whisper-1.8.2")
- **AND** the cache key includes the OS platform identifier
- **AND** a fallback key exists to match previous caches for the same version

### Requirement: Cache gracefully handles failures
The workflow SHALL continue successfully even if cache operations fail.

#### Scenario: Cache restore failure
- **WHEN** the cache restore step fails or returns a miss
- **THEN** the workflow continues to the build step
- **AND** the build downloads dependencies as it would without caching

#### Scenario: Cache save failure
- **WHEN** the cache save operation fails at job completion
- **THEN** the workflow still reports success
- **AND** the build artifacts are still valid
