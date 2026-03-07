//! Config persistence helpers for [`EngineConfig`].
//!
//! Config is stored as TOML at:
//! `{config_dir}/{app_name}/vtx-engine.toml`

use std::fmt;

use directories::ProjectDirs;

use crate::EngineConfig;

const CONFIG_FILENAME: &str = "vtx-engine.toml";

/// Errors that can occur during config load or save.
#[derive(Debug)]
pub enum ConfigError {
    /// I/O error reading or writing the config file.
    Io(std::io::Error),
    /// TOML parse error.
    Parse(String),
    /// Platform config directory could not be determined.
    NoProjectDir,
    /// Serialization error converting config to TOML.
    Serialize(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Io(e) => write!(f, "I/O error: {}", e),
            ConfigError::Parse(s) => write!(f, "Parse error: {}", s),
            ConfigError::NoProjectDir => write!(f, "Cannot determine config directory"),
            ConfigError::Serialize(s) => write!(f, "Serialization error: {}", s),
        }
    }
}

impl std::error::Error for ConfigError {}

impl EngineConfig {
    /// Load configuration from `{config_dir}/{app_name}/vtx-engine.toml`.
    ///
    /// Returns `Ok(EngineConfig::default())` if the file does not exist.
    /// Returns `Err(ConfigError::Parse(...))` if the file cannot be parsed.
    pub fn load(app_name: &str) -> Result<EngineConfig, ConfigError> {
        let path = config_path(app_name)?;
        if !path.exists() {
            return Ok(EngineConfig::default());
        }

        let content = std::fs::read_to_string(&path).map_err(ConfigError::Io)?;
        toml::from_str(&content).map_err(|e| ConfigError::Parse(e.to_string()))
    }

    /// Save configuration to `{config_dir}/{app_name}/vtx-engine.toml`.
    ///
    /// Creates parent directories if they do not exist.
    pub fn save(&self, app_name: &str) -> Result<(), ConfigError> {
        let path = config_path(app_name)?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(ConfigError::Io)?;
        }

        let content =
            toml::to_string_pretty(self).map_err(|e| ConfigError::Serialize(e.to_string()))?;

        std::fs::write(&path, content).map_err(ConfigError::Io)
    }
}

fn config_path(app_name: &str) -> Result<std::path::PathBuf, ConfigError> {
    if app_name.is_empty() {
        return Err(ConfigError::NoProjectDir);
    }
    let dirs = ProjectDirs::from("", "", app_name).ok_or(ConfigError::NoProjectDir)?;
    Ok(dirs.config_dir().join(CONFIG_FILENAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Task 1.3: A TOML file without `mic_gain_db` deserializes cleanly and
    /// produces the default value of 0.0.
    #[test]
    fn partial_toml_without_mic_gain_gets_default() {
        let toml = r#"recording_mode = "echo_cancel""#;
        let config: EngineConfig = toml::from_str(toml).expect("should parse");
        assert_eq!(config.mic_gain_db, 0.0);
    }

    /// Task 1.3 (complementary): A full round-trip preserves mic_gain_db.
    #[test]
    fn mic_gain_db_round_trips() {
        let mut config = EngineConfig::default();
        config.mic_gain_db = 6.0;
        let toml_str = toml::to_string_pretty(&config).expect("should serialize");
        let loaded: EngineConfig = toml::from_str(&toml_str).expect("should deserialize");
        assert_eq!(loaded.mic_gain_db, 6.0);
    }
}
