## Context

The demo app currently persists settings to `localStorage` via the frontend (`main.ts`). This works for web but is not ideal for Tauri desktop apps where users expect file-based persistence in platform-standard locations. The backend already has a pattern for config persistence (`config_persistence.rs`) using the `directories` crate for TOML files.

**Current state:**
- Frontend: `loadSettings()`/`saveSettings()` use `localStorage` with key `vtx-demo-settings`
- Backend: `EngineConfig::load()`/`save()` use TOML at `{config_dir}/{app_name}/vtx-engine.toml`
- `AppSettings` interface contains: model, toggle states, device IDs, engine config fields, AGC fields

## Goals / Non-Goals

**Goals:**
- Persist all demo app settings to a JSON file via Tauri backend
- Use platform-standard config directory (same pattern as engine-config-persistence)
- Load settings on app startup before UI initializes
- Save settings on config panel Save and toggle state changes

**Non-Goals:**
- Migration from existing `localStorage` data (users start fresh)
- Syncing between localStorage and JSON file
- Persisting transcription history or document state

## Decisions

### D1: Use JSON format instead of TOML
**Rationale:** User explicitly requested JSON. The frontend already serializes `AppSettings` to JSON for localStorage, so the TypeScript interface maps directly. TOML would require conversion logic.

**Alternative considered:** TOML for consistency with `engine-config-persistence`. Rejected because:
- Frontend already uses JSON serialization
- JSON is more familiar for web developers
- No need for TOML's human-editing benefits for this use case

### D2: Create `DemoConfig` struct in Tauri backend
**Rationale:** Mirror the TypeScript `AppSettings` interface as a Rust struct with `serde::Serialize`/`Deserialize`. This provides type-safe persistence and validation.

**Fields:**
```rust
struct DemoConfig {
    model: String,
    transcription_enabled: bool,
    auto_transcription_enabled: bool,
    aec_enabled: bool,
    primary_device_id: String,
    secondary_device_id: String,
    mic_gain_db: f64,
    vad_voiced_threshold_db: f64,
    vad_whisper_threshold_db: f64,
    vad_voiced_onset_ms: u32,
    vad_whisper_onset_ms: u32,
    segment_max_duration_ms: u32,
    segment_word_break_grace_ms: u32,
    segment_lookback_ms: u32,
    transcription_queue_capacity: u32,
    viz_frame_interval_ms: u32,
    word_break_segmentation_enabled: bool,
    audio_output_device_id: String,
    agc_enabled: bool,
    agc_target_level_db: f64,
    agc_gate_threshold_db: f64,
}
```

### D3: File location at `{config_dir}/vtx-demo/config.json`
**Rationale:** Use `directories::ProjectDirs` with app name `vtx-demo`, same pattern as `engine-config-persistence`. File named `config.json` for clarity.

**Locations:**
- Linux: `~/.config/vtx-demo/config.json`
- macOS: `~/Library/Application Support/vtx-demo/config.json`
- Windows: `%APPDATA%\vtx-demo\config.json`

### D4: Tauri commands `load_demo_config` and `save_demo_config`
**Rationale:** Simple, explicit commands matching the existing pattern (`get_engine_config`, `set_engine_config`).

**API:**
- `load_demo_config()` → `DemoConfig` (returns defaults if file missing)
- `save_demo_config(config: DemoConfig)` → `void`

### D5: Save on toggle changes via debounced calls
**Rationale:** Toggle state changes (transcription, auto-transcription, AEC) should persist immediately, but we should debounce rapid successive changes to avoid excessive file writes.

**Implementation:** Use a simple debounce (500ms) or save immediately on each toggle change. Given toggle changes are infrequent user actions, immediate save is acceptable.

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| File write fails (permissions, disk full) | Log error, continue with in-memory state; show toast notification |
| Corrupt JSON file on load | Return defaults, log warning, overwrite on next save |
| Race condition between load/save | Tauri commands are async; backend uses single file path, no concurrent writes expected |
| User loses existing localStorage settings | Document in release notes; no migration path (non-goal) |

## Migration Plan

1. Add `DemoConfig` struct and persistence functions to `lib.rs`
2. Add `load_demo_config` and `save_demo_config` Tauri commands
3. Update frontend to call `load_demo_config` on startup instead of `localStorage`
4. Update frontend to call `save_demo_config` on config Save and toggle changes
5. Remove localStorage-related code (`SETTINGS_KEY`, `loadSettings`, `saveSettings`)

**Rollback:** If issues arise, revert frontend to use localStorage. Backend changes are additive and don't affect existing functionality.

## Open Questions

None. Design is straightforward following existing patterns.
