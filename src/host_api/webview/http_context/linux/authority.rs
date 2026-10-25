#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Authority {
    pub host: String,
    pub port: u16,
}
pub(super) fn https(uri: &str) -> Option<Authority> {
    let url = url::Url::parse(uri).ok()?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    Some(Authority {
        host: match url.host()? {
            url::Host::Domain(host) => host.to_ascii_lowercase(),
            url::Host::Ipv4(host) => host.to_string(),
            url::Host::Ipv6(host) => host.to_string(),
        },
        port: url.port_or_known_default()?,
    })
}
pub(super) fn proxy(uri: &str) -> bool {
    let Ok(url) = url::Url::parse(uri) else {
        return false;
    };
    url.scheme() == "socks5"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some_and(|port| port != 0)
        && !url.username().is_empty()
        && url.password().is_some_and(|p| !p.is_empty())
        && matches!(url.path(), "" | "/")
        && url.query().is_none()
        && url.fragment().is_none()
}
