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
