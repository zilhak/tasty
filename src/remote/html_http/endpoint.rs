//! Authentication precedes HTTP parsing, including browser preflight requests.
//! This module never tunnels TCP. After negotiation the caller must parse HTTP or
//! terminate mirror-local TLS and forward individual requests through the executor.
use super::{Error, Result};
use std::{
    net::{Ipv4Addr, Ipv6Addr},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Caller supplies 256 bits of fresh OS-backed randomness for one mirror session.
/// This value belongs in its scoped proxy URI, never logs or persisted settings.
pub struct EndpointToken([u8; 64]);
impl EndpointToken {
    pub fn from_random_bytes(entropy: [u8; 32]) -> Self {
        let mut out = [0; 64];
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for (i, b) in entropy.into_iter().enumerate() {
            out[2 * i] = HEX[(b >> 4) as usize];
            out[2 * i + 1] = HEX[(b & 15) as usize];
        }
        Self(out)
    }
    /// Construct only the dedicated mirror's SOCKS5 URI, with an empty bypass list.
    pub fn proxy_uri(&self, port: u16) -> String {
        format!(
            "socks5://tasty:{}@127.0.0.1:{port}",
            String::from_utf8_lossy(&self.0)
        )
    }
    fn matches(&self, presented: &[u8]) -> bool {
        if presented.len() != self.0.len() {
            return false;
        }
        // Same length/XOR policy as webhook auth; no machine-code timing guarantee.
        let mut diff = 0;
        for (a, b) in self.0.iter().zip(presented) {
            diff |= a ^ b;
        }
        diff == 0
    }
}
impl std::fmt::Debug for EndpointToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("EndpointToken([redacted])")
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointAuthority {
    pub host: String,
    pub port: u16,
}

/// Authenticates RFC1929 and reads one SOCKS5 CONNECT authority within one timeout.
/// No SOCKS success reply is sent yet: the caller checks live generation, capacity
/// and destination, then prepares an HTTP/TLS parser before calling accept_route.
/// Dropping this future cancels its IO; the owner then drops the stream.
pub async fn authenticate<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut S,
    token: &EndpointToken,
    deadline: Duration,
) -> Result<EndpointAuthority> {
    if deadline.is_zero() {
        return Err(Error::Invalid("endpoint deadline"));
    }
    tokio::time::timeout(deadline, negotiate(io, token))
        .await
        .map_err(|_| Error::Timeout)?
}
async fn negotiate<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut S,
    token: &EndpointToken,
) -> Result<EndpointAuthority> {
    let mut head = [0; 2];
    io.read_exact(&mut head).await.map_err(|_| Error::Network)?;
    let mut methods = vec![0; head[1] as usize];
    io.read_exact(&mut methods)
        .await
        .map_err(|_| Error::Network)?;
    if head[0] != 5 || !methods.contains(&2) {
        send(io, &[5, 255]).await?;
        return Err(Error::Invalid("endpoint authentication required"));
    }
    send(io, &[5, 2]).await?;
    io.read_exact(&mut head).await.map_err(|_| Error::Network)?;
    let mut user = vec![0; head[1] as usize];
    io.read_exact(&mut user).await.map_err(|_| Error::Network)?;
    let len = io.read_u8().await.map_err(|_| Error::Network)?;
    let mut password = vec![0; len as usize];
    io.read_exact(&mut password)
        .await
        .map_err(|_| Error::Network)?;
    if head[0] != 1 || user != b"tasty" || !token.matches(&password) {
        send(io, &[1, 1]).await?;
        return Err(Error::Invalid("endpoint authentication failed"));
    }
    send(io, &[1, 0]).await?;
    let mut request = [0; 4];
    io.read_exact(&mut request)
        .await
        .map_err(|_| Error::Network)?;
    if request[..3] != [5, 1, 0] {
        reject_route(io).await?;
        return Err(Error::UnsupportedUpgrade);
    }
    let host = match request[3] {
        1 => {
            let mut address = [0; 4];
            io.read_exact(&mut address)
                .await
                .map_err(|_| Error::Network)?;
            Ipv4Addr::from(address).to_string()
        }
        4 => {
            let mut address = [0; 16];
            io.read_exact(&mut address)
                .await
                .map_err(|_| Error::Network)?;
            Ipv6Addr::from(address).to_string()
        }
        3 => {
            let len = io.read_u8().await.map_err(|_| Error::Network)?;
            let mut name = vec![0; len as usize];
            io.read_exact(&mut name).await.map_err(|_| Error::Network)?;
            hostname(&name)?
        }
        _ => return Err(Error::Invalid("endpoint address type")),
    };
    let port = io.read_u16().await.map_err(|_| Error::Network)?;
    if port == 0 {
        return Err(Error::Invalid("endpoint port"));
    }
    Ok(EndpointAuthority { host, port })
}
fn hostname(bytes: &[u8]) -> Result<String> {
    let name = std::str::from_utf8(bytes).map_err(|_| Error::Invalid("endpoint hostname"))?;
    let url = reqwest::Url::parse(&format!("http://{name}/"))
        .map_err(|_| Error::Invalid("endpoint hostname"))?;
    if name.is_empty()
        || url.host_str() != Some(name)
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
    {
        return Err(Error::Invalid("endpoint hostname"));
    }
    Ok(name.to_owned())
}
async fn send<S: AsyncWrite + Unpin>(io: &mut S, bytes: &[u8]) -> Result<()> {
    io.write_all(bytes).await.map_err(|_| Error::Network)?;
    io.flush().await.map_err(|_| Error::Network)
}
/// Only after preparing response-aware HTTP/TLS handling; never a blind tunnel.
pub async fn accept_route<S: AsyncWrite + Unpin>(io: &mut S) -> Result<()> {
    send(io, &[5, 0, 0, 1, 127, 0, 0, 1, 0, 0]).await
}
pub async fn reject_route<S: AsyncWrite + Unpin>(io: &mut S) -> Result<()> {
    send(io, &[5, 2, 0, 1, 0, 0, 0, 0, 0, 0]).await
}

#[cfg(test)]
mod tests;
