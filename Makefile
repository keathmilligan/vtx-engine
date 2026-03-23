.PHONY: all build build-lib build-viz build-demo test dev-demo clean help publish publish-dry-run release

# Default target
all: build

##
## Build targets
##

## Build everything (Rust library + viz package + demo app)
build: build-lib build-viz build-demo

## Build the Rust engine crate (vtx-engine)
build-lib:
	cargo build --workspace --exclude vtx-demo-src-tauri

## Build the @vtx-engine/viz TypeScript package
build-viz:
	pnpm --filter @vtx-engine/viz build

## Build the vtx-demo Tauri application
build-demo: build-viz
	pnpm --filter vtx-demo build

##
## Test targets
##

## Run all Rust tests
test:
	cargo test --workspace --exclude vtx-demo-src-tauri

## Run Rust tests with output (no capture)
test-verbose:
	cargo test --workspace --exclude vtx-demo-src-tauri -- --nocapture

##
## Development targets
##

node_modules: pnpm-lock.yaml
	pnpm install

install: node_modules

## Run the demo app in Tauri development mode
demo-dev: install
	@if lsof -tiTCP:1420 -sTCP:LISTEN >/dev/null; then \
		echo "error: port 1420 is already in use. Stop the existing Vite/Tauri dev process first, then rerun 'make demo-dev'."; \
		exit 1; \
	fi
	pnpm --filter vtx-demo tauri dev

## Watch and rebuild the viz package on changes
dev-viz:
	pnpm --filter @vtx-engine/viz dev

##
## Release targets
##

## Create a new release: bump version, commit, tag, and push (e.g. make release VERSION=0.1.2)
release:
	@test -n "$(VERSION)" || (echo "error: VERSION is required  (e.g. make release VERSION=0.1.2)" && exit 1)
	@bash scripts/release.sh $(VERSION)

##
## Publish targets
##

## Dry-run publish (verify package is ready without uploading)
publish-dry-run:
	cargo publish --dry-run --allow-dirty -p vtx-engine

## Publish vtx-engine to crates.io (requires CARGO_REGISTRY_TOKEN)
publish:
	cargo publish -p vtx-engine

##
## Utility targets
##

## Remove build artifacts
clean:
	cargo clean
	rm -rf packages/vtx-viz/dist
	rm -rf apps/vtx-demo/dist

## Show this help message
help:
	@echo "vtx-engine — available targets:"
	@echo ""
	@grep -E '^## ' Makefile | sed 's/^## /  /'
	@echo ""
	@grep -E '^[a-zA-Z_-]+:' Makefile | grep -v '^\.' | sed 's/:.*//' | awk '{printf "  make %-20s\n", $$1}'
