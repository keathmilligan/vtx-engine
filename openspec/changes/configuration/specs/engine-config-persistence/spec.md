## MODIFIED Requirements

### Requirement: EngineConfig is serializable and deserializable
`EngineConfig` SHALL derive `serde::Serialize` and `serde::Deserialize`. All fields SHALL use `#[serde(default)]` so that a config file written by an older version of the library can be loaded by a newer version without error. The fields `model`, `word_break_segmentation_enabled`, `segment_max_duration_ms`, and `mic_gain_db` SHALL all use `#[serde(default)]`. The deprecated `model_path` field SHALL use `#[serde(default, skip_serializing_if = "Option::is_none")]` so it is omitted when saving new configs but still read from old files.

#### Scenario: Partial config file loads with defaults for missing fields
- **WHEN** a TOML file containing only `recording_mode = "echo_cancel"` is loaded as `EngineConfig`
- **THEN** all other fields take their `Default` values without returning an error

#### Scenario: Config file without mic_gain_db loads with default gain
- **WHEN** a TOML file written before this change (without a `mic_gain_db` key) is loaded as `EngineConfig`
- **THEN** `mic_gain_db` takes its default value of `0.0` without returning an error
