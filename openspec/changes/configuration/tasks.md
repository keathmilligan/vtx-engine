## 1. Engine Library — EngineConfig Extension

- [x] 1.1 Add `mic_gain_db: f32` field to `EngineConfig` in `crates/vtx-engine/src/lib.rs` with `#[serde(default = "default_mic_gain_db")]` and a `fn default_mic_gain_db() -> f32 { 0.0 }` helper
- [x] 1.2 Add `mic_gain_db` to `EngineConfig::default()` impl with value `0.0`
- [x] 1.3 Verify that loading a TOML config file without a `mic_gain_db` key deserializes cleanly and produces `0.0`

## 2. Engine Library — AudioBackend Gain Trait Method

- [x] 2.1 Add `fn set_gain(&self, db: f32)` to the `AudioBackend` trait in `crates/vtx-engine/src/platform/backend.rs`
- [x] 2.2 Implement `set_gain` as a no-op stub on the Windows WASAPI backend (`platform/windows/`)
- [x] 2.3 Implement `set_gain` as a no-op stub on the macOS CoreAudio backend (`platform/macos/`)
- [x] 2.4 Implement `set_gain` as a no-op stub on the Linux PipeWire backend (`platform/linux/`)

## 3. Engine Library — Software Gain in Capture Pipeline

- [x] 3.1 In `AudioEngine`'s capture loop (`lib.rs`), after receiving raw PCM from the backend, apply a software linear gain multiplier: `linear = 10^(mic_gain_db / 20)`, multiply each sample, and clamp to `[-1.0, 1.0]`
- [x] 3.2 Add a `set_mic_gain(db: f32)` method to `AudioEngine` that updates the stored gain value and calls `backend.set_gain(db)` — this allows hot application without restarting capture
- [x] 3.3 Verify that `mic_gain_db = 0.0` produces no change to the PCM signal (multiplier of 1.0)
- [x] 3.4 Verify that samples clamp correctly and do not overflow when a large positive gain is applied

## 4. Demo Backend — Tauri Commands

- [x] 4.1 Add `get_engine_config` Tauri command to `apps/vtx-demo/src-tauri/src/lib.rs` that locks the engine, reads its stored `EngineConfig`, and returns it as a serialized JSON-compatible value
- [x] 4.2 Add a `config: EngineConfig` field (or equivalent mutable store) to `AppState` or `AudioEngine` so the config can be read and updated independently of engine rebuild
- [x] 4.3 Add `set_engine_config` Tauri command that accepts a full `EngineConfig`, stores it on the engine state, and calls `engine.set_mic_gain(config.mic_gain_db)` for immediate gain application
- [x] 4.4 Register `get_engine_config` and `set_engine_config` in the `tauri::generate_handler![]` macro in `lib.rs`

## 5. Demo Frontend — HTML Structure

- [x] 5.1 Add a gear icon button (`<button id="btn-config">`) to `apps/vtx-demo/index.html` in the `#status-bar`, positioned immediately after the `#model-name` span
- [x] 5.2 Add the modal overlay markup to `index.html`: a backdrop `<div id="config-backdrop">` containing a `<div id="config-modal" role="dialog">` with a header (title + close button), a scrollable body with five `<section>` elements for each settings group, and a footer with "Reset to Defaults" and "Save" buttons
- [x] 5.3 Add a warning banner element (`<div id="config-capture-warning">`) inside the modal, initially hidden, to be shown when capture is active

## 6. Demo Frontend — Styles

- [x] 6.1 Add styles for `#btn-config` (gear button) in `apps/vtx-demo/src/styles.css`: icon sizing, positioning, and hover/focus states consistent with the existing UI
- [x] 6.2 Add styles for the modal backdrop (`#config-backdrop`): full-viewport fixed overlay with semi-transparent background, flex-centered
- [x] 6.3 Add styles for the modal panel (`#config-modal`): max-width, max-height with scroll, padding, border-radius, and background matching the existing panel aesthetic
- [x] 6.4 Add styles for config form sections, labels, inputs (range slider, number inputs, checkbox), and the footer button row
- [x] 6.5 Add styles for the capture-active warning banner (amber/warning color)

## 7. Demo Frontend — AppSettings Extension

- [x] 7.1 Extend the `AppSettings` interface in `apps/vtx-demo/src/main.ts` with fields for all `EngineConfig` tunable parameters (`micGainDb`, `vadVoicedThresholdDb`, `vadWhisperThresholdDb`, `vadVoicedOnsetMs`, `vadWhisperOnsetMs`, `segmentMaxDurationMs`, `segmentWordBreakGraceMs`, `segmentLookbackMs`, `transcriptionQueueCapacity`, `vizFrameIntervalMs`, `wordBreakSegmentationEnabled`) and `audioOutputDeviceId: string`
- [x] 7.2 Update `defaultSettings()` to include all new fields with their corresponding `EngineConfig` default values
- [x] 7.3 Verify that existing `localStorage` blobs without the new fields load correctly (object spread in `loadSettings` handles this automatically — confirm no explicit migration is needed)

## 8. Demo Frontend — Config Panel Logic

- [x] 8.1 Add `openConfigPanel()` function that calls `invoke("get_engine_config")`, populates all form fields with the returned values, enumerates output devices via `navigator.mediaDevices.enumerateDevices()`, shows or hides the `setSinkId` section based on platform support, shows the capture-active warning if `isCapturing` is true, and makes the modal visible
- [x] 8.2 Add `closeConfigPanel()` function that hides the modal and removes the Escape key listener
- [x] 8.3 Wire the gear button click event to `openConfigPanel()`
- [x] 8.4 Wire the close button, Escape key, and backdrop click events to `closeConfigPanel()`
- [x] 8.5 Implement `saveConfig()` function: read all form field values, construct an `EngineConfig`-shaped object, call `invoke("set_engine_config", { config })`, call `audioElement.setSinkId(selectedOutputDeviceId)` if supported and changed, call `saveSettings(updatedSettings)`, then call `closeConfigPanel()`
- [x] 8.6 Implement `resetToDefaults()` function: populate all form fields with the `EngineConfig` factory defaults (matching `defaultSettings()` values) without saving or closing the panel
- [x] 8.7 Wire the Save button click to `saveConfig()` and the Reset to Defaults button to `resetToDefaults()`
- [x] 8.8 Populate the output device `<select>` with options from `enumerateDevices()` filtered to `kind === "audiooutput"`, including a "Default" option with empty value; restore the previously saved `audioOutputDeviceId` on panel open
- [x] 8.9 On application startup, apply the saved `audioOutputDeviceId` from `loadSettings()` to the `<audio>` element via `setSinkId` if supported

## 9. Demo Frontend — Mic Gain Display Value

- [x] 9.1 Add a live readout label next to the mic gain slider that shows the current value in dB (e.g., `+3.0 dB`) and updates on input events

## 10. Integration Verification

- [x] 10.1 Build the Tauri app (`cargo build` in `apps/vtx-demo/src-tauri`) and confirm no compile errors
- [x] 10.2 Build the frontend (`pnpm build` in `apps/vtx-demo`) and confirm no TypeScript errors
- [x] 10.3 Confirm `cargo test` passes in `crates/vtx-engine` (serde round-trip for new `mic_gain_db` field)
