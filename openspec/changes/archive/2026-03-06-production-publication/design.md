## Context

`vtx-engine` is a Cargo workspace with two crates:

- `vtx-common` — shared types only; two files (`lib.rs` + `types.rs`); zero runtime logic beyond simple methods on its own types; depends only on `serde`/`serde_json`
- `vtx-engine` — the main library; depends on `vtx-common`; already does `pub use vtx_common;` at its root so external consumers can reach all types through a single crate

The `vtx-demo` Tauri app currently lists both as direct workspace dependencies and glob-imports `vtx_common::*` alongside `use vtx_engine::{AudioEngine, EngineBuilder}`.

Both crate `Cargo.toml` files contain most publication metadata but have placeholder `repository`/`homepage` URLs (`github.com/user/vtx-engine`). A `publish.yml` GitHub Actions workflow already exists, triggered by `v*` tags.

The split into two crates was likely a historical convenience. Because `vtx-common` contains only types that are intrinsically part of the engine's public API, maintaining it as a separate published crate creates unnecessary consumer friction (two dependency entries, potential version skew, separate docs page). Merging brings the public API surface into one place and simplifies publication to a single step.

## Goals / Non-Goals

**Goals:**
- Merge all `vtx-common` source into `vtx-engine` so a single crate is published
- Remove the `vtx-common` workspace member
- Update `vtx-demo` to depend only on `vtx-engine`
- Fix the placeholder `repository`/`homepage` metadata in `vtx-engine/Cargo.toml`
- Simplify the publish CI to a single `cargo publish` step
- Add consumer-facing README snippets for both crates.io and path/git usage

**Non-Goals:**
- Changing the public API shape of any type (pure structural move, no behavior changes)
- Moving to a monorepo publish tool (e.g., `cargo-release`) — manual tag-based workflow is sufficient
- Automated changelog generation

## Decisions

### Decision: Move vtx-common/src/types.rs into vtx-engine as an inline module

`vtx-common` is exactly two files. The types live in `types.rs` and are glob-re-exported from `lib.rs`. The simplest merge is:

1. Create `crates/vtx-engine/src/common.rs` (or `types.rs`) by copying `vtx-common/src/types.rs` verbatim.
2. Add `pub mod common;` + `pub use common::*;` to `vtx-engine/src/lib.rs`, replacing the existing `use vtx_common::*;` and `pub use vtx_common;` lines.
3. Remove `vtx-common` as a `[dependency]` in `vtx-engine/Cargo.toml`.
4. Delete `crates/vtx-common/`.
5. Remove `vtx-common` from the workspace members and `[workspace.dependencies]`.

The `vtx-demo` app's `use vtx_common::*` imports become `use vtx_engine::*` (or keep the specific names — both work). Its `vtx-common` dependency entry is dropped.

*Alternatives considered:*
- **Keep `vtx-common` as an internal module path dependency that is not published** — reduces the crates.io footprint to one crate but still requires consumers to deal with the separate source crate if they use a Git dependency. Adds complexity for no gain once the types live in `vtx-engine`.
- **Re-export `vtx-common` as a re-exported sub-crate of `vtx-engine`** — this is what currently exists (`pub use vtx_common;`). It works but means two published crates and two docs pages, which is the problem being solved.

### Decision: Name the inline module `common` (path: `src/common.rs`)

Naming it `common` keeps the internal code readable (`use crate::common::KeyCode`) and signals the types' role. The public API is flattened via `pub use common::*` so consumers see `vtx_engine::KeyCode`, not `vtx_engine::common::KeyCode`.

*Alternatives considered:* Naming it `types` (mirrors the original file name) — works equally well; `common` is slightly more descriptive of purpose.

### Decision: Keep `vtx-demo` using explicit vtx-engine imports, drop vtx-common dependency

Rather than `use vtx_common::*`, the demo can use `use vtx_engine::*` — all the same names are available since `vtx-engine` re-exports everything. The explicit `vtx_common::TranscriptionSegment` path in the demo's return types becomes `vtx_engine::TranscriptionSegment`. This is a purely mechanical name substitution with no behavioral change.

### Decision: Simplify publish.yml to a single publish step

With one crate there is no publication-order problem and no propagation delay to wait for. The two-step publish with `sleep 30` is replaced by a single `cargo publish -p vtx-engine`.

### Decision: Fix repository/homepage placeholder before first tag

The current value `github.com/user/vtx-engine` must be replaced with `https://github.com/keathmilligan/vtx-engine` before any `v*` tag is pushed. This is a blocking pre-condition verified by `cargo publish --dry-run`.

## Risks / Trade-offs

- **[Risk] vtx-demo compilation breaks during migration** — Any import that uses `vtx_common::SomeType` by explicit path (not glob) will fail to compile until updated.  
  → Mitigation: The only explicit-path usage found is `vtx_common::TranscriptionSegment` in one Tauri command return type. This is a one-line fix.

- **[Risk] Placeholder URLs not caught before publish** — Publishing with the wrong URL results in broken links on crates.io, not fixable for that version.  
  → Mitigation: Run `cargo publish --dry-run` locally and visually inspect the output before tagging.

- **[Risk] External consumers of `vtx-common` (if any exist outside this repo)** — Any project depending on a Git path to `vtx-common` directly would break once the crate is removed.  
  → Mitigation: The crate has never been published to crates.io. External Git consumers would need to update to depend on `vtx-engine` instead. This is the intended outcome.

## Migration Plan

1. Copy `crates/vtx-common/src/types.rs` to `crates/vtx-engine/src/common.rs`.
2. Update `crates/vtx-engine/src/lib.rs`: add `pub mod common; pub use common::*;`, remove `use vtx_common::*;` and `pub use vtx_common;`.
3. Remove `vtx-common = { workspace = true, ... }` from `vtx-engine/Cargo.toml` `[dependencies]`.
4. Update all `use vtx_common::` references in `vtx-engine` source files to `use crate::` or unqualified (they are already glob-imported).
5. Remove `vtx-common` from `apps/vtx-demo/src-tauri/Cargo.toml` `[dependencies]`.
6. Update `apps/vtx-demo/src-tauri/src/lib.rs`: replace `use vtx_common::*` with `use vtx_engine::*`; replace `vtx_common::TranscriptionSegment` with `vtx_engine::TranscriptionSegment`.
7. Remove `crates/vtx-common/` from the workspace `[members]` list and from `[workspace.dependencies]`.
8. Delete the `crates/vtx-common/` directory.
9. Fix `repository` and `homepage` in `vtx-engine/Cargo.toml` to `https://github.com/keathmilligan/vtx-engine`.
10. Update `publish.yml` to remove the `vtx-common` publish step and the 30-second wait; keep a single `cargo publish -p vtx-engine` step.
11. Add consumer usage guidance to the workspace `README.md`.
12. Run `cargo build` at workspace root and `cargo publish --dry-run -p vtx-engine` to verify.

Rollback: If a bad version is published, yank with `cargo yank --version x.y.z -p vtx-engine` and publish a corrected patch.

## Open Questions

None.
