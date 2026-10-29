//! HTTP proxy front end for engines without RFC1929 SOCKS authentication (Chromium/WebView2).
//! The same document token is required through Basic proxy authentication. Missing or wrong
//! credentials receive only a 407 challenge; no request body is read and nothing is forwarded.
//! A CONNECT is answered only after the caller has checked its authority and prepared the
//! response-aware HTTP/TLS handling. An absolute-form request is returned without its proxy
//! authorization headers for the caller's bounded HTTP parser.
use super::{EndpointAuthority, EndpointToken, hostname, send};
use crate::remote::html_http::{Error, Result};
use base64::Engine;
use std::{net::IpAddr, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Bound for the proxy request head, including the request line and all headers.
pub const HEAD_BYTES: usize = 16 * 1024;
const USER: &str = "tasty";
/// Unread request bytes discarded after a refusal, and the time allowed for that.
const LINGER_BYTES: usize = 64 * 1024;
const LINGER: Duration = Duration::from_secs(1);

/// Unguessable per-document realm. A remote site cannot produce this challenge, so a native
/// handler can tell this endpoint's 407 apart from a 401 of a remote page on the same address.
pub struct ProxyRealm(String);
impl ProxyRealm {
    pub fn from_random_bytes(entropy: [u8; 16]) -> Self {
        let mut realm = String::from("tasty-");
        for b in entropy {
            realm.push_str(&format!("{b:02x}"));
        }
        Self(realm)
    }
    /// The exact Proxy-Authenticate value this endpoint sends.
    pub fn challenge(&self) -> String {
        format!("Basic realm=\"{}\"", self.0)
    }
}
impl std::fmt::Debug for ProxyRealm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProxyRealm([redacted])")
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ProxyRoute {
    /// CONNECT authority; the caller answers with accept_tunnel or reject_proxy_route.
    Tunnel(EndpointAuthority),
    /// Absolute-form plain HTTP request head without proxy authorization headers.
    Forward {
        authority: EndpointAuthority,
        head: Vec<u8>,
    },
}

/// Credentials the native engine presents for this endpoint's challenge.
pub fn credentials(token: &EndpointToken) -> (&'static str, String) {
    (USER, String::from_utf8_lossy(&token.0).into_owned())
}

pub async fn authenticate_http<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut S,
    token: &EndpointToken,
    realm: &ProxyRealm,
    deadline: Duration,
) -> Result<ProxyRoute> {
    if deadline.is_zero() {
        return Err(Error::Invalid("endpoint deadline"));
    }
    tokio::time::timeout(deadline, negotiate(io, token, realm))
        .await
        .map_err(|_| Error::Timeout)?
}

async fn negotiate<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut S,
    token: &EndpointToken,
    realm: &ProxyRealm,
) -> Result<ProxyRoute> {
    let head = read_head(io).await?;
    let text = std::str::from_utf8(&head).map_err(|_| Error::Invalid("proxy request head"))?;
    let mut lines = text.split("\r\n");
    let request = lines.next().ok_or(Error::Invalid("proxy request line"))?;
    let mut parts = request.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return refuse(io, Error::Invalid("proxy request line")).await;
    };
    if !matches!(version, "HTTP/1.1" | "HTTP/1.0") || method.is_empty() {
        return refuse(io, Error::Invalid("proxy request line")).await;
    }
    let mut authorization = None;
    for line in lines.clone().take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or(Error::Invalid("proxy header"))?;
        if name.eq_ignore_ascii_case("proxy-authorization")
            && authorization.replace(value.trim()).is_some()
        {
            return refuse(io, Error::Invalid("proxy authorization")).await;
        }
    }
    if !authorization.is_some_and(|value| authorized(value, token)) {
        let challenge = format!(
            "HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            realm.challenge()
        );
        send(io, challenge.as_bytes()).await?;
        linger(io).await;
        return Err(Error::Invalid("endpoint authentication required"));
    }
    if method == "CONNECT" {
        return match connect_authority(target) {
            Ok(authority) => Ok(ProxyRoute::Tunnel(authority)),
            Err(error) => refuse(io, error).await,
        };
    }
    let authority = match forward_authority(target) {
        Ok(authority) => authority,
        Err(error) => return refuse(io, error).await,
    };
    let mut forwarded = Vec::with_capacity(head.len());
    for line in text.split_inclusive("\r\n") {
        let name = line.split_once(':').map(|(name, _)| name);
        if name.is_some_and(|name| {
            name.eq_ignore_ascii_case("proxy-authorization")
                || name.eq_ignore_ascii_case("proxy-connection")
        }) {
            continue;
        }
        forwarded.extend_from_slice(line.as_bytes());
    }
    Ok(ProxyRoute::Forward {
        authority,
        head: forwarded,
    })
}

async fn read_head<S: AsyncRead + Unpin>(io: &mut S) -> Result<Vec<u8>> {
    // Byte reads leave any request body in the stream for the caller's HTTP parser.
    let mut head = Vec::with_capacity(1024);
    while !head.ends_with(b"\r\n\r\n") {
        if head.len() >= HEAD_BYTES {
            return Err(Error::Quota("proxy request head"));
        }
        head.push(io.read_u8().await.map_err(|_| Error::Network)?);
    }
    Ok(head)
}

fn authorized(value: &str, token: &EndpointToken) -> bool {
    let Some(encoded) = value
        .split_once(' ')
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("basic"))
        .map(|(_, encoded)| encoded.trim())
    else {
        return false;
    };
    let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) else {
        return false;
    };
    decoded
        .strip_prefix(USER.as_bytes())
        .and_then(|rest| rest.strip_prefix(b":"))
        .is_some_and(|password| token.matches(password))
}

fn connect_authority(target: &str) -> Result<EndpointAuthority> {
    let (host, port) = target
        .rsplit_once(':')
        .ok_or(Error::Invalid("proxy tunnel authority"))?;
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or(Error::Invalid("endpoint port"))?;
    let host = if let Some(ip) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        ip.parse::<std::net::Ipv6Addr>()
            .map_err(|_| Error::Invalid("endpoint hostname"))?
            .to_string()
    } else if let Ok(ip) = host.parse::<IpAddr>() {
        if ip.is_ipv6() {
            return Err(Error::Invalid("endpoint hostname"));
        }
        ip.to_string()
    } else {
        hostname(host.as_bytes())?
    };
    Ok(EndpointAuthority { host, port })
}

fn forward_authority(target: &str) -> Result<EndpointAuthority> {
    let url = reqwest::Url::parse(target).map_err(|_| Error::Invalid("proxy request target"))?;
    if url.scheme() != "http" || !url.username().is_empty() || url.password().is_some() {
        return Err(Error::UnsupportedScheme);
    }
    let host = match url.host().ok_or(Error::Invalid("endpoint hostname"))? {
        url::Host::Domain(name) => name.to_owned(),
        url::Host::Ipv4(ip) => ip.to_string(),
        url::Host::Ipv6(ip) => ip.to_string(),
    };
    let port = url
        .port_or_known_default()
        .ok_or(Error::Invalid("endpoint port"))?;
    Ok(EndpointAuthority { host, port })
}

async fn refuse<S: AsyncRead + AsyncWrite + Unpin, T>(io: &mut S, error: Error) -> Result<T> {
    reject_proxy_route(io).await?;
    Err(error)
}

/// Only after preparing response-aware HTTP/TLS handling; never a blind tunnel.
/// The only caller is the native WebView listener; test-only builds of this module have none.
#[cfg(all(
    feature = "gui",
    any(target_os = "linux", target_os = "macos", windows)
))]
pub async fn accept_tunnel<S: AsyncWrite + Unpin>(io: &mut S) -> Result<()> {
    send(io, b"HTTP/1.1 200 Connection Established\r\n\r\n").await
}
pub async fn reject_proxy_route<S: AsyncRead + AsyncWrite + Unpin>(io: &mut S) -> Result<()> {
    send(
        io,
        b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
    )
    .await?;
    linger(io).await;
    Ok(())
}

/// Closing a socket with unread request bytes resets it on Windows, and the peer may then lose
/// the refusal it has not read yet. Close only the write side and discard bounded input first.
async fn linger<S: AsyncRead + AsyncWrite + Unpin>(io: &mut S) {
    if let Err(error) = io.shutdown().await {
        tracing::debug!(%error, "proxy refusal shutdown failed");
        return;
    }
    let drain = async {
        let mut left = LINGER_BYTES;
        let mut buf = [0; 4096];
        while left > 0 {
            match io.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => left = left.saturating_sub(n),
                Err(error) => {
                    tracing::debug!(%error, "proxy refusal drain ended");
                    break;
                }
            }
        }
    };
    if tokio::time::timeout(LINGER, drain).await.is_err() {
        tracing::debug!("proxy refusal drain reached its deadline");
    }
}

#[cfg(test)]
#[path = "proxy_tests.rs"]
mod tests;
