use std::{
    default::Default,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::Deserialize;
use thiserror::Error;
use url::Url;

/// All the knobs that drive `upload_file`
///
/// Missing keys fall back to the values from [`UploadConfig::default`], so a
/// config file only needs to specify the settings that deviate from the
/// defaults.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UploadConfig {
    /// Base URL of the OnMCU server
    pub server: Url,

    /// How many MiB per chunk, maximum is 10
    pub chunk_size: usize,

    /// How many retries per chunk
    pub retries: u8,

    /// Job timeout in seconds (59-86400)
    pub timeout_seconds: u32,
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self {
            server: Url::from_str("https://ctrl1.onmcu.com")
                .expect("Parsing URL from str should be verified by a test"),
            chunk_size: 5,
            retries: 3,
            timeout_seconds: 600,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Could not read config file at {path:?}. Error: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Failed to parse config file at {path:?}. Error: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

impl UploadConfig {
    /// Load settings from a TOML file.
    pub fn from_file(path: &Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_owned(),
            source,
        })?;

        toml::from_str(&contents).map_err(|source| ConfigError::Parse {
            path: path.to_owned(),
            source,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_does_not_panic() {
        // This test verifies that the default URL parsing doesn't panic
        let config = UploadConfig::default();

        // Verify the URL was parsed correctly
        assert_eq!(config.server.scheme(), "https");
        assert_eq!(config.server.host_str(), Some("ctrl1.onmcu.com"));
        assert_eq!(config.server.port(), None);

        // Verify other default values
        assert_eq!(config.chunk_size, 5);
        assert_eq!(config.retries, 3);
        assert_eq!(config.timeout_seconds, 600);
    }

    #[test]
    fn test_empty_config_uses_defaults() {
        let config: UploadConfig = toml::from_str("").expect("empty config should parse");
        let defaults = UploadConfig::default();

        assert_eq!(config.server, defaults.server);
        assert_eq!(config.chunk_size, defaults.chunk_size);
        assert_eq!(config.retries, defaults.retries);
        assert_eq!(config.timeout_seconds, defaults.timeout_seconds);
    }

    #[test]
    fn test_partial_config_fills_missing_keys() {
        // Only override a single key; the rest must fall back to defaults.
        let config: UploadConfig =
            toml::from_str("retries = 7").expect("partial config should parse");
        let defaults = UploadConfig::default();

        assert_eq!(config.retries, 7);
        assert_eq!(config.server, defaults.server);
        assert_eq!(config.chunk_size, defaults.chunk_size);
        assert_eq!(config.timeout_seconds, defaults.timeout_seconds);
    }

    #[test]
    fn test_unknown_key_is_rejected() {
        // A misspelled key must be reported instead of silently ignored.
        let result: Result<UploadConfig, _> = toml::from_str("retris = 7");
        assert!(result.is_err());
    }
}
