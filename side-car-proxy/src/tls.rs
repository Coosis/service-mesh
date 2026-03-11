use rustls::ServerConfig;
use rustls_pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject};
use std::sync::Arc;

use crate::{Result, error::ProxyError};

pub fn get_server_config(
    cert: impl AsRef<std::path::Path>,
    key: impl AsRef<std::path::Path>,
) -> Result<Arc<ServerConfig>> {
    if !cert.as_ref().exists() {
        return Err(ProxyError::FileNotFound("cert not found".to_string()));
    }
    if !key.as_ref().exists() {
        return Err(ProxyError::FileNotFound("key not found".to_string()));
    }
    let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_file_iter(cert)
        .map_err(|_| ProxyError::CertOpenError)?
        .map(|c| c.map_err(|_| ProxyError::CertMalformedError))
        .filter_map(Result::ok)
        .collect();
    let key = PrivateKeyDer::from_pem_file(key).unwrap();
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| {
            crate::error::ProxyError::SomeError(format!("Failed to create server config: {}", e))
        })?;
    Ok(Arc::new(config))
}
