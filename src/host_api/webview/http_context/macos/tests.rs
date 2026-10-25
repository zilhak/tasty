use super::*;
#[test]
fn proxy_credentials_are_literal_and_loopback_only() {
    assert!(ProxyRoute::from_endpoint("socks5://u:p@127.0.0.1:4321").is_ok());
    for invalid in [
        "http://u:p@127.0.0.1:4321",
        "socks5://u:p@localhost:4321",
        "socks5://u:p@192.0.2.1:4321",
        "socks5://u:p@127.0.0.1:0",
        "socks5://u:p@127.0.0.1:80/path",
        "socks5://u:%00@127.0.0.1:80",
        "socks5://u:p:x@127.0.0.1:80",
        "socks5://:p@127.0.0.1:80",
    ] {
        assert!(ProxyRoute::from_endpoint(invalid).is_err());
    }
}
#[test]
fn authorities_use_effective_port_and_canonical_host() {
    assert_eq!(
        Authority::new("EXAMPLE.com", 443),
        Authority::new("example.com", 443)
    );
    assert_eq!(Authority::new("[::1]", 443), Authority::new("::1", 443));
    assert_ne!(
        Authority::new("example.com", 443),
        Authority::new("example.com", 8443)
    );
    assert!(Authority::new("*.example.com", 443).is_err());
    assert!(Authority::new("example.com", 0).is_err());
}
