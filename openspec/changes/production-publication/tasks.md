## 1. Merge vtx-common into vtx-engine

- [x] 1.1 Copy `crates/vtx-common/src/types.rs` to `crates/vtx-engine/src/common.rs`
- [x] 1.2 In `crates/vtx-engine/src/lib.rs`, add `pub mod common; pub use common::*;` and remove `use vtx_common::*;` and `pub use vtx_common;`
- [x] 1.3 Remove `vtx-common = { workspace = true, version = "0.1.0" }` from `crates/vtx-engine/Cargo.toml` `[dependencies]`
- [x] 1.4 Update any remaining `use vtx_common::` references in `vtx-engine` source files (builder.rs, history.rs, model_manager.rs) to use unqualified names (already glob-imported via `pub use common::*`)

## 2. Update vtx-demo

- [x] 2.1 Remove `vtx-common = { workspace = true }` from `apps/vtx-demo/src-tauri/Cargo.toml`
- [x] 2.2 In `apps/vtx-demo/src-tauri/src/lib.rs`, replace `use vtx_common::*` with `use vtx_engine::*`
- [x] 2.3 Replace the explicit `vtx_common::TranscriptionSegment` path with `vtx_engine::TranscriptionSegment`

## 3. Remove vtx-common from workspace

- [x] 3.1 Remove `"crates/vtx-common"` from `[workspace]` `members` in the root `Cargo.toml`
- [x] 3.2 Remove the `vtx-common` entry from `[workspace.dependencies]` in the root `Cargo.toml`
- [x] 3.3 Delete the `crates/vtx-common/` directory

## 4. Fix crate metadata

- [x] 4.1 Update `repository` and `homepage` in `crates/vtx-engine/Cargo.toml` to `https://github.com/keathmilligan/vtx-engine`
- [x] 4.2 Update `documentation` in `crates/vtx-engine/Cargo.toml` to `https://docs.rs/vtx-engine`

## 5. Update CI workflow

- [x] 5.1 In `.github/workflows/publish.yml`, remove the `vtx-common` publish step and the "Wait for vtx-common" sleep step, leaving a single `cargo publish -p vtx-engine` step

## 6. Update README

- [x] 6.1 Add a "Usage" section to the workspace `README.md` with a crates.io `[dependencies]` snippet for `vtx-engine`
- [x] 6.2 Add a path/Git dependency snippet for local development workflows

## 7. Verify

- [x] 7.1 Run `cargo build` at the workspace root and confirm it succeeds
- [x] 7.2 Run `cargo publish --dry-run -p vtx-engine` and confirm it succeeds without warnings
