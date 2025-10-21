use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use crate::LoadBalanceStrategy;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct ProxyConfig {
    /// per-route timeouts in milliseconds
    pub route_timeout: HashMap<String, u64>,

    /// A map of service name -> list of addresses
    pub cluster: HashMap<String, Vec<String>>,

    /// Load balancing strategy, defaults to RoundRobin
    pub strategy: LoadBalanceStrategy,
}

impl ProxyConfig {
    pub fn new() -> Self {
        ProxyConfig {
            route_timeout: HashMap::new(),
            cluster: HashMap::new(),
            strategy: LoadBalanceStrategy::RoundRobin,
        }
    }

    pub async fn from_file(path: &str) -> Result<Self, crate::error::ProxyError> {
        let mut file = File::open(path).await?;
        let mut content: String = String::new();
        file.read_to_string(&mut content).await?;
        ProxyConfig::from_content(&content).await
    }

    pub async fn from_content(content: &str) -> Result<Self, crate::error::ProxyError> {
        println!("CONFIG CONTENT:\n{}", content);
        let config: ProxyConfig = toml::from_str(content)?;
        Ok(config)
    }

    pub fn to_json(&self) -> Result<String, crate::error::ProxyError> {
        serde_json::to_string(self)
            .map_err(|e| crate::error::ProxyError::JSONError(e))
    }
}
