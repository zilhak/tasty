use super::*;
use crate::html_http_body::{self, BodyBudget, VerifiedBody};
use sha2::{Digest, Sha256};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tasty_ipc::{
    html_http::{BodyIdentity, Header, HttpRequest, ResponseHead, ResponseId},
    html_transfer::TransferId,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
fn limits() -> NativeLimits {
    NativeLimits {
        upload_bytes: 1_000_000,
        io_timeout: Duration::from_secs(2),
    }
}
fn destination() -> Destination {
    Destination::new("http", "example.test", 8080).unwrap()
}
fn identity(bytes: &[u8]) -> BodyIdentity {
    BodyIdentity {
        bytes: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    }
}
async fn read(bytes: &[u8], budget: &NativeBudget) -> Result<NativeRequest> {
    let (mut io, mut peer) = tokio::io::duplex(bytes.len() + 1);
    peer.write_all(bytes).await.unwrap();
    peer.shutdown().await.unwrap();
    read_request(&mut io, &destination(), limits(), budget, &|| true).await
}
#[tokio::test]
async fn binary_upload_and_raw_headers_bind_authenticated_destination() {
    let budget = NativeBudget::new(1_000_000);
    let request = read(b"POST /submit?q=%2F HTTP/1.1\r\nHost: example.test:8080\r\nCookie: one=1\r\nCookie: raw=\xff\r\nContent-Length: 3\r\n\r\n\0x\xff", &budget).await.unwrap_or_else(|_| panic!("native request"));
    assert_eq!(request.method, "POST");
    assert_eq!(request.logical_url, "http://example.test:8080/submit?q=%2F");
    assert_eq!(request.upload.as_slice(), b"\0x\xff");
    assert_eq!(request.identity, identity(b"\0x\xff"));
    let cookies: Vec<_> = request
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("cookie"))
        .map(|h| h.value.as_slice())
        .collect();
    assert_eq!(cookies, [b"one=1".as_slice(), b"raw=\xff"]);
    assert!(budget.used() >= request.upload.capacity());
    drop(request);
    assert_eq!(budget.used(), 0);
}
#[tokio::test]
async fn finite_chunked_upload_is_dechunked_without_changing_payload() {
    let budget = NativeBudget::new(1_000_000);
    let request = read(b"POST / HTTP/1.1\r\nHost: example.test:8080\r\nTransfer-Encoding: chunked\r\n\r\n2\r\n\0x\r\n1;owned=yes\r\n\xff\r\n0\r\n\r\n", &budget).await.unwrap_or_else(|_| panic!("chunked"));
    assert_eq!(request.upload.as_slice(), b"\0x\xff");
    assert_eq!(request.identity, identity(b"\0x\xff"));
    drop(request);
    assert_eq!(budget.used(), 0);
}
#[tokio::test]
async fn request_smuggling_destination_and_upgrade_cases_fail_closed() {
    let budget = NativeBudget::new(1_000_000);
    for bytes in [
        b"POST / HTTP/1.1\r\nHost: example.test:8080\r\nContent-Length: 0\r\nTransfer-Encoding: chunked\r\n\r\n".as_slice(),
        b"GET / HTTP/1.1\r\nHost: example.test:8080\r\nHost: evil.test\r\n\r\n",
        b"GET http://evil.test/ HTTP/1.1\r\nHost: example.test:8080\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: client.local:8080\r\n\r\n",
        b"CONNECT example.test:8080 HTTP/1.1\r\nHost: example.test:8080\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: example.test:8080\r\nUpgrade: websocket\r\n\r\n",
        b"POST / HTTP/1.1\r\nHost: example.test:8080\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx",
        b"GET //evil.test/ HTTP/1.1\r\nHost: example.test:8080\r\n\r\n",
        b"GET / HTTP/1.1\r\nHost: example.test:8080\r\nTransfer-Encoding: gzip, chunked\r\n\r\n",
    ] {
        assert!(read(bytes, &budget).await.is_err()); assert_eq!(budget.used(), 0);
    }
}
#[tokio::test]
async fn native_expect_continue_precedes_body_and_preserves_binary_post() {
    let budget = NativeBudget::new(1_000_000);
    let (mut io, mut peer) = tokio::io::duplex(256);
    let destination = destination();
    let read = read_request(&mut io, &destination, limits(), &budget, &|| true);
    let peer = async {
        peer.write_all(b"POST / HTTP/1.1\r\nHost: example.test:8080\r\nExpect: 100-continue\r\nContent-Length: 3\r\n\r\n").await.unwrap();
        let mut interim = [0; 25];
        peer.read_exact(&mut interim).await.unwrap();
        assert_eq!(&interim, b"HTTP/1.1 100 Continue\r\n\r\n");
        peer.write_all(b"\0x\xff").await.unwrap();
    };
    let (request, ()) = tokio::join!(read, peer);
    let request = request.unwrap_or_else(|_| panic!("expect"));
    assert_eq!(request.upload.as_slice(), b"\0x\xff");
    assert!(
        !request
            .headers
            .iter()
            .any(|h| h.name.eq_ignore_ascii_case("expect"))
    );
}
#[tokio::test]
async fn quota_and_retired_generation_release_native_capacity() {
    let budget = NativeBudget::new(1_000_000);
    assert!(matches!(
        read(
            b"POST / HTTP/1.1\r\nHost: example.test:8080\r\nContent-Length: 1000001\r\n\r\n",
            &budget
        )
        .await,
        Err(NativeError::Quota)
    ));
    assert_eq!(budget.used(), 0);
    let (mut io, mut peer) = tokio::io::duplex(256);
    peer.write_all(b"GET / HTTP/1.1\r\nHost: example.test:8080\r\n\r\n")
        .await
        .unwrap();
    let calls = AtomicUsize::new(0);
    let current = || calls.fetch_add(1, Ordering::AcqRel) < 2;
    assert!(matches!(
        read_request(&mut io, &destination(), limits(), &budget, &current).await,
        Err(NativeError::Closed)
    ));
    assert_eq!(budget.used(), 0);
    assert!(
        read(
            b"GET / HTTP/1.1\r\nHost: example.test:8080\r\n\r\n",
            &NativeBudget::new(1)
        )
        .await
        .is_err()
    );
}
fn verified(encoded: Vec<u8>, decoded: &[u8], encoding: Option<&str>) -> Arc<VerifiedBody> {
    let id = TransferId {
        epoch: 1,
        surface_id: 2,
        binding: 3,
        document: 4,
        request: 5,
        transfer: 6,
    };
    let request = HttpRequest::Begin {
        id,
        source_generation: 7,
        method: "POST".into(),
        logical_url: "http://example.test:8080/".into(),
        headers: vec![],
        upload: identity(b"\0x\xff"),
        allow_remote_content: false,
    };
    let mut headers = vec![
        Header {
            name: "Set-Cookie".into(),
            value: b"one=1".to_vec(),
        },
        Header {
            name: "Set-Cookie".into(),
            value: b"two=\xff".to_vec(),
        },
        Header {
            name: "Location".into(),
            value: b"/next".to_vec(),
        },
    ];
    if let Some(encoding) = encoding {
        headers.push(Header {
            name: "Content-Encoding".into(),
            value: encoding.as_bytes().to_vec(),
        });
    }
    finish_verified(encoded, decoded, headers, id, request)
}
fn finish_verified(
    encoded: Vec<u8>,
    decoded: &[u8],
    headers: Vec<Header>,
    id: TransferId,
    request: HttpRequest,
) -> Arc<VerifiedBody> {
    let head = ResponseHead {
        id: ResponseId {
            request: id,
            source_generation: 7,
            serial: 8,
        },
        logical_url: "http://example.test:8080/".into(),
        status: 307,
        headers,
        encoded_length: Some(encoded.len() as u64),
        has_body: true,
    };
    let ids = (identity(&encoded), identity(decoded));
    html_http_body::verify(
        &request,
        head,
        Arc::new(encoded),
        ids,
        &BodyBudget::new(1_000_000),
        1_000_000,
        &AtomicBool::new(false),
    )
    .unwrap_or_else(|_| panic!("verify"))
}
#[tokio::test]
async fn native_response_preserves_status_duplicate_headers_and_verified_encoded_gzip() {
    use std::io::Write;
    let decoded = b"<script>owned()</script>";
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(decoded).unwrap();
    let encoded = gzip.finish().unwrap();
    let body = verified(encoded.clone(), decoded, Some("gzip"));
    let mut io = Vec::new();
    write_response(
        &mut io,
        body.clone(),
        body.head().id,
        "POST",
        identity(b"\0x\xff"),
        limits(),
        &|| true,
    )
    .await
    .unwrap();
    assert!(io.starts_with(b"HTTP/1.1 307 \r\n"));
    assert!(
        io.windows(b"Set-Cookie:".len())
            .filter(|s| *s == b"Set-Cookie:")
            .count()
            == 2
    );
    let end = io.windows(4).position(|s| s == b"\r\n\r\n").unwrap() + 4;
    assert_eq!(&io[end..], encoded);
    assert_eq!(body.decoded(), decoded);
    assert_eq!(identity(&io[end..]), body.encoded_identity());
}
#[tokio::test]
async fn native_response_full_identity_and_retirement_cannot_publish_any_bytes() {
    let body = verified(b"owned".to_vec(), b"owned", None);
    for mode in 0..4 {
        let mut expected = body.head().id;
        let mut method = "POST";
        let mut upload = identity(b"\0x\xff");
        if mode == 0 {
            expected.request.document += 1;
        }
        if mode == 1 {
            expected.serial += 1;
        }
        if mode == 2 {
            method = "GET";
        }
        if mode == 3 {
            upload = identity(b"changed");
        }
        let mut io = Vec::new();
        assert!(matches!(
            write_response(
                &mut io,
                body.clone(),
                expected,
                method,
                upload,
                limits(),
                &|| true
            )
            .await,
            Err(NativeError::Identity)
        ));
        assert!(io.is_empty());
    }
    let mut io = Vec::new();
    assert!(matches!(
        write_response(
            &mut io,
            body.clone(),
            body.head().id,
            "POST",
            identity(b"\0x\xff"),
            limits(),
            &|| false
        )
        .await,
        Err(NativeError::Closed)
    ));
    assert!(io.is_empty());
}

#[tokio::test]
async fn stalled_native_input_times_out_and_returns_all_capacity() {
    let budget = NativeBudget::new(1_000_000);
    let (mut io, _peer) = tokio::io::duplex(32);
    let limits = NativeLimits {
        io_timeout: Duration::from_millis(5),
        ..limits()
    };
    assert!(matches!(
        read_request(&mut io, &destination(), limits, &budget, &|| true).await,
        Err(NativeError::Timeout)
    ));
    assert_eq!(budget.used(), 0);
}
#[tokio::test]
async fn malformed_response_framing_is_rejected_before_status_publication() {
    let id = TransferId {
        epoch: 1,
        surface_id: 2,
        binding: 3,
        document: 4,
        request: 5,
        transfer: 6,
    };
    for (name, value) in [
        ("Content-Length", b"999".as_slice()),
        ("Connection", b"upgrade"),
    ] {
        let request = HttpRequest::Begin {
            id,
            source_generation: 7,
            method: "POST".into(),
            logical_url: "http://example.test:8080/".into(),
            headers: vec![],
            upload: identity(b"\0x\xff"),
            allow_remote_content: false,
        };
        let body = finish_verified(
            b"owned".to_vec(),
            b"owned",
            vec![Header {
                name: name.into(),
                value: value.to_vec(),
            }],
            id,
            request,
        );
        let mut io = Vec::new();
        assert!(
            write_response(
                &mut io,
                body.clone(),
                body.head().id,
                "POST",
                identity(b"\0x\xff"),
                limits(),
                &|| true
            )
            .await
            .is_err()
        );
        assert!(io.is_empty());
    }
}

mod correction;
