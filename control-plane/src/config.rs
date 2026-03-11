use mesh_core::config::{Config, ProxyConfig};
use std::path::Path;
use tokio::{fs::File, io::AsyncReadExt};

type Result<T> = std::result::Result<T, crate::error::Error>;

pub async fn from_file(path: impl AsRef<Path>) -> Result<Config> {
    let mut file = File::open(path).await?;
    let mut content: String = String::new();
    file.read_to_string(&mut content).await?;
    Ok(from_content(&content)?)
}

pub fn from_content(content: &str) -> Result<Config> {
    println!("CONFIG CONTENT:\n{}", content);
    let config: Config = toml::from_str(&content)?;
    Ok(config)
}

pub fn to_json(config: &ProxyConfig) -> Result<String> {
    serde_json::to_string(config).map_err(|e| e.into())
}
