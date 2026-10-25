use super::*;
use sha2::{Digest, Sha256};
use tasty_ipc::{
    html_http::{BodyIdentity, HttpRequest, ResponseHead, ResponseId},
    html_transfer::TransferId,
};
fn identity(bytes: &[u8]) -> BodyIdentity {
    BodyIdentity {
        bytes: bytes.len() as u64,
        sha256: Sha256::digest(bytes).into(),
    }
}
fn response(n: u64, method: &str, upload: &[u8], body: &[u8]) -> Arc<VerifiedBody> {
    let id = TransferId {
        epoch: 1,
        surface_id: 2,
        binding: 3,
        document: 4,
        request: n,
        transfer: n,
    };
    let begin = HttpRequest::Begin {
        id,
        source_generation: 5,
        method: method.into(),
        logical_url: "http://example.test/post".into(),
        headers: vec![],
        upload: identity(upload),
        allow_remote_content: false,
    };
    let head = ResponseHead {
        id: ResponseId {
            request: id,
            source_generation: 5,
            serial: n,
        },
        logical_url: "http://example.test/post".into(),
        status: 200,
        headers: vec![],
        encoded_length: Some(body.len() as u64),
        has_body: true,
    };
    tasty_remote::html_http_body::verify(
        &begin,
        head,
        Arc::new(body.to_vec()),
        (identity(body), identity(body)),
        &BodyBudget::new(1_000_000),
        1_000_000,
        &AtomicBool::new(false),
    )
    .expect("actual verified representation")
}
fn delivered(body: Arc<VerifiedBody>, method: &str, upload: &[u8]) -> Delivery {
    Delivery {
        ticket: None,
        body,
        method: method.into(),
        upload: identity(upload),
    }
}
#[test]
fn native_main_post_consumes_its_exact_body_instead_of_the_prior_get_at_the_same_url() {
    let initial = response(1, "GET", b"", b"<p>initial</p>");
    let post = response(2, "POST", b"\0\xffupload", b"<script>changed()</script>");
    let deliveries = Deliveries::default();
    deliveries.pending.lock().unwrap().extend([
        delivered(initial, "GET", b""),
        delivered(post.clone(), "POST", b"\0\xffupload"),
    ]);
    let claimed = deliveries
        .select("http://example.test/post", "POST")
        .unwrap();
    assert!(Arc::ptr_eq(&claimed.body, &post));
    assert!(
        claimed
            .body
            .matches_native(post.head().id, "POST", identity(b"\0\xffupload"))
    );
    assert!(
        deliveries
            .select("http://example.test/post", "POST")
            .is_err()
    );
}
#[test]
fn ambiguous_native_deliveries_and_mismatched_uploads_never_publish_a_scan() {
    let a = response(1, "POST", b"a", b"<p>a</p>");
    let b = response(2, "POST", b"b", b"<script>b()</script>");
    let deliveries = Deliveries::default();
    deliveries.pending.lock().unwrap().extend([
        delivered(a.clone(), "POST", b"a"),
        delivered(b, "POST", b"b"),
    ]);
    assert!(
        deliveries
            .select("http://example.test/post", "POST")
            .is_err()
    );
    assert!(deliveries.main().is_none());
    deliveries.pending.lock().unwrap().clear();
    deliveries
        .pending
        .lock()
        .unwrap()
        .push(delivered(a, "POST", b"changed"));
    assert!(
        deliveries
            .select("http://example.test/post", "POST")
            .is_err()
    );
    assert!(deliveries.main().is_none());
}

#[test]
fn provisional_get_recovery_requires_actual_empty_get_and_no_published_main_body() {
    let deliveries = Deliveries::default();
    let url = "http://example.test/post";
    assert!(!deliveries.initial_get(url));
    *deliveries.first.lock().unwrap() = Some(InitialRequest {
        method: "POST".into(),
        url: url.into(),
        upload: identity(b""),
    });
    assert!(!deliveries.initial_get(url));
    *deliveries.first.lock().unwrap() = Some(InitialRequest {
        method: "GET".into(),
        url: url.into(),
        upload: identity(b"upload"),
    });
    assert!(!deliveries.initial_get(url));
    *deliveries.first.lock().unwrap() = Some(InitialRequest {
        method: "GET".into(),
        url: url.into(),
        upload: identity(b""),
    });
    assert!(deliveries.initial_get(url));
    assert!(!deliveries.initial_get("http://example.test/other"));
    *deliveries.main.lock().unwrap() = Some(response(1, "GET", b"", b"<p>committed</p>"));
    assert!(!deliveries.initial_get(url));
}
