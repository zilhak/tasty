//! Authority preparation is independent of native HTTP and never authorizes a TLS exception.
use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_and_new_cross_origin_pins_are_ready_before_any_native_connection_or_http_request()
{
    let sentinel = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    sentinel.set_nonblocking(true).unwrap();
    let port = sentinel.local_addr().unwrap().port();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let handler: Handler = Arc::new(move |request| {
        seen.fetch_add(1, Ordering::AcqRel);
        Box::pin(async move { Ok(reply(request)) })
    });
    let mut settings = config("https://remote.example:8443/doc".into(), handler);
    settings.allow_remote_content = true;
    let budget = EndpointBudget::new();
    let certificates = budget.certificates();
    let endpoint = Endpoint::start(Handle::current(), settings, budget.clone())
        .await
        .unwrap_or_else(|_| panic!("endpoint"));
    assert!(endpoint.expected_leaf("remote.example", 8443).is_none());
    let work = endpoint
        .prepare_https("https://REMOTE.example:8443/doc?mode=1#section")
        .unwrap();
    assert!(endpoint.expected_leaf("remote.example", 8443).is_none());
    let first = Handle::current().spawn(work).await.unwrap().unwrap();
    assert_eq!(first.host(), "remote.example");
    assert_eq!(first.port(), 8443);
    assert!(first.is_current());
    assert_eq!(
        endpoint.expected_leaf(first.host(), first.port()),
        Some(first.leaf())
    );
    let same = endpoint
        .prepare_https("https://remote.example:8443/other")
        .unwrap()
        .await
        .unwrap();
    assert_eq!(same.leaf(), first.leaf());
    let other = endpoint
        .prepare_https(&format!("https://127.0.0.1:{port}/foreign"))
        .unwrap()
        .await
        .unwrap();
    assert!(other.is_current());
    assert_ne!(other.leaf(), first.leaf());
    assert_eq!(budget.active_connections(), 0);
    assert_eq!(calls.load(Ordering::Acquire), 0);
    assert_eq!(
        sentinel.accept().unwrap_err().kind(),
        std::io::ErrorKind::WouldBlock
    );
    endpoint.join().await.unwrap();
    assert!(!first.is_current());
    assert!(!same.is_current());
    assert!(!other.is_current());
    assert_eq!(budget.active_owners(), 0);
    assert!(certificates.reserved() > 0);
    drop((first, same, other));
    assert_eq!(certificates.reserved(), 0);
}

#[tokio::test]
async fn preparation_refuses_unnegotiated_https_foreign_policy_and_invalid_uri_before_generation() {
    let handler: Handler = Arc::new(|_| Box::pin(async { Err(EndpointError::Body) }));
    let budget = EndpointBudget::new();
    let mut settings = config("http://remote.example:8080/doc".into(), handler.clone());
    settings.https = false;
    let endpoint = Endpoint::start(Handle::current(), settings, budget.clone())
        .await
        .unwrap_or_else(|_| panic!("endpoint"));
    assert!(matches!(
        endpoint.prepare_https("https://remote.example:8080/doc"),
        Err(EndpointError::Destination)
    ));
    endpoint.join().await.unwrap();
    let endpoint = Endpoint::start(
        Handle::current(),
        config("https://remote.example:8443/doc".into(), handler),
        budget.clone(),
    )
    .await
    .unwrap_or_else(|_| panic!("endpoint"));
    assert!(matches!(
        endpoint.prepare_https("https://foreign.example:8443/"),
        Err(EndpointError::Destination)
    ));
    for uri in [
        "http://remote.example:8443/",
        "https://user:secret@remote.example:8443/",
        "not a URL",
    ] {
        assert!(matches!(
            endpoint.prepare_https(uri),
            Err(EndpointError::Invalid)
        ));
    }
    assert_eq!(budget.certificates().reserved(), 0);
    assert_eq!(budget.active_connections(), 0);
    endpoint.join().await.unwrap();
}

#[tokio::test]
async fn preparation_capacity_includes_unpolled_work_and_ready_receipts_until_actual_drop() {
    let handler: Handler = Arc::new(|_| Box::pin(async { Err(EndpointError::Body) }));
    let budget = EndpointBudget::new();
    let endpoint = Endpoint::start(
        Handle::current(),
        config("https://remote.example:8443/doc".into(), handler),
        budget.clone(),
    )
    .await
    .unwrap_or_else(|_| panic!("endpoint"));
    let mut pending = Vec::new();
    for _ in 0..8 {
        pending.push(
            endpoint
                .prepare_https("https://remote.example:8443/doc")
                .unwrap(),
        );
    }
    assert_eq!(budget.active_preparations(), 8);
    assert_eq!(budget.certificates().reserved(), 0);
    assert!(matches!(
        endpoint.prepare_https("https://remote.example:8443/doc"),
        Err(EndpointError::Capacity)
    ));
    let ready = pending.pop().unwrap().await.unwrap();
    assert!(ready.is_current());
    assert_eq!(budget.active_preparations(), 8);
    assert!(matches!(
        endpoint.prepare_https("https://remote.example:8443/doc"),
        Err(EndpointError::Capacity)
    ));
    endpoint.close();
    assert!(!ready.is_current());
    let replacement = Endpoint::start(
        Handle::current(),
        config(
            "https://remote.example:8443/doc".into(),
            Arc::new(|_| Box::pin(async { Err(EndpointError::Body) })),
        ),
        budget.clone(),
    )
    .await
    .unwrap_or_else(|_| panic!("replacement endpoint"));
    assert!(matches!(
        replacement.prepare_https("https://remote.example:8443/doc"),
        Err(EndpointError::Capacity)
    ));
    drop(ready);
    assert_eq!(budget.active_preparations(), 7);
    drop(
        replacement
            .prepare_https("https://remote.example:8443/doc")
            .unwrap(),
    );
    drop(pending);
    assert_eq!(budget.active_preparations(), 0);
    endpoint.join().await.unwrap();
    replacement.join().await.unwrap();
    assert_eq!(budget.certificates().reserved(), 0);
}

#[tokio::test]
async fn unpolled_or_retired_preparation_never_generates_and_new_document_gets_a_fresh_ready_pin() {
    let handler: Handler = Arc::new(|_| Box::pin(async { Err(EndpointError::Body) }));
    let budget = EndpointBudget::new();
    let endpoint = Endpoint::start(
        Handle::current(),
        config("https://remote.example:8443/doc".into(), handler.clone()),
        budget.clone(),
    )
    .await
    .unwrap_or_else(|_| panic!("endpoint"));
    drop(
        endpoint
            .prepare_https("https://remote.example:8443/doc")
            .unwrap(),
    );
    assert_eq!(budget.certificates().reserved(), 0);
    let ready = endpoint
        .prepare_https("https://remote.example:8443/doc")
        .unwrap()
        .await
        .unwrap();
    let work = endpoint
        .prepare_https("https://remote.example:8443/next")
        .unwrap();
    endpoint.close();
    assert!(!ready.is_current());
    assert!(matches!(work.await, Err(EndpointError::Retired)));
    assert!(matches!(
        endpoint.prepare_https("https://remote.example:8443/"),
        Err(EndpointError::Retired)
    ));
    endpoint.join().await.unwrap();
    let next = Endpoint::start(
        Handle::current(),
        config("https://remote.example:8443/doc".into(), handler),
        budget.clone(),
    )
    .await
    .unwrap_or_else(|_| panic!("replacement endpoint"));
    let replacement = next
        .prepare_https("https://remote.example:8443/doc")
        .unwrap()
        .await
        .unwrap();
    assert_ne!(ready.leaf(), replacement.leaf());
    assert!(!ready.is_current());
    assert!(replacement.is_current());
    next.join().await.unwrap();
    drop((ready, replacement));
    assert_eq!(budget.certificates().reserved(), 0);
    assert_eq!(budget.active_owners(), 0);
}
