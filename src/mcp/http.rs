//! Streamable HTTP の待ち受け（endpoint は `/mcp`）。
//!
//! MCP の仕様が localhost の server に求める DNS rebinding の対策（Host と Origin の確かめ）は、
//! rmcp の設定で有効にする。認証は仕様に無いので足さない。

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use tokio::net::TcpListener;

use super::server::SuperbashServer;

pub const ENDPOINT: &str = "/mcp";

/// Host と Origin の許可（loopback）。`host` が loopback で無ければ、その host も足す。
fn config(host: &str) -> StreamableHttpServerConfig {
    let mut hosts = vec!["localhost".to_string(), "127.0.0.1".into(), "::1".into()];
    let mut origins = vec![
        "http://localhost:*".to_string(),
        "http://127.0.0.1:*".into(),
        "http://[::1]:*".into(),
    ];
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    if !hosts.iter().any(|h| h == bare) {
        hosts.push(bare.to_string());
        let shown = if bare.contains(':') {
            format!("[{bare}]")
        } else {
            bare.to_string()
        };
        origins.push(format!("http://{shown}:*"));
    }
    StreamableHttpServerConfig::default()
        .with_allowed_hosts(hosts)
        .with_allowed_origins(origins)
}

/// 待ち受けを始め、address を 1 行（stdout）出し、SIGINT / SIGTERM まで動く。
pub async fn serve(server: SuperbashServer, host: &str, port: u16) -> Result<()> {
    let listener = TcpListener::bind((host.trim_start_matches('[').trim_end_matches(']'), port))
        .await
        .with_context(|| format!("{host}:{port} で待ち受けられない"))?;
    let addr: SocketAddr = listener.local_addr()?;

    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        config(host),
    );
    let app = axum::Router::new().route_service(ENDPOINT, service);

    println!("superbash: listening on http://{addr}{ENDPOINT}");
    // 信号が来たら、開いたままの接続（SSE など）を待たずに抜ける。抜けると server が落ち、
    // background で走っている command も止まる（exec.rs の GroupKill）。
    tokio::select! {
        r = axum::serve(listener, app) => r.context("MCP server が異常終了"),
        () = shutdown_signal() => Ok(()),
    }
}

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut term = signal(SignalKind::terminate()).expect("SIGTERM を扱えない");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}
