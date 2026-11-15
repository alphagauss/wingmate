use super::{client, connection_panel, probe, target, Desktop, Identity};
use anyhow::{ensure, Result};
use futures_util::{stream, StreamExt};
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr},
    time::Duration,
};
use tauri::{Url, Webview};
use wingmate_launcher::{
    service::{DEV_WEB_PORTS, WEB_PORTS},
    IS_DEV,
};

fn ports() -> impl Iterator<Item = u16> {
    WEB_PORTS
        .iter()
        .copied()
        .chain(DEV_WEB_PORTS.iter().copied().filter(|_| IS_DEV))
}

pub(super) fn alternatives(url: &Url) -> impl Iterator<Item = Url> + '_ {
    let current = url.port_or_known_default();
    ports().filter_map(move |port| {
        if current == Some(port) || !url.host_str()?.parse::<Ipv4Addr>().ok()?.is_private() {
            return None;
        }
        let mut next = url.clone();
        next.set_port(Some(port)).ok()?;
        Some(next)
    })
}

pub(super) async fn scan(
    webview: &Webview,
    desktop: &Desktop,
    saved: Vec<String>,
    scan: u32,
) -> Result<bool> {
    let client = client(true)?;
    let local: HashSet<Ipv4Addr> = if_addrs::get_if_addrs()?
        .into_iter()
        .filter(|interface| interface.is_oper_up())
        .filter_map(|interface| match interface.ip() {
            IpAddr::V4(ip) if ip.is_private() => Some(ip),
            _ => None,
        })
        .collect();
    let subnets: HashSet<[u8; 3]> = local
        .iter()
        .map(|ip| {
            let [a, b, c, _] = ip.octets();
            [a, b, c]
        })
        .collect();
    ensure!(!subnets.is_empty(), "未连接到可搜索的局域网");

    let mut checked = HashSet::new();
    let saved = saved
        .iter()
        .take(20)
        .filter_map(|link| target(link).ok())
        .filter(|url| {
            url.query().is_none()
                && url.scheme() == "http"
                && url
                    .host_str()
                    .and_then(|host| host.parse::<Ipv4Addr>().ok())
                    .is_some_and(|ip| ip.is_private() && !local.contains(&ip))
        })
        .filter(|url| checked.insert(url.clone()))
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for last in 1..=254 {
        for &[a, b, c] in &subnets {
            let ip = Ipv4Addr::new(a, b, c, last);
            if !local.contains(&ip) {
                for port in ports() {
                    let url = Url::parse(&format!("http://{ip}:{port}/"))?;
                    if checked.insert(url.clone()) {
                        candidates.push(url);
                    }
                }
            }
        }
    }

    let client = &client;
    let search = |urls: Vec<Url>| {
        stream::iter(urls)
            .map(|url| async move {
                let identity = probe(client, &url, Duration::from_millis(600)).await?;
                (identity.transport == "lan").then_some((url, identity))
            })
            .buffer_unordered(64)
    };

    let publish = |url: Url, identity: Identity| -> Result<()> {
        connection_panel::local_panel(webview, desktop).map_err(anyhow::Error::msg)?;
        let found = serde_json::json!({"origin": url.origin().ascii_serialization(), "name": identity.name});
        webview.eval(format!("window.wingmateLanHost?.({scan}, {found})"))?;
        Ok(())
    };
    let mut probes = search(saved)
        .chain(search(candidates))
        .filter_map(std::future::ready);
    let deadline = tokio::time::sleep(Duration::from_secs(16));
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return Ok(true),
            next = probes.next() => match next {
                Some((url, identity)) => publish(url, identity)?,
                None => return Ok(false),
            },
        }
    }
}
