//! Temporary LAN host. No separate server installation or public endpoint.
use crate::{service::Service, Request};
use anyhow::Result;
use axum::{
    extract::{ConnectInfo, DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener},
    path::Path,
    sync::{mpsc, Arc},
    thread,
};

pub fn local_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => v.is_private() || v.is_loopback() || v.is_link_local(),
        IpAddr::V6(v) => {
            v.is_loopback() || v.to_ipv4_mapped().is_some_and(|v| local_address(v.into()))
        }
    }
}
async fn action(
    State(service): State<Arc<Service>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(request): Json<Request>,
) -> (StatusCode, Json<Value>) {
    if !local_address(peer.ip()) {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"ok":false,"error":"此版本仅接受局域网连接"})),
        );
    }
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("")
        .to_owned();
    match tokio::task::spawn_blocking(move || service.handle(&token, request)).await {
        Ok(Ok(value)) => (StatusCode::OK, Json(json!({"ok":true,"value":value}))),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok":false,"error":e.to_string()})),
        ),
        Err(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"ok":false,"error":"服务暂不可用"})),
        ),
    }
}
pub fn router(service: Arc<Service>) -> Router {
    Router::new()
        .route(
            "/health",
            get(|| async { Json(json!({"ok":true,"protocol":1,"network":"lan"})) }),
        )
        .route("/v1/action", post(action))
        .layer(DefaultBodyLimit::max(128 * 1024))
        .with_state(service)
}
/// Read interface addresses directly: proxy/VPN default routes must not turn
/// a usable physical LAN into a loopback-only invitation.
pub fn local_addresses() -> Vec<String> {
    let mut addresses: Vec<_> = if_addrs::get_if_addrs()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|interface| {
            let IpAddr::V4(ip) = interface.ip() else {
                return None;
            };
            if ip.is_loopback() || !(ip.is_private() || ip.is_link_local()) {
                return None;
            }
            let name = interface.name.to_ascii_lowercase();
            let tunnel = name.contains("tun") || name.contains("vpn") || name.contains("tailscale");
            Some((tunnel, ip.is_link_local(), interface.name, ip.to_string()))
        })
        .collect();
    addresses.sort();
    let mut result = Vec::new();
    for (_, _, _, ip) in addresses {
        if !result.contains(&ip) {
            result.push(ip);
        }
    }
    result
}

pub struct Host {
    pub server: String,
    pub key: String,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Host {
    pub fn start(directory: &Path, advertise: Option<Ipv4Addr>) -> Result<Self> {
        let key = crate::secret();
        let service = Arc::new(Service::open(directory, &key)?);
        let listener = TcpListener::bind((Ipv4Addr::UNSPECIFIED, 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let ip = advertise
            .filter(|ip| local_address((*ip).into()))
            .or_else(|| local_addresses().first().and_then(|ip| ip.parse().ok()))
            .unwrap_or(Ipv4Addr::LOCALHOST);
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let (started, ready) = mpsc::sync_channel(1);
        let thread = thread::spawn(move || {
            let result = (|| -> Result<()> {
                let rt = tokio::runtime::Runtime::new()?;
                rt.block_on(async {
                    let listener = tokio::net::TcpListener::from_std(listener)?;
                    let _ = started.send(true);
                    axum::serve(
                        listener,
                        router(service).into_make_service_with_connect_info::<SocketAddr>(),
                    )
                    .with_graceful_shutdown(async {
                        let _ = stopped.await;
                    })
                    .await?;
                    Ok(())
                })
            })();
            if result.is_err() {
                let _ = started.send(false);
            }
        });
        anyhow::ensure!(
            ready
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap_or(false),
            "无法启动局域网共读"
        );
        Ok(Self {
            server: format!("http://{ip}:{port}"),
            key,
            stop: Some(stop),
            thread: Some(thread),
        })
    }
}
impl Drop for Host {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
