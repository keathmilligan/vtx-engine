## 1. Refactor publish.yml

- [x] 1.1 Rename the existing `publish` job to `publish-crate` in `.github/workflows/publish.yml`
- [x] 1.2 Add `needs: publish-crate` to the new `create-release` job so it only runs after successful crate publication
- [x] 1.3 Add `permissions: contents: write` scoped to the `create-release` job
- [x] 1.4 Add the `create-release` job using `softprops/action-gh-release@v2` with `generate_release_notes: true`
- [x] 1.5 Verify the workflow file is valid YAML and that both jobs are correctly structured

## 2. Verify

- [x] 2.1 Confirm the workflow triggers on `v*` tags and that both jobs are listed under the same `on:` trigger
- [x] 2.2 Confirm no new repository secrets are introduced (only `GITHUB_TOKEN` and existing `CARGO_REGISTRY_TOKEN` are referenced)
