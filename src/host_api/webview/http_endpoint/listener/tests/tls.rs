//! Real SOCKS → mapped TLS → HTTP controls; this is not WebKit trust-callback evidence.
use super::*;
use tokio_rustls::{TlsConnector, rustls};
#[derive(Debug)]
pub(super) struct Pinned(pub(super) [u8; 32]);
impl rustls::client::danger::ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        leaf: &rustls::pki_types::CertificateDer<'_>,
        _: &[rustls::pki_types::CertificateDer<'_>],
        _: &rustls::pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if <[u8; 32]>::from(Sha256::digest(leaf)) != self.0 {
            return Err(rustls::Error::General("fixture leaf mismatch".into()));
        }
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &rustls::crypto::aws_lc_rs::default_provider().signature_verification_algorithms,
        )
    }
    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &rustls::crypto::aws_lc_rs::default_provider().signature_verification_algorithms,
        )
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        rustls::crypto::aws_lc_rs::default_provider()
            .signature_verification_algorithms
            .supported_schemes()
    }
}
async fn tls(endpoint: &Endpoint, sni: &'static str) -> tokio_rustls::client::TlsStream<TcpStream> {
    let prepared = endpoint
        .prepare_https("https://remote.example:8443/doc")
        .unwrap()
        .await
        .unwrap_or_else(|_| panic!("prepared endpoint authority"));
    assert!(prepared.is_current());
    assert_eq!(prepared.host(), "remote.example");
    assert_eq!(prepared.port(), 8443);
    let mut socket = authenticated(endpoint, None).await;
    assert_eq!(route(&mut socket, "remote.example", 8443).await[1], 0);
    // Pin preparation precedes even SOCKS admission and performs no speculative HTTP request.
    // The private verifier checks that prepared leaf and signatures, including foreign SNI.
    assert_eq!(
        endpoint.expected_leaf("remote.example", 8443),
        Some(prepared.leaf())
    );
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::aws_lc_rs::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .dangerous()
    .with_custom_certificate_verifier(Arc::new(Pinned(prepared.leaf())))
    .with_no_client_auth();
    TlsConnector::from(Arc::new(config))
        .connect(
            rustls::pki_types::ServerName::try_from(sni).unwrap(),
            socket,
        )
        .await
        .unwrap()
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn mapped_tls_preserves_binary_post_and_rejects_foreign_sni_before_handler() {
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let handler: Handler = Arc::new(move |request| {
        seen.fetch_add(1, Ordering::AcqRel);
        Box::pin(async move {
            assert_eq!(request.method, "POST");
            assert_eq!(request.upload.as_slice(), b"\0\xffx");
            assert_eq!(request.logical_url, "https://remote.example:8443/doc");
            Ok(reply(request))
        })
    });
    let budget = EndpointBudget::new();
    let endpoint = Endpoint::start(
        Handle::current(),
        config("https://remote.example:8443/doc".into(), handler),
        budget.clone(),
    )
    .await
    .unwrap_or_else(|_| panic!("endpoint"));
    let mut socket = tls(&endpoint, "remote.example").await;
    socket
        .write_all(
            b"POST /doc HTTP/1.1\r\nHost: remote.example:8443\r\nContent-Length: 3\r\n\r\n\0\xffx",
        )
        .await
        .unwrap();
    let mut bytes = Vec::new();
    // A finite HTTP Connection: close need not send TLS close_notify. Read through the body.
    let mut byte = [0];
    while !bytes.ends_with(b"REMOTE body") {
        socket.read_exact(&mut byte).await.unwrap();
        bytes.push(byte[0]);
    }
    assert!(bytes.starts_with(b"HTTP/1.1 200 \r\n"));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    drop(socket);
    let mut wrong = tls(&endpoint, "foreign.example").await;
    let written = wrong
        .write_all(
            b"POST /doc HTTP/1.1\r\nHost: remote.example:8443\r\nContent-Length: 3\r\n\r\n\0\xffx",
        )
        .await;
    if let Err(error) = written {
        tracing::debug!(%error,"expected wrong-SNI close");
    }
    assert!(matches!(wrong.read(&mut byte).await, Ok(0) | Err(_)));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    endpoint.join().await.unwrap();
    assert!(endpoint_pin_retired(&budget));
}
fn endpoint_pin_retired(budget: &EndpointBudget) -> bool {
    budget.active_connections() == 0 && budget.active_owners() == 0
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retirement_during_authentication_prevents_route_and_joins_old_sockets() {
    let active = Arc::new(AtomicBool::new(true));
    let current = active.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let handler: Handler = Arc::new(move |request| {
        seen.fetch_add(1, Ordering::AcqRel);
        Box::pin(async move { Ok(reply(request)) })
    });
    let mut settings = config("http://remote.example:8080/doc".into(), handler);
    settings.current = Arc::new(move || current.load(Ordering::Acquire));
    let budget = EndpointBudget::new();
    let endpoint = Endpoint::start(Handle::current(), settings, budget.clone())
        .await
        .unwrap_or_else(|_| panic!("endpoint"));
    let mut socket = authenticated(&endpoint, None).await;
    active.store(false, Ordering::Release);
    let mut route = vec![5, 1, 0, 3, 14];
    route.extend_from_slice(b"remote.example");
    route.extend_from_slice(&8080u16.to_be_bytes());
    socket.write_all(&route).await.unwrap();
    let mut bytes = Vec::new();
    if let Err(error) = socket.read_to_end(&mut bytes).await {
        assert!(close_observation(Err(error)));
    }
    assert!(bytes.is_empty());
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert!(endpoint.expected_leaf("remote.example", 8080).is_none());
    endpoint.join().await.unwrap();
    assert!(endpoint_pin_retired(&budget));
}
