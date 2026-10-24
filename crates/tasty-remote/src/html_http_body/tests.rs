use super::*;
use flate2::{
    Compression,
    write::{GzEncoder, ZlibEncoder},
};
use std::io::Write;
use tasty_ipc::{html_http::Header, html_transfer::TransferId};
fn identity(bytes: &[u8]) -> BodyIdentity {
    BodyIdentity {
        bytes: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    }
}
fn setup(encoding: &str, length: usize) -> (HttpRequest, ResponseHead) {
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
        logical_url: "https://example.test/post".into(),
        headers: vec![],
        upload: identity(b"private upload"),
        allow_remote_content: false,
    };
    let head = ResponseHead {
        id: ResponseId {
            request: id,
            source_generation: 7,
            serial: 8,
        },
        logical_url: "https://example.test/post".into(),
        status: 201,
        headers: if encoding.is_empty() {
            vec![]
        } else {
            vec![Header {
                name: "Content-Encoding".into(),
                value: encoding.as_bytes().to_vec(),
            }]
        },
        encoded_length: Some(length as u64),
        has_body: true,
    };
    (request, head)
}
fn gzip(body: &[u8]) -> Vec<u8> {
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(body).unwrap();
    gzip.finish().unwrap()
}
fn prepare(
    encoded: Vec<u8>,
    decoded: &[u8],
    encoding: &str,
    budget: &BodyBudget,
) -> Result<Arc<VerifiedBody>, BodyError> {
    let (request, head) = setup(encoding, encoded.len());
    let ids = (identity(&encoded), identity(decoded));
    verify(
        &request,
        head,
        Arc::new(encoded),
        ids,
        budget,
        1024 * 1024,
        &AtomicBool::new(false),
    )
}
#[test]
fn identity_reuses_allocation_and_full_native_post_identity() {
    let body = Arc::new(b"<script>owned()</script>".to_vec());
    let budget = BodyBudget::new(1024);
    let (request, head) = setup("", body.len());
    let id = head.id;
    let verified = verify(
        &request,
        head,
        body.clone(),
        (identity(&body), identity(&body)),
        &budget,
        1024,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(Arc::ptr_eq(&body, &verified.encoded));
    assert!(Arc::ptr_eq(&verified.encoded, &verified.decoded));
    assert!(budget.used() > body.capacity());
    assert!(verified.scan().detection.has_scripts());
    let upload = identity(b"private upload");
    assert!(verified.matches_native(id, "POST", upload));
    assert!(!verified.matches_native(id, "GET", upload));
    assert!(!verified.matches_native(id, "POST", identity(b"other upload")));
    assert!(!verified.matches_native(ResponseId { serial: 9, ..id }, "POST", upload));
    let clone = verified.clone();
    drop(verified);
    assert!(budget.used() > 0);
    drop(clone);
    assert_eq!(budget.used(), 0);
}
#[test]
fn gzip_scans_decoded_but_retains_exact_encoded_and_headers() {
    let decoded = b"<script>received_bytes()</script>";
    let encoded = gzip(decoded);
    let hash = identity(&encoded);
    let budget = BodyBudget::new(4096);
    let verified = prepare(encoded, decoded, "GZip", &budget).unwrap();
    assert_eq!(verified.encoded_identity(), hash);
    assert_ne!(verified.encoded_identity(), verified.decoded_identity());
    assert_eq!(verified.decoded(), decoded);
    assert_eq!(verified.scan().fingerprint.0, identity(decoded).sha256);
    assert!(verified.scan().detection.has_scripts());
    assert_eq!(verified.head().status, 201);
    assert_eq!(verified.head().logical_url, "https://example.test/post");
    assert_eq!(verified.head().headers[0].value, b"GZip");
    assert!(budget.used() > verified.encoded.capacity() + verified.decoded.capacity());
    drop(verified);
    assert_eq!(budget.used(), 0);
}
#[test]
fn deflate_and_concatenated_gzip_match_decoded_identity() {
    let body = b"<html>payload</html>";
    let mut deflate = ZlibEncoder::new(Vec::new(), Compression::default());
    deflate.write_all(body).unwrap();
    let budget = BodyBudget::new(4096);
    assert_eq!(
        prepare(deflate.finish().unwrap(), body, "deflate", &budget)
            .unwrap()
            .decoded(),
        body
    );
    let mut encoded = gzip(b"<html>");
    encoded.extend(gzip(b"payload</html>"));
    assert_eq!(
        prepare(encoded, body, "gzip", &budget).unwrap().decoded(),
        body
    );
    assert_eq!(budget.used(), 0);
}
#[test]
fn either_identity_mismatch_fails_and_releases_quota() {
    let body = gzip(b"<script>bad()</script>");
    let budget = BodyBudget::new(4096);
    let (request, head) = setup("gzip", body.len());
    let ids = (identity(&body), identity(b"<script>bad()</script>"));
    let wrong = BodyIdentity {
        sha256: [0; 32],
        ..ids.0
    };
    assert!(matches!(
        verify(
            &request,
            head.clone(),
            Arc::new(body.clone()),
            (wrong, ids.1),
            &budget,
            1024,
            &AtomicBool::new(false)
        ),
        Err(BodyError::Identity)
    ));
    let wrong = BodyIdentity {
        sha256: [0; 32],
        ..ids.1
    };
    assert!(matches!(
        verify(
            &request,
            head,
            Arc::new(body),
            (ids.0, wrong),
            &budget,
            1024,
            &AtomicBool::new(false)
        ),
        Err(BodyError::Identity)
    ));
    assert_eq!(budget.used(), 0);
}
#[test]
fn decoded_size_and_encoded_retention_are_independent_limits() {
    let decoded = vec![b'x'; 10000];
    let encoded = gzip(&decoded);
    let (request, head) = setup("gzip", encoded.len());
    let ids = (identity(&encoded), identity(&decoded));
    let budget = BodyBudget::new(20000);
    assert!(matches!(
        verify(
            &request,
            head.clone(),
            Arc::new(encoded.clone()),
            ids,
            &budget,
            9999,
            &AtomicBool::new(false)
        ),
        Err(BodyError::Quota)
    ));
    let budget = BodyBudget::new(encoded.capacity() + decoded.len() - 1);
    assert!(matches!(
        verify(
            &request,
            head,
            Arc::new(encoded),
            ids,
            &budget,
            10000,
            &AtomicBool::new(false)
        ),
        Err(BodyError::Quota)
    ));
    assert_eq!(budget.used(), 0);
}
#[test]
fn stale_generation_or_wrong_advertised_length_never_publishes() {
    let body = b"body";
    let budget = BodyBudget::new(4096);
    let (request, mut head) = setup("", body.len());
    let ids = (identity(body), identity(body));
    head.id.source_generation += 1;
    assert!(matches!(
        verify(
            &request,
            head.clone(),
            Arc::new(body.to_vec()),
            ids,
            &budget,
            1024,
            &AtomicBool::new(false)
        ),
        Err(BodyError::Identity)
    ));
    head.id.source_generation -= 1;
    head.encoded_length = Some(100);
    assert!(matches!(
        verify(
            &request,
            head,
            Arc::new(body.to_vec()),
            ids,
            &budget,
            1024,
            &AtomicBool::new(false)
        ),
        Err(BodyError::Identity)
    ));
    assert_eq!(budget.used(), 0);
}
#[test]
fn cancellation_and_malformed_compression_fail_closed() {
    let budget = BodyBudget::new(4096);
    let body = b"body";
    let (request, head) = setup("", body.len());
    assert!(matches!(
        verify(
            &request,
            head,
            Arc::new(body.to_vec()),
            (identity(body), identity(body)),
            &budget,
            1024,
            &AtomicBool::new(true)
        ),
        Err(BodyError::Cancelled)
    ));
    assert!(matches!(
        prepare(b"not gzip".to_vec(), body, "gzip", &budget),
        Err(BodyError::Encoding)
    ));
    assert!(matches!(
        prepare(body.to_vec(), body, "br", &budget),
        Err(BodyError::Encoding)
    ));
    assert_eq!(budget.used(), 0);
}
#[test]
fn duplicate_encoding_or_body_on_no_body_response_is_rejected() {
    let budget = BodyBudget::new(4096);
    let body = b"body";
    let (request, mut head) = setup("identity", body.len());
    head.headers.push(Header {
        name: "content-encoding".into(),
        value: b"identity".to_vec(),
    });
    let ids = (identity(body), identity(body));
    assert!(matches!(
        verify(
            &request,
            head,
            Arc::new(body.to_vec()),
            ids,
            &budget,
            1024,
            &AtomicBool::new(false)
        ),
        Err(BodyError::Encoding)
    ));
    let (_, mut head) = setup("", body.len());
    head.has_body = false;
    assert!(
        verify(
            &request,
            head,
            Arc::new(body.to_vec()),
            ids,
            &budget,
            1024,
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(budget.used(), 0);
}
#[test]
fn identity_after_scan_limit_still_covers_entire_body() {
    let mut body = vec![b' '; SCAN_LIMIT_BYTES + 1];
    body.extend(b"<script>late()</script>");
    let budget = BodyBudget::new(body.capacity() + 1024);
    let (request, head) = setup("", body.len());
    let ids = (identity(&body), identity(&body));
    let prepared = verify(
        &request,
        head,
        Arc::new(body),
        ids,
        &budget,
        SCAN_LIMIT_BYTES + 1024,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(prepared.scan().fingerprint.0, ids.1.sha256);
    assert!(!prepared.scan().detection.has_scripts());
    drop(prepared);
    assert_eq!(budget.used(), 0);
}

#[test]
fn empty_no_body_keeps_status_and_metadata_quota_is_held() {
    let budget = BodyBudget::new(4096);
    let (request, mut head) = setup("gzip", 0);
    head.status = 204;
    head.has_body = false;
    let prepared = verify(
        &request,
        head,
        Arc::new(vec![]),
        (identity(b""), identity(b"")),
        &budget,
        0,
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(prepared.head().status, 204);
    assert!(prepared.encoded().is_empty());
    assert_eq!(prepared.head().headers[0].value, b"gzip");
    assert!(budget.used() > 0);
    drop(prepared);
    assert_eq!(budget.used(), 0);
    let (request, mut head) = setup("", 0);
    head.headers = Vec::with_capacity(1024);
    assert!(matches!(
        verify(
            &request,
            head,
            Arc::new(vec![]),
            (identity(b""), identity(b"")),
            &budget,
            0,
            &AtomicBool::new(false)
        ),
        Err(BodyError::Quota)
    ));
    assert_eq!(budget.used(), 0);
}
