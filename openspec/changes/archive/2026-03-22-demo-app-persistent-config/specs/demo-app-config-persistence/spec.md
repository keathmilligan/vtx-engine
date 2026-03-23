## Purpose

JSON file persistence for demo app settings, including engine config, toggle states, and device selections.

## Requirements

### Requirement: DemoConfig is serializable and deserializable
`DemoConfig` SHALL derive `serde::Serialize` and `serde::Deserialize`. All fields SHALL use `#[serde(default)]` so that a config file written by an older version can be loaded by a newer version without error.

#### Scenario: Partial config file loads with defaults for missing fields
- **WHEN** a JSON file containing only `{"model": "base_en"}` is loaded as `DemoConfig`
- **THEN** all other fields take their `Default` values without returning an error

#### Scenario: Config file round-trips through save and load
- **WHEN** a `DemoConfig` with `transcription_enabled = false` and `mic_gain_db = 5.0` is saved and reloaded
- **THEN** the reloaded `transcription_enabled` is `false` and `mic_gain_db` is `5.0`

### Requirement: DemoConfig can be saved to a platform-standard directory
`DemoConfig` SHALL expose a `save() -> Result<(), ConfigError>` method. The method SHALL serialize the config to JSON and write it to `{config_dir}/vtx-demo/config.json`, where `config_dir` is resolved by the `directories::ProjectDirs` API. The directory SHALL be created if it does not exist.

#### Scenario: Save creates the config file
- **WHEN** `config.save()` is called and the config directory does not exist
- **THEN** the directory is created and `config.json` is written with the serialized config
- **THEN** the method returns `Ok(())`

#### Scenario: Save overwrites an existing config file
- **WHEN** `config.save()` is called and `config.json` already exists
- **THEN** the existing file is replaced with the new serialized content

### Requirement: DemoConfig can be loaded from the platform-standard directory
`DemoConfig` SHALL expose a `load() -> Result<DemoConfig, ConfigError>` associated function. If the config file does not exist, the method SHALL return `Ok(DemoConfig::default())`. If the file exists but fails to parse, the method SHALL return `Err(ConfigError::Parse(...))`.

#### Scenario: Load returns default when no file exists
- **WHEN** `DemoConfig::load()` is called and no config file exists
- **THEN** `Ok(DemoConfig::default())` is returned

#### Scenario: Load returns saved values
- **WHEN** `config.save()` has been called and then `DemoConfig::load()` is called
- **THEN** the loaded config equals the saved config

#### Scenario: Load returns parse error for corrupt file
- **WHEN** `config.json` contains invalid JSON and `DemoConfig::load()` is called
- **THEN** `Err(ConfigError::Parse(_))` is returned

### Requirement: load_demo_config Tauri command is available
The demo Tauri backend SHALL expose a `load_demo_config` command that returns the current `DemoConfig` from the config file, or default values if the file does not exist.

#### Scenario: load_demo_config returns saved config
- **WHEN** the frontend calls `invoke("load_demo_config")` and a config file exists
- **THEN** it receives a JSON object matching the saved `DemoConfig` values

#### Scenario: load_demo_config returns defaults when no file exists
- **WHEN** the frontend calls `invoke("load_demo_config")` and no config file exists
- **THEN** it receives a JSON object with all default values

### Requirement: save_demo_config Tauri command is available
The demo Tauri backend SHALL expose a `save_demo_config` command that accepts a full `DemoConfig` JSON object and persists it to the config file.

#### Scenario: save_demo_config persists config to file
- **WHEN** the frontend calls `invoke("save_demo_config", { config: { ...settings } })`
- **THEN** the config is written to `{config_dir}/vtx-demo/config.json`
- **THEN** the method returns `Ok(())`

#### Scenario: save_demo_config overwrites existing config
- **WHEN** `save_demo_config` is called and a config file already exists
- **THEN** the existing file is replaced with the new content

### Requirement: DemoConfig includes all AppSettings fields
`DemoConfig` SHALL include fields for: model, transcription_enabled, auto_transcription_enabled, aec_enabled, primary_device_id, secondary_device_id, mic_gain_db, vad_voiced_threshold_db, vad_whisper_threshold_db, vad_voiced_onset_ms, vad_whisper_onset_ms, segment_max_duration_ms, segment_word_break_grace_ms, segment_lookback_ms, transcription_queue_capacity, viz_frame_interval_ms, word_break_segmentation_enabled, audio_output_device_id, agc_enabled, agc_target_level_db, agc_gate_threshold_db.

#### Scenario: All settings fields are persisted
- **WHEN** the user modifies any setting and saves
- **THEN** the modified value is present in the saved `config.json` file

#### Scenario: All settings fields are restored on load
- **WHEN** the app loads a saved config file
- **THEN** all UI controls reflect the saved values
