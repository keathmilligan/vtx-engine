## Context

The current `publish.yml` is a single job that checks out the repo, installs the Rust toolchain and Linux audio deps, then runs `cargo publish -p vtx-engine`. It fires on any `v*` tag push. There is no GitHub release created and no release notes surfaced to users or downstream consumers.

GitHub Actions provides `GITHUB_TOKEN` with release-write permission by default on public repos. The `softprops/action-gh-release` action is the de-facto standard for creating releases from a workflow and supports auto-generated notes natively.

## Goals / Non-Goals

**Goals:**
- Restructure `publish.yml` into two explicit jobs that run sequentially.
- Job 1 (`publish-crate`): unchanged behavior — publish `vtx-engine` to crates.io.
- Job 2 (`create-release`): create a GitHub release for the tag with auto-generated release notes; runs only after `publish-crate` succeeds.
- No new secrets; use the built-in `GITHUB_TOKEN`.

**Non-Goals:**
- Uploading binary build artifacts or pre-built binaries to the release.
- Changelog file generation (auto-notes from GitHub are sufficient for now).
- Modifying `ci.yml` or any Rust source.

## Decisions

### Job sequencing via `needs`

The `create-release` job declares `needs: publish-crate`. This ensures crates.io publication succeeds before a GitHub release is created, avoiding a visible release that points to an unpublished crate version.

Alternative: run in parallel. Rejected — a failed crate publish with a live GitHub release would be confusing.

### GitHub release action: `softprops/action-gh-release@v2`

This is the most widely used action for this purpose (~50 M uses/month), with active maintenance and first-class support for `generate_release_notes: true`. It requires only `GITHUB_TOKEN` and the tag ref, both available implicitly.

Alternative: `gh release create` via CLI in a `run` step. Works but is more verbose and requires manually passing `${{ github.ref_name }}`. The action is cleaner and idiomatic.

Alternative: `actions/create-release@v1` — deprecated, not considered.

### Auto-generated release notes

GitHub's auto-generated notes summarise commits and PRs merged since the previous tag. No additional configuration is needed unless a `release.yml` categories file is added later. This is sufficient for a library where each tag corresponds to a crate version bump.

### `permissions` block

The workflow-level (or job-level) `permissions` block must explicitly grant `contents: write` so `GITHUB_TOKEN` can create a release. The crate publish job does not need this permission; it is scoped to `create-release` only.

## Risks / Trade-offs

- **Tag on failed publish**: If the crate publish fails mid-run but the tag already exists, re-running the workflow will retry both jobs. `cargo publish` is idempotent for an already-published version (fails gracefully), so the release job will still be reached on retry.
  → Mitigation: acceptable; no data loss.

- **Release notes quality**: Auto-generated notes depend on commit message hygiene. If commits are poorly worded the notes will be low quality.
  → Mitigation: out of scope for this change; can add a `.github/release.yml` categories file later.

## Migration Plan

1. Edit `.github/workflows/publish.yml` in place — no branch or environment changes required.
2. Test by pushing a pre-release tag (e.g. `v0.1.1-rc.1`) to a fork or by using `act` locally.
3. First production use on the next real version tag.
