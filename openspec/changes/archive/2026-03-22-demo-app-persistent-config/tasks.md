## 1. Backend - DemoConfig Struct

- [x] 1.1 Add `directories` dependency to `apps/vtx-demo/src-tauri/Cargo.toml`
- [x] 1.2 Create `DemoConfig` struct with all fields from `AppSettings` interface
- [x] 1.3 Implement `Default` for `DemoConfig` matching TypeScript `defaultSettings()`
- [x] 1.4 Add `serde(default)` attributes for backward compatibility

## 2. Backend - Persistence Functions

- [x] 2.1 Create `config_path()` helper using `directories::ProjectDirs`
- [x] 2.2 Implement `DemoConfig::load()` returning defaults if file missing
- [x] 2.3 Implement `DemoConfig::save()` creating directory if needed
- [x] 2.4 Add error handling for file I/O and JSON parse errors

## 3. Backend - Tauri Commands

- [x] 3.1 Add `load_demo_config` Tauri command returning `DemoConfig`
- [x] 3.2 Add `save_demo_config` Tauri command accepting `DemoConfig`
- [x] 3.3 Register commands in `run()` function

## 4. Frontend - Load Settings

- [x] 4.1 Replace `loadSettings()` localStorage call with `invoke("load_demo_config")`
- [x] 4.2 Update `initApp()` to await config load before UI initialization
- [x] 4.3 Remove `SETTINGS_KEY` constant
- [x] 5.1 Replace `saveSettings()` localStorage call with `invoke("save_demo_config")`
- [x] 5.2 Update config panel Save button to call `save_demo_config`
- [x] 5.3 Add `save_demo_config` call on toggle state changes (transcription, auto-transcription, AEC)
- [x] 5.4 Add `save_demo_config` call on device selection changes
- [x] 6.1 Remove `loadSettings()` function
- [x] 6.2 Remove `saveSettings()` function
- [x] 6.3 Remove `defaultSettings()` function (backend provides defaults)
- [x] 6.4 Remove `getCurrentSettings()` function if no longer needed

## 7. Verification

- [x] 7.1 Test config file is created on first save
- [x] 7.2 Test config values persist across app restarts
- [x] 7.3 Test toggle states are restored on app load
- [x] 7.4 Test device selections are restored on app load
- [x] 7.5 Test partial/corrupt config file loads with defaults
