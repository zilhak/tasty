//! Pre-upload duplicate classification mirrors the executor's normalized request fingerprint.
//! Keep normalization aligned with html_http::Document::open and its header cleaner.
use super::*;
use sha2::{Digest, Sha256};
pub fn begin(value: &wire::HttpRequest, header_limit: usize) -> core::Result<[u8; 32]> {
    let wire::HttpRequest::Begin {
        method,
        logical_url,
        headers,
        upload,
        ..
    } = value
    else {
        return Err(core::Error::Invalid("begin command"));
    };
    let method = reqwest::Method::from_bytes(method.as_bytes())
        .map_err(|_| core::Error::Invalid("HTTP method"))?;
    let mut url = reqwest::Url::parse(logical_url)
        .map_err(|_| core::Error::Invalid("absolute HTTP(S) URL"))?;
    if !["http", "https"].contains(&url.scheme()) || url.host_str().is_none() {
        return Err(core::Error::UnsupportedScheme);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(core::Error::Invalid("URL userinfo"));
    }
    url.set_fragment(None);
    if method == reqwest::Method::CONNECT
        || headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("upgrade"))
    {
        return Err(core::Error::UnsupportedUpgrade);
    }
    let mut names = Vec::new();
    let mut bytes = 0usize;
    for h in headers {
        bytes = bytes
            .checked_add(h.name.len())
            .and_then(|n| n.checked_add(h.value.len()))
            .ok_or(core::Error::Quota("headers"))?;
        if bytes > header_limit {
            return Err(core::Error::Quota("headers"));
        }
        reqwest::header::HeaderName::from_bytes(h.name.as_bytes())
            .map_err(|_| core::Error::Invalid("header name"))?;
        reqwest::header::HeaderValue::from_bytes(&h.value)
            .map_err(|_| core::Error::Invalid("header value"))?;
        if h.name.eq_ignore_ascii_case("connection") {
            for name in h.value.split(|b| *b == b',') {
                let name = std::str::from_utf8(name)
                    .map_err(|_| core::Error::Invalid("connection options"))?
                    .trim()
                    .to_ascii_lowercase();
                reqwest::header::HeaderName::from_bytes(name.as_bytes())
                    .map_err(|_| core::Error::Invalid("connection option name"))?;
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
                    return Err(core::Error::Invalid("end-to-end connection option"));
                }
                names.push(name);
            }
        }
    }
    let mut digest = Sha256::new();
    for bytes in [
        method.as_str().as_bytes(),
        url.as_str().as_bytes(),
        upload.sha256.as_slice(),
    ] {
        digest.update(bytes.len().to_le_bytes());
        digest.update(bytes);
    }
    digest.update(
        upload
            .checked_len()
            .map_err(core::Error::Invalid)?
            .to_le_bytes(),
    );
    for h in headers {
        let n = h.name.to_ascii_lowercase();
        if [
            "connection",
            "proxy-connection",
            "keep-alive",
            "te",
            "trailer",
            "transfer-encoding",
            "upgrade",
            "proxy-authorization",
            "proxy-authenticate",
            "host",
            "content-length",
        ]
        .contains(&n.as_str())
            || names.contains(&n)
        {
            continue;
        }
        digest.update(h.name.len().to_le_bytes());
        digest.update(n.as_bytes());
        digest.update(h.value.len().to_le_bytes());
        digest.update(&h.value);
    }
    Ok(digest.finalize().into())
}
