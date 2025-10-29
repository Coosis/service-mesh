use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use crate::strategy::LoadBalanceStrategy;

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ProxyConfig<E> {
    /// per-route timeouts in milliseconds
    pub route_timeout: HashMap<String, u64>,

    /// A map of service name -> list of addresses
    pub cluster: HashMap<String, Vec<String>>,

    /// Load balancing strategy, defaults to RoundRobin
    pub strategy: LoadBalanceStrategy,

    #[serde(skip, default)]
    _marker: std::marker::PhantomData<E>,
}

impl<E> ProxyConfig<E> 
where E: From<std::io::Error> + From<serde_json::Error> + From<toml::de::Error>
{
    pub fn new() -> Self {
        ProxyConfig {
            route_timeout: HashMap::new(),
            cluster: HashMap::new(),
            strategy: LoadBalanceStrategy::RoundRobin,

            _marker: std::marker::PhantomData,
        }
    }

    pub async fn from_file(path: &str) -> Result<Self, E> {
        let mut file = File::open(path).await?;
        let mut content: String = String::new();
        file.read_to_string(&mut content).await?;
        ProxyConfig::from_content(&content).await
    }

    pub async fn from_content(content: &str) -> Result<Self, E> {
        println!("CONFIG CONTENT:\n{}", content);
        let config: ProxyConfig<E> = toml::from_str(content)?;
        Ok(config)
    }

    pub fn to_json(&self) -> Result<String, E> {
        serde_json::to_string(self)
            .map_err(|e| e.into())
    }
}

impl<E> Clone for ProxyConfig<E> {
    fn clone(&self) -> Self {
        ProxyConfig {
            route_timeout: self.route_timeout.clone(),
            cluster: self.cluster.clone(),
            strategy: self.strategy.clone(),
            _marker: std::marker::PhantomData,
        }
    }

}
