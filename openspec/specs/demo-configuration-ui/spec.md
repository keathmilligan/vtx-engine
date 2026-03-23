## Purpose

TBD — Specification for the configuration panel UI in the vtx-engine demo application, allowing users to view and modify engine settings at runtime.

## Requirements

### Requirement: Gear icon button opens the configuration panel
The demo app SHALL display a gear icon button in the status bar, positioned immediately to the right of the `#model-name` badge. Clicking the button SHALL open a modal configuration panel. The button SHALL be accessible via keyboard (focusable, activates on Enter/Space).

#### Scenario: Gear button is present in the status bar
- **WHEN** the application loads
- **THEN** a gear icon button is visible in the status bar to the right of the model name badge

#### Scenario: Clicking the gear button opens the config panel
- **WHEN** the user clicks the gear icon button
- **THEN** the configuration modal panel becomes visible with a backdrop overlay

#### Scenario: Keyboard activation opens the config panel
- **WHEN** the gear icon button has focus and the user presses Enter or Space
- **THEN** the configuration modal panel becomes visible

### Requirement: Configuration panel can be dismissed
The configuration panel SHALL be dismissible by clicking the close button inside the panel, pressing the Escape key, or clicking the backdrop overlay outside the panel.

#### Scenario: Close button dismisses the panel
- **WHEN** the configuration panel is open and the user clicks the close button
- **THEN** the configuration panel is hidden

#### Scenario: Escape key dismisses the panel
- **WHEN** the configuration panel is open and the user presses the Escape key
- **THEN** the configuration panel is hidden

#### Scenario: Backdrop click dismisses the panel
- **WHEN** the configuration panel is open and the user clicks the backdrop overlay outside the panel content area
- **THEN** the configuration panel is hidden

### Requirement: Configuration panel displays current engine config values
When the configuration panel is opened, it SHALL fetch the current `EngineConfig` from the backend via the `get_engine_config` Tauri command and pre-populate all form fields with the returned values.

#### Scenario: Panel fields show current engine values on open
- **WHEN** the user opens the configuration panel
- **THEN** each form field is populated with the value from the current `EngineConfig` as returned by `get_engine_config`

#### Scenario: Panel fetches fresh values each time it is opened
- **WHEN** the user opens the panel, closes it, and opens it again
- **THEN** the fields reflect the most recently saved config values, not stale cached values

### Requirement: Configuration panel includes Model section as first section
The configuration panel SHALL include a Model section positioned before the Audio Input section. The Model section SHALL display all available `WhisperModel` variants with their download status and allow model selection and downloading.

#### Scenario: Model section appears before Audio Input
- **WHEN** the configuration panel is opened
- **THEN** the Model section is visible as the first section
- **THEN** the Audio Input section appears after the Model section

#### Scenario: Model section fetches status on panel open
- **WHEN** the configuration panel is opened
- **THEN** `get_model_status` is invoked to populate the model list
- **THEN** the current `EngineConfig.model` is shown as selected

### Requirement: Configuration panel groups settings into labeled sections
The panel SHALL organize settings into the following labeled sections:
- **Model**: model selection with download status and actions (positioned first)
- **Audio Input**: mic gain (dB slider, range -20 to +20, default 0.0); AGC enable toggle (checkbox); AGC target level (dB slider, range -40 to 0, default -18.0)
- **Voice Detection**: voiced threshold (dB), whisper threshold (dB), voiced onset (ms), whisper onset (ms)
- **Segmentation**: segment max duration (ms), word-break grace period (ms), lookback (ms), word-break segmentation toggle (checkbox), transcription queue capacity
- **Visualization**: viz frame interval (ms)
- **Audio Output**: output device selector (populated from `navigator.mediaDevices.enumerateDevices()` filtered to `audiooutput`; hidden on platforms where `setSinkId` is unavailable)

#### Scenario: All sections and their fields are rendered
- **WHEN** the configuration panel is open
- **THEN** all five section headings and their associated controls are visible
- **THEN** the Audio Input section includes an AGC enable checkbox and an AGC target level slider

#### Scenario: Output device selector is hidden when setSinkId is unsupported
- **WHEN** the configuration panel is open on a platform where `HTMLMediaElement.setSinkId` is not a function
- **THEN** the Audio Output section shows an explanatory note that output device selection is not supported on this platform, and no device selector is rendered

#### Scenario: AGC target level slider is disabled when AGC is unchecked
- **WHEN** the AGC enable checkbox is unchecked
- **THEN** the AGC target level slider is visually disabled and not interactive

#### Scenario: AGC target level slider is enabled when AGC is checked
- **WHEN** the AGC enable checkbox is checked
- **THEN** the AGC target level slider is interactive and its current value is displayed

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

### Requirement: Reset to Defaults restores all fields to factory values
The configuration panel SHALL include a "Reset to Defaults" button that resets all form fields to the `EngineConfig` default values without closing the panel. The user must still click Save to persist the reset values.

#### Scenario: Reset to Defaults populates fields with default values
- **WHEN** the user has modified fields and clicks "Reset to Defaults"
- **THEN** all form fields are reset to the factory default values (e.g., voiced threshold -42.0 dB, word-break segmentation enabled, viz frame interval 16 ms)
- **THEN** the panel remains open

#### Scenario: Reset does not persist until Save is clicked
- **WHEN** the user clicks "Reset to Defaults" and then closes the panel without clicking Save
- **THEN** the previously saved config values are unchanged in the JSON config file and the backend

### Requirement: A warning is shown when configuration changes require capture restart
When the configuration panel is opened while audio capture is active, the panel SHALL display a visible inline warning stating that changes will take effect on the next capture session.

#### Scenario: Warning banner shown during active capture
- **WHEN** the user opens the configuration panel while audio capture is running
- **THEN** a warning banner is displayed inside the panel indicating that changes apply on next capture start

#### Scenario: Warning banner absent when capture is inactive
- **WHEN** the user opens the configuration panel while audio capture is not running
- **THEN** no warning banner is displayed

### Requirement: AppSettings is extended to persist engine config and output device
The `AppSettings` interface SHALL be extended with fields for all `EngineConfig` tunable parameters and the selected audio output device ID. These fields SHALL use the `EngineConfig` defaults when absent (backward-compatible via object spread merge in `loadSettings`).

#### Scenario: Settings load without error after schema extension
- **WHEN** the config file is loaded
- **THEN** `loadSettings()` returns an object with all fields populated from the file or defaults
- **THEN** no error is thrown

### Requirement: get_engine_config and set_engine_config Tauri commands are available
The demo Tauri backend SHALL expose two commands:
- `get_engine_config` — returns the current `EngineConfig` of the active engine as a JSON-serializable object
- `set_engine_config` — accepts a full `EngineConfig` JSON object, validates it, updates the engine's stored config, and applies `mic_gain_db` immediately via the backend's gain setter

#### Scenario: get_engine_config returns current config
- **WHEN** the frontend calls `invoke("get_engine_config")`
- **THEN** it receives a JSON object matching the active engine's `EngineConfig` field values

#### Scenario: set_engine_config updates engine config
- **WHEN** the frontend calls `invoke("set_engine_config", { config: { ...updatedFields } })`
- **THEN** the engine's stored `EngineConfig` is updated with the provided values
- **THEN** `mic_gain_db` is applied immediately via `engine.set_mic_gain(mic_gain_db)`

#### Scenario: set_engine_config does not restart capture
- **WHEN** `set_engine_config` is called while capture is active
- **THEN** capture continues uninterrupted
- **THEN** the new config (except `mic_gain_db`) takes effect on the next `start_capture` call

### Requirement: AGC config fields are included in AppSettings persistence
The `AppSettings` TypeScript interface SHALL include `agcEnabled: boolean`, `agcTargetLevelDb: number`, and `agcGateThresholdDb: number` fields. These SHALL be written to the JSON config file on Save and restored on load, merging with defaults when absent.

#### Scenario: AppSettings without AGC fields loads without error
- **WHEN** the config file does not contain `agcEnabled`, `agcTargetLevelDb`, or `agcGateThresholdDb`
- **THEN** `loadSettings()` returns an object with `agcEnabled = false`, `agcTargetLevelDb = -18.0`, and `agcGateThresholdDb = -50.0`
- **THEN** no error is thrown

#### Scenario: AGC fields round-trip through JSON config file
- **WHEN** the user enables AGC, sets target to -20 dB, clicks Save, and reopens the config panel
- **THEN** the AGC enable checkbox is checked and the target level slider shows -20 dB

### Requirement: set_engine_config Tauri command applies AgcConfig immediately
The `set_engine_config` Tauri command SHALL apply the `agc` field of the provided `EngineConfig` immediately via the engine's `set_agc_config` method, consistent with the immediate application of `mic_gain_db`.

#### Scenario: set_engine_config enables AGC without restart
- **WHEN** the frontend calls `invoke("set_engine_config", { config: { agc: { enabled: true, ... }, ... } })` while capture is active
- **THEN** AGC becomes active immediately without restarting capture
