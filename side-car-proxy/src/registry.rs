// talks with etcd
use crate::Result;
use crate::error::ProxyError;

pub trait Registry {
    fn register(&self, service_name: &str, address: &str) -> Result<()>;
    fn deregister(&self, service_name: &str, address: &str) -> Result<()>;
    fn discover(&self, service_name: &str) -> Option<Vec<String>>;
}

pub struct EtcdRegistry {}

impl EtcdRegistry {
    pub fn new() -> Self {
        EtcdRegistry {}
    }
}

impl Registry for EtcdRegistry {
    fn register(&self, service_name: &str, address: &str) -> Result<()> {
        todo!()
    }

    fn deregister(&self, service_name: &str, address: &str) -> Result<()> {
        todo!()
    }

    fn discover(&self, service_name: &str) -> Option<Vec<String>> {
        todo!()
    }
}
