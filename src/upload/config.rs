use std::{default::Default, str::FromStr};

use serde::Deserialize;
use url::Url;

/// All the knobs that drive `upload_file`
#[derive(Debug, Clone, Deserialize)]
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
            server: Url::from_str("http://localhost:8020")
                .expect("Parsing URL from str should be verified by a test"),
            chunk_size: 5,
            retries: 3,
            timeout_seconds: 600,
        }
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
        assert_eq!(config.server.scheme(), "http");
        assert_eq!(config.server.host_str(), Some("localhost"));
        assert_eq!(config.server.port(), Some(8020));

        // Verify other default values
        assert_eq!(config.chunk_size, 5);
        assert_eq!(config.retries, 3);
        assert_eq!(config.timeout_seconds, 600);
    }
}
