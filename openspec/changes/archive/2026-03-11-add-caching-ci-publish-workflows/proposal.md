## Why

The CI and publish workflows download large external dependencies (whisper libraries, CUDA support, and other assets from external GitHub repositories and sites) on every run. This significantly slows down builds, wastes bandwidth, and increases CI costs. Caching these artifacts will dramatically reduce build times and improve developer productivity.

## What Changes

- Add caching configuration to the CI workflow for external dependencies
- Add caching configuration to the publish workflow for external dependencies
- Cache whisper libraries and models downloaded from external repositories
- Cache CUDA toolkit and related dependencies
- Cache other external build artifacts and support libraries
- Configure cache keys based on dependency versions to ensure cache invalidation when dependencies change

## Capabilities

### New Capabilities
- `workflow-caching`: Cache external dependencies in CI and publish workflows to reduce build times and improve reliability

### Modified Capabilities
<!-- No existing specs are being modified - this is an infrastructure enhancement -->

## Impact

- GitHub Actions workflows: CI and publish workflows will be modified to include caching steps
- Build time: Significant reduction in build times for subsequent runs
- External dependencies: whisper libraries, CUDA toolkit, and other downloaded artifacts
- No API changes or breaking changes to the codebase
