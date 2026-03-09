## MODIFIED Requirements

### Requirement: Configuration panel groups settings into labeled sections
The panel SHALL organize settings into the following labeled sections:
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

## ADDED Requirements

### Requirement: AGC config fields are included in AppSettings persistence
The `AppSettings` TypeScript interface SHALL include `agcEnabled: boolean` and `agcTargetLevelDb: number` fields. These SHALL be written to `localStorage` on Save and restored on load, merging with defaults when absent.

#### Scenario: AppSettings without AGC fields loads without error
- **WHEN** `localStorage` contains an `AppSettings` blob written before this change (without `agcEnabled` or `agcTargetLevelDb`)
- **THEN** `loadSettings()` returns an object with `agcEnabled = false` and `agcTargetLevelDb = -18.0`
- **THEN** no error is thrown

#### Scenario: AGC fields round-trip through localStorage
- **WHEN** the user enables AGC, sets target to -20 dB, clicks Save, and reopens the config panel
- **THEN** the AGC enable checkbox is checked and the target level slider shows -20 dB

### Requirement: set_engine_config Tauri command applies AgcConfig immediately
The `set_engine_config` Tauri command SHALL apply the `agc` field of the provided `EngineConfig` immediately via the engine's `set_agc_config` method, consistent with the immediate application of `mic_gain_db`.

#### Scenario: set_engine_config enables AGC without restart
- **WHEN** the frontend calls `invoke("set_engine_config", { config: { agc: { enabled: true, ... }, ... } })` while capture is active
- **THEN** AGC becomes active immediately without restarting capture
