//! 将桌面 WebView 的专用回环入口接到已验证的 Wingmate 网关。
use anyhow::{ensure, Context, Result};
use http_body_util::{combinators::UnsyncBoxBody, BodyExt, Full};
use hyper::{
    body::{Bytes, Incoming},
    header::{self, HeaderMap, HeaderValue},
    service::service_fn,
    Method, Request, Response, StatusCode,
};
use hyper_util::rt::{TokioIo, TokioTimer};
use std::{convert::Infallible, future::Future, sync::Arc, time::Duration};
use tauri::Url;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpListener,
    sync::Semaphore,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

pub(super) trait Stream: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Stream for T {}

pub(super) type BoxStream = Box<dyn Stream>;
type Body = UnsyncBoxBody<Bytes, hyper::Error>;

pub(super) struct Relay {
    cancel: CancellationToken,
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Clone)]
struct Route {
    local: String,
    upstream: String,
    cookie: String,
    local_cookie: String,
    cancel: CancellationToken,
}

impl Relay {
    pub(super) async fn listen<F, S>(pairing: Url, dial: F) -> Result<(Self, Url)>
    where
        F: Fn() -> S + Send + Sync + 'static,
        S: Future<Output = Result<BoxStream>> + Send,
    {
        let port = pairing.port().context("缺少网关端口")?;
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let local = listener.local_addr()?.to_string();
        let mut url = Url::parse(&format!("http://{local}/"))?;
        url.set_query(pairing.query());
        let cancel = CancellationToken::new();
        let route = Route {
            local,
            upstream: format!("127.0.0.1:{port}"),
            cookie: format!("wingmate_share_{port}"),
            local_cookie: format!("wingmate_relay_{}", Uuid::new_v4().simple()),
            cancel: cancel.clone(),
        };
        let dial = Arc::new(dial);
        tokio::spawn(async move {
            let slots = Arc::new(Semaphore::new(64));
            while let Some(Ok((socket, _))) =
                route.cancel.run_until_cancelled(listener.accept()).await
            {
                let Ok(slot) = slots.clone().try_acquire_owned() else {
                    continue;
                };
                let route = route.clone();
                let dial = dial.clone();
                tokio::spawn(async move {
                    let _slot = slot;
                    let cancel = route.cancel.clone();
                    let handler = service_fn(move |request| {
                        let route = route.clone();
                        let dial = dial.clone();
                        async move {
                            let reply = proxy(request, &route, &*dial).await.unwrap_or_else(|_| {
                                reply(
                                    StatusCode::BAD_GATEWAY,
                                    "与 Wingmate 的连接已中断，请重新连接",
                                )
                            });
                            Ok::<_, Infallible>(reply)
                        }
                    });
                    let mut http = hyper::server::conn::http1::Builder::new();
                    http.timer(TokioTimer::new())
                        .header_read_timeout(Duration::from_secs(10));
                    let connection = http
                        .serve_connection(TokioIo::new(socket), handler)
                        .with_upgrades();
                    let _ = cancel.run_until_cancelled(connection).await;
                });
            }
        });
        Ok((Self { cancel }, url))
    }
}

fn reply(status: StatusCode, message: &'static str) -> Response<Body> {
    Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
        .body(
            Full::new(Bytes::from_static(message.as_bytes()))
                .map_err(|never| match never {})
                .boxed_unsync(),
        )
        .expect("static response")
}

fn request_allowed(request: &Request<impl Sized>, route: &Route) -> bool {
    let headers = request.headers();
    let local_origin = format!("http://{}", route.local);
    headers
        .get(header::HOST)
        .is_some_and(|host| host == route.local.as_str())
        && headers
            .get(header::ORIGIN)
            .is_none_or(|origin| origin == local_origin.as_str())
        && headers
            .get("sec-fetch-site")
            .is_none_or(|site| site == "same-origin" || site == "none")
        && request.uri().authority().is_none()
        && !matches!(*request.method(), Method::CONNECT | Method::TRACE)
}

fn connection_headers(headers: &mut HeaderMap, upgrade: bool) {
    let names = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|name| header::HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect::<Vec<_>>();
    for name in names {
        headers.remove(name);
    }
    for name in [
        header::CONNECTION,
        header::UPGRADE,
        header::TE,
        header::TRAILER,
        header::TRANSFER_ENCODING,
        header::PROXY_AUTHENTICATE,
        header::PROXY_AUTHORIZATION,
    ] {
        headers.remove(name);
    }
    for name in ["keep-alive", "proxy-connection"] {
        headers.remove(name);
    }
    if upgrade {
        headers.insert(header::CONNECTION, HeaderValue::from_static("upgrade"));
        headers.insert(header::UPGRADE, HeaderValue::from_static("websocket"));
    }
}

fn rewrite_request(headers: &mut HeaderMap, route: &Route, upgrade: bool) -> Result<()> {
    let has_origin = headers.contains_key(header::ORIGIN);
    let cookie = headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|value| value.trim().split_once('='))
        .find(|(name, _)| *name == route.local_cookie)
        .and_then(|(_, value)| Uuid::parse_str(value).ok());
    connection_headers(headers, upgrade);
    for name in [header::COOKIE, header::AUTHORIZATION, header::REFERER] {
        headers.remove(name);
    }
    headers.insert(header::HOST, route.upstream.parse()?);
    if has_origin {
        headers.insert(
            header::ORIGIN,
            format!("http://{}", route.upstream).parse()?,
        );
    }
    if let Some(cookie) = cookie {
        headers.insert(
            header::COOKIE,
            format!("{}={cookie}", route.cookie).parse()?,
        );
    }
    Ok(())
}

fn rewrite_response(headers: &mut HeaderMap, route: &Route, upgrade: bool) -> Result<()> {
    connection_headers(headers, upgrade);
    let cookies = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| {
            let cookie = cookie::Cookie::parse(value.to_str().ok()?).ok()?;
            (cookie.name() == route.cookie).then(|| {
                let mut local =
                    cookie::Cookie::build((route.local_cookie.clone(), cookie.value().to_owned()))
                        .path("/")
                        .http_only(true)
                        .same_site(cookie::SameSite::Strict);
                if let Some(age) = cookie.max_age() {
                    local = local.max_age(age);
                }
                local.build().to_string()
            })
        })
        .collect::<Vec<_>>();
    headers.remove(header::SET_COOKIE);
    headers.remove("clear-site-data");
    for cookie in cookies {
        headers.append(header::SET_COOKIE, cookie.parse()?);
    }
    if let Some(url) = headers
        .get(header::LOCATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| Url::parse(value).ok())
    {
        if url.origin().ascii_serialization() == format!("http://{}", route.upstream) {
            let mut url = url;
            let local: std::net::SocketAddr = route.local.parse()?;
            url.set_port(Some(local.port()))
                .map_err(|()| anyhow::anyhow!("无效的转发地址"))?;
            headers.insert(header::LOCATION, url.as_str().parse()?);
        }
    }
    Ok(())
}

async fn proxy<F, S>(
    mut request: Request<Incoming>,
    route: &Route,
    dial: &F,
) -> Result<Response<Body>>
where
    F: Fn() -> S,
    S: Future<Output = Result<BoxStream>>,
{
    if !request_allowed(&request, route) {
        return Ok(reply(StatusCode::FORBIDDEN, "不接受此来源的请求"));
    }
    let websocket = request
        .headers()
        .get(header::UPGRADE)
        .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"websocket"));
    let downstream = websocket.then(|| hyper::upgrade::on(&mut request));
    rewrite_request(request.headers_mut(), route, websocket)?;
    let stream = tokio::time::timeout(Duration::from_secs(8), dial()).await??;
    let (mut sender, connection) =
        hyper::client::conn::http1::handshake(TokioIo::new(stream)).await?;
    let cancel = route.cancel.clone();
    tokio::spawn(async move {
        tokio::select! {
            _ = cancel.cancelled_owned() => {}
            _ = connection.with_upgrades() => {}
        }
    });
    let mut response = sender.send_request(request).await?;
    let switched = response.status() == StatusCode::SWITCHING_PROTOCOLS;
    if switched {
        let downstream = downstream.context("网关返回了意外的协议切换")?;
        ensure!(
            response
                .headers()
                .get(header::UPGRADE)
                .is_some_and(|value| value.as_bytes().eq_ignore_ascii_case(b"websocket")),
            "网关未返回 WebSocket"
        );
        let upstream = hyper::upgrade::on(&mut response);
        let cancel = route.cancel.clone();
        tokio::spawn(async move {
            let _ = cancel
                .run_until_cancelled(async move {
                    let (downstream, upstream) = tokio::try_join!(downstream, upstream)?;
                    tokio::io::copy_bidirectional(
                        &mut TokioIo::new(downstream),
                        &mut TokioIo::new(upstream),
                    )
                    .await?;
                    Ok::<(), anyhow::Error>(())
                })
                .await;
        });
    }
    rewrite_response(response.headers_mut(), route, switched)?;
    Ok(response.map(BodyExt::boxed_unsync))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route() -> Route {
        Route {
            local: "127.0.0.1:6543".into(),
            upstream: "127.0.0.1:8746".into(),
            cookie: "wingmate_share_8746".into(),
            local_cookie: "wingmate_relay_test".into(),
            cancel: CancellationToken::new(),
        }
    }

    #[test]
    fn rejects_a_sibling_loopback_origin() {
        let route = route();
        for (key, value) in [
            ("sec-fetch-site", "same-site"),
            ("origin", "http://127.0.0.1:8746"),
        ] {
            let request = Request::builder()
                .header(header::HOST, &route.local)
                .header(key, value)
                .body(())
                .unwrap();
            assert!(!request_allowed(&request, &route));
        }
    }

    #[test]
    fn isolates_gateway_cookies_and_remaps_same_gateway_redirects() {
        let route = route();
        let token = Uuid::new_v4();
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            format!("wingmate_share_8746=local; {}={token}", route.local_cookie)
                .parse()
                .unwrap(),
        );
        headers.insert(header::ORIGIN, "http://127.0.0.1:6543".parse().unwrap());
        rewrite_request(&mut headers, &route, false).unwrap();
        assert_eq!(
            headers[header::COOKIE],
            format!("wingmate_share_8746={token}")
        );
        assert_eq!(headers[header::HOST], route.upstream);
        assert_eq!(headers[header::ORIGIN], "http://127.0.0.1:8746");
        headers.append(header::SET_COOKIE, "unrelated=value".parse().unwrap());
        headers.append(
            header::SET_COOKIE,
            format!(
                "{}={token}; Max-Age=0; Domain=untrusted.example",
                route.cookie
            )
            .parse()
            .unwrap(),
        );
        headers.insert(
            header::LOCATION,
            "http://127.0.0.1:8746/path?q=1".parse().unwrap(),
        );
        rewrite_response(&mut headers, &route, false).unwrap();
        assert_eq!(headers.get_all(header::SET_COOKIE).iter().count(), 1);
        let cookie = cookie::Cookie::parse(headers[header::SET_COOKIE].to_str().unwrap()).unwrap();
        assert_eq!(cookie.name(), route.local_cookie);
        assert_eq!(cookie.max_age(), Some(cookie::time::Duration::ZERO));
        assert_eq!(cookie.domain(), None);
        assert_eq!(headers[header::LOCATION], "http://127.0.0.1:6543/path?q=1");
    }
}
