use crate::Result;
use mesh_core::config::ProxyConfig;
use tracing::{debug, info};

pub fn from_content(content: &str) -> Result<ProxyConfig> {
    debug!("CONFIG CONTENT:\n{}", content);
    let mut config: ProxyConfig = serde_json::from_str(content)?;
    match std::env::var("TLS_CRT") {
        Ok(crt_path) => {
            info!(
                "TLS_CRT env var detected, overriding config with crt path: {}",
                crt_path
            );
            config.tls.replace_cert(crt_path);
            // config.tls_cfg = mesh_core::config::TlsConfig::Plain(crt_path.into(), key);
        }
        Err(_) => {}
    }
    match std::env::var("TLS_KEY") {
        Ok(key_path) => {
            info!(
                "TLS_KEY env var detected, overriding config with key path: {}",
                key_path
            );
            config.tls.replace_key(key_path);
        }
        Err(_) => {}
    }

    Ok(config)
}

// pub fn from_file(path: &str) -> Result<ProxyConfig> {
//     let content = std::fs::read_to_string(path)?;
//     from_content(&content)
// }
//
// pub fn to_json(config: &ProxyConfig) -> Result<String> {
//     serde_json::to_string(config)
//         .map_err(|e| e.into())
// }
