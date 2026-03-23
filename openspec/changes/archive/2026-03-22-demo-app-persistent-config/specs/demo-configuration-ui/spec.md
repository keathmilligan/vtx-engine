## MODIFIED Requirements

### Requirement: Saving configuration applies settings and persists them
Clicking the Save button in the configuration panel SHALL invoke the `set_engine_config` Tauri command with the current form values, update the audio output device on the `<audio>` element via `setSinkId` (if supported and changed), persist all config values to the JSON config file via the `save_demo_config` Tauri command, and close the panel.

#### Scenario: Save sends updated config to backend
- **WHEN** the user changes the voiced threshold value and clicks Save
- **THEN** `set_engine_config` is invoked with an `EngineConfig` object containing the updated value
- **THEN** the panel closes

#### Scenario: Save persists values to JSON config file
- **WHEN** the user changes any config field and clicks Save
- **THEN** `save_demo_config` is invoked with the updated settings
- **THEN** the config file at `{config_dir}/vtx-demo/config.json` contains the updated values

#### Scenario: Save applies audio output device selection
- **WHEN** the user selects a different output device and clicks Save on a platform supporting `setSinkId`
- **THEN** `audioElement.setSinkId(selectedDeviceId)` is called with the selected device ID

### Requirement: AppSettings is extended to persist engine config and output device
The `AppSettings` interface SHALL be extended with fields for all `EngineConfig` tunable parameters and the selected audio output device ID. These fields SHALL use the `EngineConfig` defaults when absent (backward-compatible via object spread merge in `loadSettings`).

#### Scenario: Settings load without error after schema extension
- **WHEN** the config file is loaded
- **THEN** `loadSettings()` returns an object with all fields populated from the file or defaults
- **THEN** no error is thrown

### Requirement: AGC config fields are included in AppSettings persistence
The `AppSettings` TypeScript interface SHALL include `agcEnabled: boolean`, `agcTargetLevelDb: number`, and `agcGateThresholdDb: number` fields. These SHALL be written to the JSON config file on Save and restored on load, merging with defaults when absent.

#### Scenario: AppSettings without AGC fields loads without error
- **WHEN** the config file does not contain `agcEnabled`, `agcTargetLevelDb`, or `agcGateThresholdDb`
- **THEN** `loadSettings()` returns an object with `agcEnabled = false`, `agcTargetLevelDb = -18.0`, and `agcGateThresholdDb = -50.0`
- **THEN** no error is thrown

#### Scenario: AGC fields round-trip through JSON config file
- **WHEN** the user enables AGC, sets target to -20 dB, clicks Save, and reopens the config panel
- **THEN** the AGC enable checkbox is checked and the target level slider shows -20 dB

## REMOVED Requirements

### Requirement: Save persists values to localStorage
**Reason**: Replaced by JSON file persistence via Tauri backend
**Migration**: Settings are now stored in `{config_dir}/vtx-demo/config.json` instead of browser localStorage

### Requirement: AppSettings without AGC fields loads without error (localStorage version)
**Reason**: Replaced by JSON file persistence; localStorage is no longer used
**Migration**: Settings are loaded from JSON config file instead

### Requirement: AGC fields round-trip through localStorage
**Reason**: Replaced by JSON file persistence; localStorage is no longer used
**Migration**: AGC settings are persisted to JSON config file instead
