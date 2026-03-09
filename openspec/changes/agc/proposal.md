## Why

The engine currently applies a fixed software gain (`mic_gain_db`) set manually by the user, leaving them responsible for compensating when input levels are too quiet or too loud. Automatic Gain Control (AGC) removes this burden by continuously adjusting the gain to keep processed audio at a consistent target level — improving transcription accuracy across varied microphones, environments, and speakers.

## What Changes

- Add an `AgcConfig` struct to `EngineConfig` with enable flag, target level, and tuning parameters (attack/release time constants, gain limits).
- Implement a digital AGC algorithm in the capture pipeline using an RMS-based feed-forward envelope follower with configurable attack/release smoothing.
- AGC operates **after** the existing `mic_gain_db` stage (manual gain remains as a coarse pre-gain trim).
- Expose `set_agc_config` and `agc_config` accessor methods on `AudioEngine` for hot-update without restart.
- Broadcast an `AgcGainChanged(f32)` engine event so callers can observe the current AGC gain in real time.
- Add AGC controls to the demo configuration UI (enable toggle, target level slider).
- Persist `AgcConfig` through the existing TOML config persistence mechanism.

## Capabilities

### New Capabilities

- `agc`: Automatic Gain Control — RMS envelope-follower AGC stage in the capture pipeline, with `AgcConfig` configuration, hot-update API, and `AgcGainChanged` broadcast event.

### Modified Capabilities

- `demo-configuration-ui`: Add AGC enable toggle and target level controls to the engine config panel.
- `engine-config-persistence`: `AgcConfig` embedded in `EngineConfig` must round-trip through TOML with sane defaults when the key is absent.
- `broadcast-events`: New `AgcGainChanged(f32)` variant added to the engine event enum.

## Impact

- `crates/vtx-engine/src/lib.rs` — capture loop gain stage, `AudioEngine` struct, public API methods.
- `crates/vtx-engine/src/builder.rs` — initialize AGC state from config at build time.
- `crates/vtx-engine/src/processor.rs` — AGC algorithm implementation (new `AgcProcessor` struct).
- `openspec/specs/broadcast-events/spec.md` — new event variant.
- `openspec/specs/demo-configuration-ui/spec.md` — new UI controls.
- `openspec/specs/engine-config-persistence/spec.md` — `AgcConfig` default-deserialization behavior.
- `apps/vtx-demo/src/main.ts` and `src-tauri/src/lib.rs` — UI bindings and Tauri command pass-through.
- No breaking changes to existing public API; `AgcConfig` defaults to disabled so behavior is unchanged for existing callers.
