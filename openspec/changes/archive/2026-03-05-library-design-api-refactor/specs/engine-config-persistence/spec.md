## MODIFIED Requirements

### Requirement: EngineConfig is serializable and deserializable
`EngineConfig` SHALL derive `serde::Serialize` and `serde::Deserialize`. All fields SHALL use `#[serde(default)]` so that a config file written by an older version of the library can be loaded by a newer version without error. The new fields `model`, `word_break_segmentation_enabled`, `max_segment_duration_ms` (renamed from `segment_max_duration_ms` for clarity — or keeping the existing name) SHALL all use `#[serde(default)]`. The deprecated `model_path` field SHALL use `#[serde(default, skip_serializing_if = "Option::is_none")]` so it is omitted when saving new configs but still read from old files.

#### Scenario: Partial config file loads with defaults for missing fields
- **WHEN** a TOML file containing only `recording_mode = "echo_cancel"` is loaded as `EngineConfig`
- **THEN** all other fields take their `Default` values without returning an error

#### Scenario: Old config with model_path loads and model field gets a default
- **WHEN** a TOML file containing `model_path = "/some/path/ggml-base.en.bin"` and no `model` key is loaded
- **THEN** `EngineConfig::model_path` is `Some("/some/path/ggml-base.en.bin")`
- **THEN** `EngineConfig::model` takes its default value (`WhisperModel::BaseEn`)

#### Scenario: New config with model field serializes without model_path
- **WHEN** a default `EngineConfig` (with `model_path = None`) is serialized to TOML
- **THEN** the TOML output does not contain a `model_path` key
- **THEN** the TOML output contains a `model = "base_en"` key

## ADDED Requirements

### Requirement: model_path takes precedence over model when both are set
When loading an `EngineConfig` where both `model_path` and `model` are present, the engine's path resolution SHALL prefer `model_path` so that legacy explicit-path configs continue to work. A deprecation warning SHALL be logged at `tracing::warn` level when `model_path` is used.

#### Scenario: model_path overrides model in path resolution
- **WHEN** `EngineConfig { model_path: Some("/custom/path/ggml-small.en.bin"), model: WhisperModel::BaseEn, .. }` is used to build an engine
- **THEN** the whisper library is loaded from `/custom/path/ggml-small.en.bin`, not from the `BaseEn` cache path
- **THEN** a `warn` level log message mentions that `model_path` is deprecated
