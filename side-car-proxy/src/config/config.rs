use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::AsyncReadExt;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// per-route timeouts in milliseconds
    pub route_timeout: HashMap<String, u64>
}

impl ProxyConfig {
    pub fn new() -> Self {
        ProxyConfig {
            route_timeout: HashMap::new(),
        }
    }

    pub async fn from_file(path: &str) -> Result<Self, crate::error::ProxyError> {
        let mut file = File::open(path).await?;
        let mut content: String = String::new();
        file.read_to_string(&mut content).await?;
        ProxyConfig::from_content(&content).await
    }

    pub async fn from_content(content: &str) -> Result<Self, crate::error::ProxyError> {
        let config: ProxyConfig = toml::from_str(content)?;
        Ok(config)
    }
}
