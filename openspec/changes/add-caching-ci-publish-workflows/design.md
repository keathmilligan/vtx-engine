## Context

The vtx-engine build process downloads substantial external dependencies from GitHub releases on every CI run:

- **whisper.cpp binaries** (~50-100MB per platform): Downloaded from ggml-org/whisper.cpp releases
  - Windows x64: Two variants (CUDA and CPU-only) with multiple DLLs
  - macOS: xcframework with Metal-enabled binaries
  - Linux: Built from source via git clone of whisper.cpp repository

Currently, the build script maintains a local cache in `target/whisper-cache/`, but this cache is not persisted across GitHub Actions runs. Each workflow execution re-downloads all dependencies, adding 30-60 seconds to every build.

The Rust build artifacts are already cached via `Swatinem/rust-cache@v2`, but external C++ libraries and downloaded binaries are not included in that cache.

## Goals / Non-Goals

**Goals:**
- Cache downloaded whisper.cpp binaries across CI runs to reduce build times by 30-60 seconds
- Cache whisper.cpp git repository for Linux builds to avoid cloning on every run
- Ensure cache keys properly invalidate when whisper.cpp version changes
- Add caching to both CI and publish workflows
- Maintain reliability - builds should still succeed if cache is corrupted or unavailable

**Non-Goals:**
- Changing the build script logic or download behavior
- Caching system package dependencies (apt packages)
- Caching the actual whisper models (ggml-model-*.bin files)
- Optimizing the build script's internal caching mechanism

## Decisions

### Use GitHub Actions Cache with Static Cache Keys

**Decision:** Use `actions/cache@v4` with cache keys based on the whisper.cpp version and platform.

**Rationale:** 
- The build script already has a well-defined version constant (`WHISPER_VERSION = "1.8.2"`)
- Cache keys can be derived from this version to ensure automatic invalidation on updates
- Separate caches per OS platform (ubuntu, windows, macos) to avoid cache pollution

**Alternative Considered:** Hash-based cache keys on the downloaded files.
- **Rejected:** The download URLs and version are deterministic; version-based keys are simpler and sufficient.

### Cache the Entire `target/whisper-cache/` Directory

**Decision:** Cache the entire whisper-cache directory including both extracted libraries and zip files.

**Rationale:**
- The build script stores all downloaded artifacts in `target/whisper-cache/`
- This includes zip files (used for extraction) and the extracted library files
- Caching both ensures the build script's cache detection logic works correctly

### Support All Three Platform Matrices in CI

**Decision:** Configure caching for all three OS platforms: ubuntu-latest, windows-latest, macos-latest.

**Rationale:**
- The CI workflow runs on all three platforms
- Each platform downloads different binaries (or clones source for Linux)
- Separate caches per platform prevent cache collisions and ensure appropriate content

### Include Cache Step Before Build

**Decision:** Place the cache restoration step before the Rust build step.

**Rationale:**
- GitHub Actions cache restore should happen early in the job
- The build script will detect cached files and skip downloads
- Cache save happens automatically at the end of the job

## Risks / Trade-offs

**[Risk] Cache size limits** → GitHub Actions has a 10GB cache limit per repository. The whisper binaries are ~50-100MB per platform, so ~300MB total. This is well within limits.

**[Risk] Cache key collisions on version updates** → Using the whisper version in the cache key ensures invalidation, but requires remembering to update the cache key constant when `WHISPER_VERSION` changes. Mitigation: Document this clearly in the workflow file.

**[Risk] Corrupted cache entries** → If a partial download gets cached, subsequent builds may fail. Mitigation: The build script already has logic to check file existence; we can add a cache-busting input or manual cache deletion workflow if needed.

**[Trade-off] Linux builds still require compilation** → Even with the git repository cached, whisper.cpp still needs to be compiled from source on Linux. This saves the clone time but not the build time. This is acceptable as the clone is a significant portion of the time.

## Migration Plan

1. **Update CI workflow** (`.github/workflows/ci.yml`):
   - Add cache step for whisper libraries before the build
   - Configure cache keys based on whisper version and platform
   
2. **Update Publish workflow** (`.github/workflows/publish.yml`):
   - Add cache step for whisper libraries before the publish
   - Use same caching strategy as CI

3. **Validation**:
   - Run CI on a PR to verify first build (cold cache) succeeds
   - Re-run CI to verify second build (warm cache) uses cached artifacts
   - Verify build times improve significantly on cached runs

4. **Rollback**: If issues arise, revert the workflow changes. The build will simply re-download dependencies as it does today.

## Open Questions

- Should we also cache the actual whisper model files (`.ggml` models) used in tests? These are separate from the library binaries and much larger.
- For Linux, should we cache the compiled whisper libraries in addition to the source repository? This would require detecting the build profile and CUDA feature flag.
