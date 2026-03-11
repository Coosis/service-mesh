use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub enum AccessStrategy {
    #[default]
    #[serde(rename = "allow")]
    Allow,
    #[serde(rename = "deny")]
    Deny,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, PartialEq, Eq)]
pub enum Listener {
    #[default]
    #[serde(rename = "ingress")]
    Ingress,
    #[serde(rename = "mesh")]
    Mesh,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct AccessRule {
    pub listener: Listener,
    pub path: String,
    pub strategy: AccessStrategy,
}

impl AccessRule {
    pub fn new<E: Into<String>>(listener: Listener, path: E, strategy: AccessStrategy) -> Self {
        AccessRule {
            listener,
            path: path.into(),
            strategy,
        }
    }
}

impl AccessRule {}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct ACL {
    pub default: AccessStrategy,
    pub rules: Vec<AccessRule>,
}
