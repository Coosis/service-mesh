use crate::{
    acl::{ACL, AccessStrategy},
    strategy::LoadBalanceStrategy,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// Control plane parses a list of ProxyConfig objects, one per proxy instance
// when a proxy requests its configuration, it provides its instance ID and
// the control plane returns the corresponding ProxyConfig
// proxy deserialize the config

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub config: HashMap<String, ProxyConfig>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct MeshEgressConfig {
    /// Addr on which the proxy listens for service-to-cluster mTLS egress traffic
    /// should be localhost for security reasons
    pub mesh_egress_bind: String,

    // TLS settings
    // client cert/key used when proxy acts as a client to other proxies inside the mesh
    pub tls_client_crt: String,
    pub tls_client_key: String,
}

impl MeshEgressConfig {
    pub fn new(mesh_egress_bind: String, tls_client_crt: String, tls_client_key: String) -> Self {
        MeshEgressConfig {
            mesh_egress_bind,
            tls_client_crt,
            tls_client_key,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum TlsConfig {
    #[serde(rename = "none")]
    /// don't use tls for listeners
    None,
    /// (cert, key) pair:
    ///  - server cert/key used when proxy acts as a mesh server to other proxy clients
    /// use tls only for client ingress
    Plain(String, String),
    /// use tls for both client ingress and mesh ingress
    /// (cert, key, mesh_ingress_bind) pair:
    ///  - server cert/key used when proxy acts as a mesh server to other proxy clients
    ///  - mesh_ingress_bind: addr on which the proxy listens for cluster-to-cluster traffic(mTLS)
    MTLS(String, String, String),
}

impl TlsConfig {
    pub fn cert_and_key(&self) -> Option<(&String, &String)> {
        match self {
            TlsConfig::None => None,
            TlsConfig::Plain(cert, key) => Some((cert, key)),
            TlsConfig::MTLS(cert, key, _) => Some((cert, key)),
        }
    }

    pub fn mesh_ingress_bind(&self) -> Option<&String> {
        match self {
            TlsConfig::MTLS(_, _, mesh_ingress_bind) => Some(mesh_ingress_bind),
            _ => None,
        }
    }

    pub fn replace_cert(&mut self, cert: String) {
        match self {
            TlsConfig::None => {}
            TlsConfig::Plain(_, key) => {
                *self = TlsConfig::Plain(cert, key.clone());
            }
            TlsConfig::MTLS(_, key, mesh_ingress_bind) => {
                *self = TlsConfig::MTLS(cert, key.clone(), mesh_ingress_bind.clone());
            }
        }
    }

    pub fn replace_key(&mut self, key: String) {
        match self {
            TlsConfig::None => {}
            TlsConfig::Plain(cert, _) => {
                *self = TlsConfig::Plain(cert.clone(), key);
            }
            TlsConfig::MTLS(cert, _, mesh_ingress_bind) => {
                *self = TlsConfig::MTLS(cert.clone(), key, mesh_ingress_bind.clone());
            }
        }
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        TlsConfig::None
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ProxyConfig {
    /// per-route timeouts in milliseconds
    pub route_timeout: HashMap<String, u64>,

    /// A map of service name -> list of addresses
    pub cluster: HashMap<String, Vec<String>>,

    /// Load balancing strategy, defaults to RoundRobin
    pub strategy: LoadBalanceStrategy,

    /// Addr on which the proxy listens for client ingress traffic
    pub ingress_bind: String,

    /// Addr on which the proxy listens for admin traffic
    pub admin_bind: String,

    // mental model:
    // service_behind_proxy_a -> proxy_a's egress_bind -(mTLS)-> proxy_b's mesh_bind
    // -> service_behind_proxy_b
    pub root_ca: Option<String>,

    pub tls: TlsConfig,

    pub egress: Option<MeshEgressConfig>,

    pub acl: ACL,
}

impl ProxyConfig {
    pub fn new() -> Self {
        ProxyConfig {
            route_timeout: HashMap::new(),
            cluster: HashMap::new(),
            strategy: LoadBalanceStrategy::RoundRobin,

            ingress_bind: "0.0.0.0:8333".to_string(),
            admin_bind: "0.0.0.0:15000".to_string(),

            root_ca: None,
            tls: TlsConfig::None,
            egress: None,

            acl: ACL {
                default: AccessStrategy::Allow,
                rules: vec![],
            },
        }
    }

    pub fn with_root_ca(mut self, ca: String) -> Self {
        self.root_ca = Some(ca);
        self
    }

    pub fn with_tls_cfg(mut self, tls: TlsConfig) -> Self {
        self.tls = tls;
        self
    }

    pub fn with_strategy(mut self, strategy: LoadBalanceStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    pub fn with_egress_cfg(mut self, egress: MeshEgressConfig) -> Self {
        self.egress = Some(egress);
        self
    }

    pub fn with_acl(mut self, acl: ACL) -> Self {
        self.acl = acl;
        self
    }
}
