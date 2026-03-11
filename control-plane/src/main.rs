use mesh_core::config::Config;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;

use axum::{
    Router,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
};

use crate::config::{from_content, to_json};

mod config;
mod error;
type Result<T> = std::result::Result<T, error::Error>;

type SharedState = Arc<RwLock<AppState>>;
struct AppState {
    config_mp: Option<Config>,
    config_tx: HashMap<String, tokio::sync::watch::Sender<Bytes>>,
}

#[tokio::main]
async fn main() {
    // read config
    let config_path =
        std::env::var("CONFIG_PATH").unwrap_or("example/proxy_config.toml".to_string());
    let config_file = config::from_file(&config_path).await.unwrap();

    // uncomment to check single service config toml format
    // use mesh_core::{acl::AccessRule, config::Config};
    // use mesh_core::acl::{AccessStrategy, Listener};
    // let acl = mesh_core::acl::ACL {
    //     default: AccessStrategy::Deny,
    //     rules: vec![
    //         AccessRule::new(Listener::Mesh, "/allowedpath", AccessStrategy::Allow),
    //         AccessRule::new(Listener::Mesh, "somepath", AccessStrategy::Deny),
    //         AccessRule::new(Listener::Ingress, "anotherpath", AccessStrategy::Deny),
    //     ],
    // };
    // let mut pconfig = mesh_core::config::ProxyConfig::default()
    //     .with_acl(acl)
    //     // .with_tls_cfg(mesh_core::config::TlsConfig::MTLS("test.cert".into(), "test.key".into(), "addr:8008".into()))
    //     // .with_tls_cfg(mesh_core::config::TlsConfig::Plain("test.cert".into(), "test.key".into()))
    //     .with_tls_cfg(mesh_core::config::TlsConfig::None)
    //     .with_strategy(mesh_core::strategy::LoadBalanceStrategy::RoundRobin);
    //     // .with_egress_cfg(mesh_core::config::MeshEgressConfig::new(
    //     //         "test2.bind".into(),
    //     //         "test2.cert".into(),
    //     //         "test2.key".into()));
    // let config_toml_str = toml::to_string_pretty(&pconfig).unwrap_or("".to_string());
    // println!("pconfig: {}", config_toml_str);
    // return;

    // uncomment to check multi service config toml format
    // let mut config = mesh_core::config::Config::default();
    // config.config.insert("service-a".to_string(), pconfig);
    // let config_toml_str = toml::to_string_pretty(&config_file).unwrap_or("".to_string());
    // println!("Loaded config:\n{}", config_toml_str);

    let state = Arc::new(RwLock::new(AppState {
        config_mp: Some(config_file),
        config_tx: HashMap::new(),
    }));
    let app = Router::new()
        .route("/poll_config/{svc_name}", get(poll_config))
        .route("/upload_config", post(upload_config))
        .route("/config/{svc_name}", get(config))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:13000")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn poll_config(
    Path(svc_name): Path<String>,
    State(s): State<SharedState>,
) -> (StatusCode, Result<Bytes>) {
    let mut rx = {
        let need_write = {
            let s = s.read().await;
            s.config_tx.contains_key(&svc_name)
        };
        if need_write {
            let mut s = s.write().await;
            if !s.config_tx.contains_key(&svc_name) {
                let (tx, rx) = tokio::sync::watch::channel(Bytes::new());
                s.config_tx.insert(svc_name.clone(), tx);
                rx
            } else {
                let tx = s.config_tx.get(&svc_name).unwrap();
                tx.subscribe()
            }
        } else {
            let mut s = s.write().await;
            let (tx, rx) = tokio::sync::watch::channel(Bytes::new());
            s.config_tx.insert(svc_name.clone(), tx);
            rx
        }
    };
    if let Ok(Ok(_)) =
        tokio::time::timeout(std::time::Duration::from_millis(20000), rx.changed()).await
    {
        return (StatusCode::OK, Ok(rx.borrow().clone()));
    }
    return (StatusCode::REQUEST_TIMEOUT, Err(error::Error::Timeout));
}

async fn config(Path(svc_name): Path<String>, State(s): State<SharedState>) -> Result<Bytes> {
    let s = s.read().await;
    if let Some(mp) = &s.config_mp {
        let cfg = mp
            .config
            .get(&svc_name)
            .ok_or_else(|| error::Error::NoConfig)?;
        let b = to_json(cfg)?.into();
        Ok(b)
    } else {
        Err(error::Error::NoConfig)
    }
}

async fn upload_config(State(s): State<SharedState>, body: Bytes) -> Result<()> {
    {
        let mut s = s.write().await;
        // todo: parse and validate
        let content =
            std::str::from_utf8(&body).map_err(|e| error::Error::MalformedConfig(e.to_string()))?;
        let cfg = from_content(content)?;
        s.config_mp = Some(cfg);
    }

    let s = s.read().await;
    if let Some(cfg_mp) = &s.config_mp {
        for (name, cfg) in &cfg_mp.config {
            let b: Bytes = to_json(cfg)?.into();
            if let Some(tx) = &s.config_tx.get(name) {
                match tx.send(b.clone()) {
                    Ok(_) => (),
                    Err(e) => {
                        println!("error broadcasting config to {}: {}", name, e);
                    }
                }
            }
        }
    }
    return Ok(());
}
