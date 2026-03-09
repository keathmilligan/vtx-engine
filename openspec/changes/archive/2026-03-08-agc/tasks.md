## 1. Data Structures

- [x] 1.1 Add `AgcConfig` struct to `crates/vtx-engine/src/lib.rs` with all six fields, serde derives, and `Default` impl matching the documented defaults
- [x] 1.2 Embed `pub agc: AgcConfig` in `EngineConfig` with `#[serde(default)]`
- [x] 1.3 Add `AgcGainChanged(f32)` variant to the `EngineEvent` enum

## 2. AgcProcessor Algorithm

- [x] 2.1 Add `AgcProcessor` struct to `crates/vtx-engine/src/processor.rs` with fields: `power_estimate: f32`, `current_gain_linear: f32`, `config: AgcConfig`, `chunks_since_event: u32`
- [x] 2.2 Implement `AgcProcessor::new(config: AgcConfig) -> Self` initialising state to neutral (power 1e-6, gain 1.0)
- [x] 2.3 Implement `AgcProcessor::update_config(&mut self, config: AgcConfig)` for hot-update
- [x] 2.4 Implement `AgcProcessor::process(&mut self, samples: &mut [f32], sample_rate: u32) -> Option<f32>` — RMS envelope follower per the design spec; returns `Some(gain_db)` when it's time to emit an event (every ~100 ms), otherwise `None`
- [x] 2.5 Implement `AgcProcessor::current_gain_db(&self) -> f32`
- [x] 2.6 Write unit test: unity gain convergence on a signal already at target level
- [x] 2.7 Write unit test: gain increases for quiet input (-40 dBFS → converges toward -18 dBFS)
- [x] 2.8 Write unit test: gain decreases for loud input (0 dBFS → falls toward -18 dBFS)
- [x] 2.9 Write unit test: gain is clamped to `max_gain_db`
- [x] 2.10 Write unit test: all-zero input does not produce NaN, infinity, or gain exceeding `max_gain_db`

## 3. Engine Integration

- [x] 3.1 In `crates/vtx-engine/src/builder.rs`, extract `config.agc` and store as `Arc<Mutex<AgcConfig>>` on `AudioEngine`, initialising `AgcProcessor` state from it
- [x] 3.2 Add `agc_config: Arc<Mutex<AgcConfig>>` field to the `AudioEngine` struct in `lib.rs`
- [x] 3.3 Implement `AudioEngine::set_agc_config(&self, config: AgcConfig)` — locks and replaces the shared config
- [x] 3.4 Implement `AudioEngine::agc_config(&self) -> AgcConfig` — locks and clones the current config
- [x] 3.5 In `AudioEngine::set_config`, call `self.set_agc_config(config.agc.clone())` immediately alongside the existing `set_mic_gain` call
- [x] 3.6 In the capture loop (`lib.rs`), clone the `Arc<Mutex<AgcConfig>>` before spawning the thread
- [x] 3.7 In the capture loop, instantiate `AgcProcessor` from the initial config before the loop body
- [x] 3.8 In the capture loop, after the manual gain stage: `try_lock` the config, call `processor.update_config` if changed, then call `processor.process` when `enabled` is true
- [x] 3.9 In the capture loop, when `process` returns `Some(gain_db)`, send `EngineEvent::AgcGainChanged(gain_db)` on the broadcast sender

## 4. Config Persistence

- [x] 4.1 Write unit test: `EngineConfig` TOML without `[agc]` deserialises with `agc.enabled = false`
- [x] 4.2 Write unit test: `AgcConfig` round-trips through `EngineConfig::save` / `EngineConfig::load`

## 5. Demo App — Tauri Backend

- [x] 5.1 In `apps/vtx-demo/src-tauri/src/lib.rs`, add `agc` field to the `EngineConfig` TypeScript-facing struct (if a separate mirror struct is used) or confirm pass-through serialization is sufficient
- [x] 5.2 In the `set_engine_config` Tauri command, call `engine.set_agc_config(config.agc.clone())` immediately alongside `set_mic_gain`

## 6. Demo App — Frontend UI

- [x] 6.1 Add `agcEnabled: boolean` and `agcTargetLevelDb: number` to the `AppSettings` TypeScript interface in `apps/vtx-demo/src/main.ts`
- [x] 6.2 Add default values `agcEnabled: false` and `agcTargetLevelDb: -18.0` to `defaultSettings()`
- [x] 6.3 Add `agc_enabled` checkbox and `agc_target_level_db` range slider (range -40 to 0, step 0.5) under the Audio Input section in the config panel HTML
- [x] 6.4 Implement DOM refs and `updateAgcTargetDisplay` helper (analogous to `updateGainDisplay`)
- [x] 6.5 In `populateConfigForm`, populate AGC checkbox and slider from config; enable/disable slider based on checkbox state
- [x] 6.6 In `readConfigForm`, read AGC checkbox and slider values into the `EngineConfig` object
- [x] 6.7 Wire checkbox `change` event to toggle the enabled/disabled state of the target level slider in real time
- [x] 6.8 In `AppSettings` save/restore logic, persist and restore `agcEnabled` and `agcTargetLevelDb`
- [x] 6.9 Update the `EngineConfig` TypeScript interface to include `agc: { enabled: boolean; target_level_db: number; attack_time_ms: number; release_time_ms: number; min_gain_db: number; max_gain_db: number }`
