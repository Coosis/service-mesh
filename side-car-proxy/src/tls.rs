use std::sync::Arc;
use rustls::ServerConfig;
use rustls_pki_types::{pem::PemObject, CertificateDer, PrivateKeyDer};

use crate::{error::ProxyError, Result};

pub fn get_server_config(
    fullchain: impl AsRef<std::path::Path>,
    key: impl AsRef<std::path::Path>,
) ->Result<Arc<ServerConfig>> {
    if !fullchain.as_ref().exists() {
        return Err(ProxyError::FileNotFound("fullchain not found".to_string()));
    }
    if !key.as_ref().exists() {
        return Err(ProxyError::FileNotFound("key not found".to_string()));
    }
    let certs: Vec<CertificateDer<'static>> = 
        CertificateDer::pem_file_iter(fullchain)
        .map_err(|_| ProxyError::CertOpenError)?
        .map(|c| c.map_err(|_| ProxyError::CertMalformedError))
        .filter_map(Result::ok)
        .collect();
    let key = PrivateKeyDer::from_pem_file(key)
        .unwrap();
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| crate::error::ProxyError::SomeError(format!("Failed to create server config: {}", e)))?;
    Ok(Arc::new(config))
}
