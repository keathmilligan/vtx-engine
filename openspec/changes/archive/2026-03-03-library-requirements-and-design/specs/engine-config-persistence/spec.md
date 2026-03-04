## ADDED Requirements

### Requirement: EngineConfig is serializable and deserializable
`EngineConfig` SHALL derive `serde::Serialize` and `serde::Deserialize`. All fields SHALL use `#[serde(default)]` so that a config file written by an older version of the library can be loaded by a newer version without error.

#### Scenario: Partial config file loads with defaults for missing fields
- **WHEN** a TOML file containing only `recording_mode = "echo_cancel"` is loaded as `EngineConfig`
- **THEN** all other fields take their `Default` values without returning an error

### Requirement: EngineConfig can be saved to a platform-standard directory
`EngineConfig` SHALL expose a `save(app_name: &str) -> Result<(), ConfigError>` method. The method SHALL serialize the config to TOML and write it to `{config_dir}/{app_name}/vtx-engine.toml`, where `config_dir` is resolved by the `directories::ProjectDirs` API using `app_name` as the application name. The directory SHALL be created if it does not exist.

#### Scenario: Save creates the config file
- **WHEN** `config.save("my-app")` is called and the config directory does not exist
- **THEN** the directory is created and `vtx-engine.toml` is written with the serialized config
- **THEN** the method returns `Ok(())`

#### Scenario: Save overwrites an existing config file
- **WHEN** `config.save("my-app")` is called and `vtx-engine.toml` already exists
- **THEN** the existing file is replaced with the new serialized content

### Requirement: EngineConfig can be loaded from the platform-standard directory
`EngineConfig` SHALL expose a `load(app_name: &str) -> Result<EngineConfig, ConfigError>` associated function. If the config file does not exist, the method SHALL return `Ok(EngineConfig::default())`. If the file exists but fails to parse, the method SHALL return `Err(ConfigError::Parse(...))`.

#### Scenario: Load returns default when no file exists
- **WHEN** `EngineConfig::load("my-app")` is called and no config file exists
- **THEN** `Ok(EngineConfig::default())` is returned

#### Scenario: Load returns saved values
- **WHEN** `config.save("my-app")` has been called and then `EngineConfig::load("my-app")` is called
- **THEN** the loaded config equals the saved config

#### Scenario: Load returns parse error for corrupt file
- **WHEN** `vtx-engine.toml` contains invalid TOML and `EngineConfig::load("my-app")` is called
- **THEN** `Err(ConfigError::Parse(_))` is returned

### Requirement: ConfigError provides actionable diagnostics
`ConfigError` SHALL be a public enum with at least the variants: `Io(std::io::Error)`, `Parse(String)`, and `NoProjectDir` (returned when `directories::ProjectDirs` cannot resolve a path for the given app name).

#### Scenario: ConfigError::NoProjectDir returned for empty app name
- **WHEN** `EngineConfig::load("")` is called on a platform where project dirs cannot be determined for an empty name
- **THEN** `Err(ConfigError::NoProjectDir)` is returned
