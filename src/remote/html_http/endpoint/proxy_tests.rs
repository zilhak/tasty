use super::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
fn token() -> EndpointToken {
    EndpointToken::from_random_bytes([17; 32])
}
fn realm() -> ProxyRealm {
    ProxyRealm::from_random_bytes([3; 16])
}
fn basic(password: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(format!("tasty:{password}"))
}
fn good() -> String {
    basic(&credentials(&token()).1)
}
async fn run(request: &str) -> (Result<ProxyRoute>, Vec<u8>) {
    let (mut server, mut peer) = tokio::io::duplex(64 * 1024);
    peer.write_all(request.as_bytes()).await.unwrap();
    peer.shutdown().await.unwrap();
    let result = authenticate_http(&mut server, &token(), &realm(), Duration::from_secs(1)).await;
    drop(server);
    let mut reply = Vec::new();
    peer.read_to_end(&mut reply).await.unwrap();
    (result, reply)
}
#[tokio::test]
async fn missing_and_wrong_credentials_receive_only_the_exact_challenge() {
    let challenge = format!(
        "HTTP/1.1 407 Proxy Authentication Required\r\nProxy-Authenticate: {}\r\n",
        realm().challenge()
    );
    for auth in [
        String::new(),
        format!("Proxy-Authorization: Basic {}\r\n", basic("wrong")),
        format!(
            "Proxy-Authorization: Bearer {}\r\n",
            credentials(&token()).1
        ),
    ] {
        let (result, reply) = run(&format!(
            "POST http://remote.example/doc HTTP/1.1\r\nHost: remote.example\r\n{auth}Content-Length: 4\r\n\r\nbody"
        ))
        .await;
        assert!(matches!(
            result,
            Err(Error::Invalid("endpoint authentication required"))
        ));
        assert!(reply.starts_with(challenge.as_bytes()));
        assert!(!reply.windows(4).any(|w| w == b"body"));
    }
}
#[tokio::test]
async fn authenticated_connect_returns_authority_without_a_reply() {
    for (target, host, port) in [
        ("remote.example:443", "remote.example", 443),
        ("127.0.0.1:3000", "127.0.0.1", 3000),
        ("[::1]:8443", "::1", 8443),
    ] {
        let (result, reply) = run(&format!(
            "CONNECT {target} HTTP/1.1\r\nHost: {target}\r\nProxy-Authorization: Basic {}\r\n\r\n",
            good()
        ))
        .await;
        assert_eq!(
            result.unwrap(),
            ProxyRoute::Tunnel(EndpointAuthority {
                host: host.into(),
                port
            })
        );
        assert!(reply.is_empty());
    }
}
#[tokio::test]
async fn absolute_form_request_drops_only_proxy_headers_and_leaves_body_unread() {
    let (mut server, mut peer) = tokio::io::duplex(64 * 1024);
    peer.write_all(
        format!(
            "POST http://localhost:3000/form?q=1 HTTP/1.1\r\nHost: localhost:3000\r\nProxy-Connection: keep-alive\r\nProxy-Authorization: Basic {}\r\nCookie: a=1\r\nContent-Length: 3\r\n\r\nabc",
            good()
        )
        .as_bytes(),
    )
    .await
    .unwrap();
    let route = authenticate_http(&mut server, &token(), &realm(), Duration::from_secs(1))
        .await
        .unwrap();
    let ProxyRoute::Forward { authority, head } = route else {
        panic!("forward route");
    };
    assert_eq!(
        authority,
        EndpointAuthority {
            host: "localhost".into(),
            port: 3000
        }
    );
    assert_eq!(
        head,
        b"POST http://localhost:3000/form?q=1 HTTP/1.1\r\nHost: localhost:3000\r\nCookie: a=1\r\nContent-Length: 3\r\n\r\n"
    );
    let mut body = [0; 3];
    server.read_exact(&mut body).await.unwrap();
    assert_eq!(&body, b"abc");
}
#[tokio::test]
async fn unsupported_targets_are_forbidden_after_authentication() {
    for target in [
        "GET /origin-form HTTP/1.1",
        "GET https://remote.example/ HTTP/1.1",
        "GET http://user:pw@remote.example/ HTTP/1.1",
        "CONNECT remote.example:0 HTTP/1.1",
        "CONNECT ::1:443 HTTP/1.1",
        "GET http://remote.example/ HTTP/2",
    ] {
        let (result, reply) = run(&format!(
            "{target}\r\nHost: remote.example\r\nProxy-Authorization: Basic {}\r\n\r\n",
            good()
        ))
        .await;
        assert!(result.is_err(), "{target}");
        assert!(reply.starts_with(b"HTTP/1.1 403 Forbidden\r\n"), "{target}");
    }
}
#[tokio::test]
async fn duplicate_authorization_and_oversized_heads_are_refused() {
    let (result, reply) = run(&format!(
        "CONNECT remote.example:443 HTTP/1.1\r\nProxy-Authorization: Basic {0}\r\nProxy-Authorization: Basic {0}\r\n\r\n",
        good()
    ))
    .await;
    assert!(result.is_err());
    assert!(reply.starts_with(b"HTTP/1.1 403 Forbidden\r\n"));
    let (result, _) = run(&format!(
        "GET http://remote.example/ HTTP/1.1\r\nX: {}\r\n\r\n",
        "a".repeat(HEAD_BYTES)
    ))
    .await;
    assert!(matches!(result, Err(Error::Quota("proxy request head"))));
}
#[test]
fn realm_and_token_are_redacted() {
    assert_eq!(format!("{:?}", realm()), "ProxyRealm([redacted])");
    assert!(realm().challenge().starts_with("Basic realm=\"tasty-"));
}
