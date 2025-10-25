use serde::{Deserialize, Serialize};
pub mod error;
pub mod config;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub enum LoadBalanceStrategy {
    #[default]
    P2C,
    RoundRobin,
}
