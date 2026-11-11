use super::{response, Body, Session};
use crate::engine::Snapshot;
use anyhow::{Context, Result};
use http_body_util::{BodyExt, StreamBody};
use hyper::{body::Incoming, header, HeaderMap, Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_util::sync::CancellationToken;

pub(super) struct Credentials {
    client: reqwest::Client,
    cached: tokio::sync::Mutex<Option<(String, header::HeaderValue)>>,
}

impl Credentials {
    pub(super) fn new() -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(5))
                .build()?,
            cached: tokio::sync::Mutex::new(None),
        })
    }

    async fn cookie(&self, instance: &str, login: &reqwest::Url) -> Result<header::HeaderValue> {
        let mut cached = self.cached.lock().await;
        if let Some((_, cookie)) = cached.as_ref().filter(|(current, _)| current == instance) {
            return Ok(cookie.clone());
        }
        let response = self.client.get(login.clone()).send().await?;
        anyhow::ensure!(response.status() == StatusCode::SEE_OTHER, "DSH 登录未完成");
        let cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .filter_map(|value| value.split(';').next())
            .collect::<Vec<_>>()
            .join("; ");
        anyhow::ensure!(!cookie.is_empty(), "DSH 登录未提供会话");
        let cookie: header::HeaderValue = cookie.parse()?;
        *cached = Some((instance.to_owned(), cookie.clone()));
        Ok(cookie)
    }

    pub(super) async fn forward(
        &self,
        request: Request<Incoming>,
        state: Snapshot,
        session: Session,
        closed: CancellationToken,
    ) -> Response<Body> {
        match self.proxy(request, &state, session, closed).await {
            Ok(reply) => {
                if reply.status() == StatusCode::UNAUTHORIZED {
                    let mut cached = self.cached.lock().await;
                    if cached
                        .as_ref()
                        .is_some_and(|(instance, _)| *instance == state.instance)
                    {
                        *cached = None;
                    }
                }
                reply
            }
            Err(_) => response(StatusCode::BAD_GATEWAY, "无法连接业务后台，请重试"),
        }
    }

    async fn proxy(
        &self,
        mut request: Request<Incoming>,
        state: &Snapshot,
        session: Session,
        closed: CancellationToken,
    ) -> Result<Response<Body>> {
        let login = reqwest::Url::parse(state.url.as_deref().context("缺少 DSH 地址")?)?;
        let port = login.port().context("缺少 DSH 端口")?;
        let authority = format!("127.0.0.1:{port}");
        let upstream_origin = format!("http://{authority}");
        let request_uri = request.uri().clone();
        let upgrade = request.headers().get(header::UPGRADE);
        let websocket =
            upgrade.is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"websocket"));
        if upgrade.is_some() && (!websocket || request.method() != Method::GET) {
            return Ok(response(StatusCode::BAD_REQUEST, "不支持此连接升级"));
        }
        if request.uri().path() == "/"
            && request.uri().query().is_some_and(|query| {
                reqwest::Url::parse(&format!("http://127.0.0.1/?{query}"))
                    .ok()
                    .is_some_and(|url| url.query_pairs().any(|(key, _)| key == "token"))
            })
        {
            return Ok(response(StatusCode::BAD_REQUEST, "请使用配对链接"));
        }
        let cookie = self.cookie(&state.instance, &login).await?;
        let socket = tokio::time::timeout(
            Duration::from_secs(3),
            TcpStream::connect((std::net::Ipv4Addr::LOCALHOST, port)),
        )
        .await??;
        let (mut sender, connection) =
            hyper::client::conn::http1::handshake(TokioIo::new(socket)).await?;
        tokio::spawn(async move {
            let _ = connection.with_upgrades().await;
        });
        let client_upgrade = websocket.then(|| hyper::upgrade::on(&mut request));
        let headers = request.headers_mut();
        let has_origin = headers.contains_key(header::ORIGIN);
        strip_hop_headers(headers, websocket);
        headers.remove(header::AUTHORIZATION);
        headers.remove("forwarded");
        headers.remove("x-forwarded-for");
        headers.remove("x-forwarded-host");
        headers.remove("x-forwarded-proto");
        headers.insert(header::HOST, authority.parse()?);
        headers.insert(header::COOKIE, cookie);
        if has_origin {
            headers.insert(header::ORIGIN, upstream_origin.parse()?);
        }
        let mut reply = tokio::select! {
            biased;
            _ = session.ended() => return Ok(response(StatusCode::UNAUTHORIZED, "连接已撤销或过期")),
            result = async { sender.send_request(request).await } => result?,
        };
        let upgraded = reply.status() == StatusCode::SWITCHING_PROTOCOLS;
        if upgraded {
            let client = client_upgrade.context("上游意外升级连接")?;
            let server = hyper::upgrade::on(&mut reply);
            let session = session.clone();
            tokio::spawn(async move {
                let tunnel = async {
                    let (client, server) = tokio::try_join!(client, server)?;
                    let _ = tokio::io::copy_bidirectional(
                        &mut TokioIo::new(client),
                        &mut TokioIo::new(server),
                    )
                    .await;
                    Ok::<_, hyper::Error>(())
                };
                tokio::select! {
                    biased;
                    _ = session.ended() => {},
                    _ = closed.cancelled() => {},
                    _ = tunnel => {},
                }
            });
        }
        let (mut parts, body) = reply.into_parts();
        strip_hop_headers(&mut parts.headers, upgraded);
        parts.headers.remove(header::SET_COOKIE);
        parts.headers.insert(
            header::CACHE_CONTROL,
            header::HeaderValue::from_static("no-store"),
        );
        if let Some(location) = parts
            .headers
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok())
        {
            let upstream_request = reqwest::Url::parse(&format!("{upstream_origin}{request_uri}"))?;
            if let Ok(url) = upstream_request.join(location) {
                if url.origin() == login.origin() {
                    let origin = login.origin().ascii_serialization();
                    let path = url
                        .as_str()
                        .strip_prefix(&origin)
                        .context("无效的业务跳转地址")?;
                    // Same-upstream redirects must never expose the upstream login credential.
                    if path.starts_with("//") || url.query_pairs().any(|(key, _)| key == "token") {
                        return Ok(response(StatusCode::BAD_GATEWAY, "业务后台返回无效跳转"));
                    }
                    parts.headers.insert(header::LOCATION, path.parse()?);
                }
            }
        }
        let frames =
            futures_util::stream::unfold((body, session), |(mut body, session)| async move {
                tokio::select! {
                    biased;
                    _ = session.ended() => None,
                    frame = body.frame() => frame.map(|frame| (frame, (body, session))),
                }
            });
        Ok(Response::from_parts(
            parts,
            StreamBody::new(frames).boxed_unsync(),
        ))
    }
}

fn strip_hop_headers(headers: &mut HeaderMap, websocket: bool) {
    let named: Vec<_> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| header::HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect();
    for name in named {
        if !websocket || name != header::UPGRADE {
            headers.remove(name);
        }
    }
    for name in [
        "keep-alive",
        "proxy-authenticate",
        "proxy-authorization",
        "proxy-connection",
        "te",
        "trailer",
        "transfer-encoding",
    ] {
        headers.remove(name);
    }
    if websocket {
        headers.insert(
            header::CONNECTION,
            header::HeaderValue::from_static("upgrade"),
        );
    } else {
        headers.remove(header::CONNECTION);
        headers.remove(header::UPGRADE);
    }
}
