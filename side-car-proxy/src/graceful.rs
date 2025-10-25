use std::net::SocketAddr;
use std::ops::ControlFlow;
use hyper_util::server::graceful::GracefulShutdown;
use tokio::{net::{TcpListener, TcpStream}, task::JoinSet};
use tracing::debug;

use crate::Result;

/// - tasks: A JoinSet to spawn tasks into, drained on shutdown
pub async fn run_graceful(
    listener: TcpListener,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    lo: impl AsyncFn((TcpStream, SocketAddr), &mut JoinSet<()>, &GracefulShutdown) -> ControlFlow<(), ()>,
) -> Result<()> {
    let graceful = GracefulShutdown::new();
    let mut tasks = JoinSet::<()>::new();

    loop {
        tokio::select! {
            biased;
            _ = async {
                if *shutdown_rx.borrow() { return; }
                let _ = shutdown_rx.changed().await;
            } => { break; }
            res = listener.accept() => {
                match lo(res?, &mut tasks, &graceful).await {
                    ControlFlow::Continue(()) => {}
                    ControlFlow::Break(()) => break,
                }
            }
        }
    }
    drop(listener);

    let deadline = std::time::Duration::from_secs(10);
    let tasks_drained = tokio::time::timeout(deadline, async {
        tokio::join!(
            graceful.shutdown(),
            async {
                while let Some(res) = tasks.join_next().await {
                    if let Err(err) = res {
                        debug!("Task failed: {}", err);
                    } else {
                        // debug!("Task completed");
                    }
                }
            }
        );
    }).await.is_ok();

    if tasks_drained {
        debug!("Graceful shutdown complete");
    } else {
        debug!("Timed out");
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
    }

    Ok(())
}

