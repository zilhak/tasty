use super::*;
fn token() -> EndpointToken {
    EndpointToken::from_random_bytes([17; 32])
}
#[tokio::test]
async fn no_auth_is_rejected_before_any_http() {
    let (mut server, mut peer) = tokio::io::duplex(512);
    peer.write_all(&[5, 1, 0]).await.unwrap();
    assert!(matches!(
        authenticate(&mut server, &token(), Duration::from_secs(1)).await,
        Err(Error::Invalid("endpoint authentication required"))
    ));
    let mut b = [0; 2];
    peer.read_exact(&mut b).await.unwrap();
    assert_eq!(b, [5, 255]);
}
#[tokio::test]
async fn wrong_token_never_reaches_connect() {
    let (mut server, mut peer) = tokio::io::duplex(512);
    let request = [vec![5, 1, 2, 1, 5], b"tasty".to_vec(), vec![1, b'x']].concat();
    peer.write_all(&request).await.unwrap();
    assert!(matches!(
        authenticate(&mut server, &token(), Duration::from_secs(1)).await,
        Err(Error::Invalid("endpoint authentication failed"))
    ));
    let mut b = [0; 4];
    peer.read_exact(&mut b).await.unwrap();
    assert_eq!(b, [5, 2, 1, 1]);
}
fn authenticated_request() -> Vec<u8> {
    [
        vec![5, 1, 2, 1, 5],
        b"tasty".to_vec(),
        vec![64],
        token().0.to_vec(),
    ]
    .concat()
}
#[tokio::test]
async fn authenticated_authority_preserves_http_until_explicit_accept() {
    let (mut server, mut peer) = tokio::io::duplex(512);
    let mut request = authenticated_request();
    request.extend_from_slice(&[5, 1, 0, 3, 9]);
    request.extend_from_slice(b"localhost");
    request.extend_from_slice(&8080u16.to_be_bytes());
    request.extend_from_slice(b"OPTIONS /path HTTP/1.1\r\n\r\n");
    peer.write_all(&request).await.unwrap();
    let authority = authenticate(&mut server, &token(), Duration::from_secs(1))
        .await
        .unwrap();
    assert_eq!(
        authority,
        EndpointAuthority {
            host: "localhost".into(),
            port: 8080
        }
    );
    let mut auth = [0; 4];
    peer.read_exact(&mut auth).await.unwrap();
    assert_eq!(auth, [5, 2, 1, 0]);
    assert!(
        tokio::time::timeout(Duration::from_millis(10), peer.read_u8())
            .await
            .is_err()
    );
    accept_route(&mut server).await.unwrap();
    let mut ack = [0; 10];
    peer.read_exact(&mut ack).await.unwrap();
    assert_eq!(ack, [5, 0, 0, 1, 127, 0, 0, 1, 0, 0]);
    let mut http = [0; 26];
    server.read_exact(&mut http).await.unwrap();
    assert_eq!(&http, b"OPTIONS /path HTTP/1.1\r\n\r\n");
}
#[tokio::test]
async fn udp_and_zero_port_fail_closed() {
    for command in [3, 1] {
        let (mut server, mut peer) = tokio::io::duplex(512);
        let mut r = authenticated_request();
        r.extend_from_slice(&[5, command, 0, 1, 127, 0, 0, 1, 0, 0]);
        peer.write_all(&r).await.unwrap();
        let error = authenticate(&mut server, &token(), Duration::from_secs(1))
            .await
            .unwrap_err();
        assert_eq!(
            error,
            if command == 3 {
                Error::UnsupportedUpgrade
            } else {
                Error::Invalid("endpoint port")
            }
        );
    }
}
#[tokio::test]
async fn incomplete_authentication_is_bounded_and_dropped_stream_closes() {
    let (mut server, mut peer) = tokio::io::duplex(512);
    assert_eq!(
        authenticate(&mut server, &token(), Duration::from_millis(10))
            .await
            .unwrap_err(),
        Error::Timeout
    );
    drop(server);
    assert_eq!(
        peer.read_u8().await.unwrap_err().kind(),
        std::io::ErrorKind::UnexpectedEof
    );
    assert!(!format!("{:?}", token()).contains("11"));
}

#[test]
fn proxy_uri_and_authority_are_canonical_and_secrets_are_redacted() {
    let token = token();
    let uri = token.proxy_uri(1234);
    let url = reqwest::Url::parse(&uri).unwrap();
    assert_eq!(url.scheme(), "socks5");
    assert_eq!(url.username(), "tasty");
    assert_eq!(url.password().unwrap().len(), 64);
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert_eq!(url.port(), Some(1234));
    assert_eq!(hostname(b"localhost").unwrap(), "localhost");
    for invalid in [
        b"bad@localhost".as_slice(),
        b"localhost:80",
        b"localhost/path",
        b"",
    ] {
        assert!(hostname(invalid).is_err());
    }
    assert!(!format!("{token:?}").contains(url.password().unwrap()));
}
