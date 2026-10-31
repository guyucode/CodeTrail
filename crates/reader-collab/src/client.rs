use crate::*;
pub use url::{form_urlencoded, Url};
pub fn base_url(server: &str) -> Result<Url> {
    endpoint(server)?;
    Ok(Url::parse(server)?)
}
use anyhow::{ensure, Context, Result};
use std::{io::Read, time::Duration};
/// LAN only, no proxies or redirects; credentials never leave the local network.
pub fn endpoint(server: &str) -> Result<reqwest::Url> {
    let mut url = reqwest::Url::parse(server).context("服务地址无效")?;
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none(),
        "服务地址不能携带凭证、查询或片段"
    );
    let ip = url
        .host_str()
        .and_then(|h| h.parse::<std::net::IpAddr>().ok())
        .context("局域网邀请须使用本机 IP 地址")?;
    ensure!(
        crate::server::local_address(ip) && url.scheme() == "http",
        "此版本仅支持局域网 HTTP 地址，不连接公网服务"
    );
    let path = format!("{}/v1/action", url.path().trim_end_matches('/'));
    url.set_path(&path);
    Ok(url)
}
pub fn call(server: &str, token: &str, request: &Request) -> Result<Value> {
    let url = endpoint(server)?;
    let client = reqwest::blocking::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(4))
        .connect_timeout(Duration::from_millis(1500))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let response = client
        .post(url)
        .bearer_auth(token)
        .json(request)
        .send()
        .context("连接共读服务失败，请检查网络与服务地址")?;
    let status = response.status();
    let mut bytes = Vec::new();
    response.take(5 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 5 * 1024 * 1024, "服务响应超出上限");
    let result: Value = serde_json::from_slice(&bytes).context("服务返回无效响应")?;
    ensure!(
        status.is_success() && result["ok"].as_bool() == Some(true),
        "{}",
        result["error"].as_str().unwrap_or("共读请求失败")
    );
    result.get("value").cloned().context("服务响应缺少结果")
}
