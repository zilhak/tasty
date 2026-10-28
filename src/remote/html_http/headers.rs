//! End-to-end byte values survive; connection-local fields never cross the proxy.
use super::{Error, Header, Result};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
const HOP: &[&str] = &[
    "connection",
    "proxy-connection",
    "keep-alive",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
    "proxy-authorization",
    "proxy-authenticate",
];
pub(super) fn clean(headers: &[Header], limit: usize, request: bool) -> Result<Vec<Header>> {
    let mut names = Vec::new();
    let mut bytes = 0usize;
    for h in headers {
        bytes = bytes
            .checked_add(h.name.len())
            .and_then(|n| n.checked_add(h.value.len()))
            .ok_or(Error::Quota("headers"))?;
        if bytes > limit {
            return Err(Error::Quota("headers"));
        }
        HeaderName::from_bytes(h.name.as_bytes()).map_err(|_| Error::Invalid("header name"))?;
        HeaderValue::from_bytes(&h.value).map_err(|_| Error::Invalid("header value"))?;
        if h.name.eq_ignore_ascii_case("connection") {
            for name in h.value.split(|b| *b == b',') {
                let name = std::str::from_utf8(name)
                    .map_err(|_| Error::Invalid("connection options"))?
                    .trim()
                    .to_ascii_lowercase();
                HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| Error::Invalid("connection option name"))?;
                // A peer cannot nominate a policy/representation header and thereby weaken it.
                if name.starts_with("access-control-")
                    || [
                        "content-security-policy",
                        "content-security-policy-report-only",
                        "content-encoding",
                        "content-type",
                        "authorization",
                        "origin",
                        "referer",
                        "cookie",
                        "set-cookie",
                        "location",
                    ]
                    .contains(&name.as_str())
                {
                    return Err(Error::Invalid("end-to-end connection option"));
                }
                names.push(name);
            }
        }
    }
    Ok(headers
        .iter()
        .filter(|h| {
            let n = h.name.to_ascii_lowercase();
            !HOP.contains(&n.as_str())
                && !names.contains(&n)
                && !(request && (n == "host" || n == "content-length"))
        })
        .cloned()
        .collect())
}
/// The backend decodes only these codings for the script scan. A browser's wider offer
/// (Chromium adds br and zstd over HTTPS) would let the origin pick an undecodable body.
const ACCEPT_ENCODING: &[u8] = b"gzip, deflate";
pub(super) fn limit_accept_encoding(headers: &mut Vec<Header>) {
    let offered = headers
        .iter()
        .any(|h| h.name.eq_ignore_ascii_case("accept-encoding"));
    if offered {
        headers.retain(|h| !h.name.eq_ignore_ascii_case("accept-encoding"));
        headers.push(Header {
            name: "accept-encoding".into(),
            value: ACCEPT_ENCODING.to_vec(),
        });
    }
}
pub(super) fn request(headers: Vec<Header>) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();
    for h in headers {
        map.append(
            HeaderName::from_bytes(h.name.as_bytes()).map_err(|_| Error::Invalid("header name"))?,
            HeaderValue::from_bytes(&h.value).map_err(|_| Error::Invalid("header value"))?,
        );
    }
    Ok(map)
}
pub(super) fn response(map: &HeaderMap, limit: usize) -> Result<Vec<Header>> {
    clean(
        &map.iter()
            .map(|(n, v)| Header {
                name: n.to_string(),
                value: v.as_bytes().to_vec(),
            })
            .collect::<Vec<_>>(),
        limit,
        false,
    )
}
