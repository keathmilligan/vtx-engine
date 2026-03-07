## Why

The demo app exposes no way to tune engine parameters at runtime — users must recompile to change VAD thresholds, segment durations, or similar settings. Adding a configuration panel gives users and developers a practical way to explore and adjust the engine's tunable parameters without touching code.

## What Changes

- Add a gear icon button to the status bar, positioned to the right of the model name badge
- Clicking the gear opens a modal configuration panel
- The panel exposes the tunable `EngineConfig` fields: VAD thresholds, segment timing, word-break segmentation toggle, visualization frame interval, and transcription queue capacity
- The panel also exposes microphone input gain (a new capability to be added to `EngineConfig` and the audio backend)
- Audio output device selection is exposed in the panel (demo playback device for the HTML `<audio>` element, using the browser's `setSinkId` API — no engine changes required)
- Settings are persisted to the existing `localStorage` `AppSettings` store and applied on next capture start
- A "Reset to Defaults" button restores all values to factory defaults
- New Tauri commands: `get_engine_config` and `set_engine_config` allow the frontend to read and write the active `EngineConfig`

## Capabilities

### New Capabilities

- `demo-configuration-ui`: Modal configuration panel in the demo app — gear icon trigger, grouped settings fields, reset-to-defaults, and persistence via localStorage

### Modified Capabilities

- `engine-config-persistence`: `EngineConfig` gains a `mic_gain_db` field (f32, default 0.0 dB). The persistence format is extended accordingly. Existing saved configs without this field deserialize cleanly via serde default.

## Impact

- **`crates/vtx-engine/src/lib.rs`**: Add `mic_gain_db: f32` field to `EngineConfig`; add gain application in the audio capture pipeline
- **`crates/vtx-engine/src/platform/backend.rs`**: Add `set_gain(db: f32)` to the `AudioBackend` trait; implement on Windows (WASAPI), macOS (CoreAudio), and Linux (PipeWire) backends
- **`apps/vtx-demo/src-tauri/src/lib.rs`**: Add `get_engine_config` and `set_engine_config` Tauri commands
- **`apps/vtx-demo/src/main.ts`**: Add gear button, modal panel UI, settings form, `AppSettings` extension for new fields
- **`apps/vtx-demo/index.html`**: Add gear icon button markup next to `#model-name` badge
- **`apps/vtx-demo/src/styles.css`**: Modal, form, and gear button styles
- No public API changes to the library crate beyond the new `mic_gain_db` field (non-breaking with serde default)
