## Why

The demo app currently persists settings to `localStorage`, which is browser-specific and not shared across sessions when running as a Tauri desktop app. Users expect their configuration (model selection, audio settings, device selections, toggle states) to persist reliably across app restarts. A JSON file in the platform-standard config directory provides consistent, portable persistence for the Tauri desktop experience.

## What Changes

- Replace `localStorage` persistence with JSON file persistence via Tauri backend
- Persist all config screen items (model, audio input, voice detection, segmentation, visualization, audio output settings)
- Persist main screen toggle states (transcription, auto-transcription, AEC) and device selections (primary/secondary input devices)
- Load settings from JSON file on app startup
- Save settings to JSON file on config panel Save and toggle state changes

## Capabilities

### New Capabilities
- `demo-app-config-persistence`: JSON file persistence for demo app settings, including engine config, toggle states, and device selections

### Modified Capabilities
- `demo-configuration-ui`: Change persistence mechanism from `localStorage` to JSON file via Tauri backend commands

## Impact

- **Frontend**: `apps/vtx-demo/src/main.ts` - replace `loadSettings`/`saveSettings` localStorage calls with Tauri invoke commands
- **Backend**: `apps/vtx-demo/src-tauri/src/lib.rs` - add `load_demo_config` and `save_demo_config` Tauri commands
- **Config file location**: Platform-standard config directory (e.g., `~/.config/vtx-demo/config.json` on Linux, `~/Library/Application Support/vtx-demo/config.json` on macOS)
- **Backward compatibility**: Existing `localStorage` settings will not be migrated; users start fresh
