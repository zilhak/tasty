use super::{tests::*, *};
use crate::html_http_native::NativeBudget;
use tasty_ipc::html_http::HttpRequest;
fn config(bridge: &HttpBridge, document: u64) -> BridgeConfig {
    let mut scope = bridge.config.scope.clone();
    scope.document = document;
    BridgeConfig {
        scope,
        limits: bridge.config.limits,
        pending: bridge.config.pending.clone(),
        bodies: bridge.config.bodies.clone(),
    }
}
async fn delivered(
    bridge: &Arc<HttpBridge>,
    rx: &std::sync::mpsc::Receiver<crate::connection::QueuedFrame>,
) -> (NativeResponse, NativeBudget) {
    let (request, budget) = native(b"\0opaque\xff").await;
    let wait = bridge.submit(request).unwrap();
    let mut sequence = 0;
    drive(bridge, &mut sequence);
    command(rx);
    bridge.receive(HttpReply::UploadReady {
        id: id(1),
        source_generation: 9,
        offset: 0,
        max_bytes: 100,
    });
    drive(bridge, &mut sequence);
    command(rx);
    command(rx);
    complete(bridge, id(1));
    drive(bridge, &mut sequence);
    (wait.await.unwrap(), budget)
}
#[tokio::test]
async fn receipt_capture_does_not_retire_delivery_and_retired_successor_preserves_owned_post() {
    let (bridge, rx) = bridge();
    let (response, _budget) = delivered(&bridge, &rx).await;
    let native = Arc::new(());
    let receipt = bridge
        .capture_reload(&response, &native, Arc::new(|| true))
        .unwrap();
    assert!(bridge.current());
    assert!(rx.try_recv().is_err());
    assert!(
        bridge
            .capture_reload(&response, &native, Arc::new(|| true))
            .is_err()
    );
    let receipt = receipt.retire().unwrap();
    assert!(!bridge.current());
    assert!(rx.try_recv().is_err());
    let next = HttpBridge::new_successor(
        config(&bridge, 3),
        bridge.sender.clone(),
        Arc::new(|| true),
        Arc::new(|| {}),
        receipt,
        &native,
        true,
    )
    .unwrap();
    let next_request = response.request.clone();
    let wait = next.submit(next_request.clone()).unwrap();
    let mut sequence = 1;
    next.drive(
        || {
            sequence += 1;
            let mut id = id(sequence);
            id.document = 3;
            Some(id)
        },
        Err,
    );
    let reload = command(&rx);
    assert!(matches!(reload,HttpRequest::Reload{previous,..}if previous==response.body.head().id));
    let (begin, _) = reload.into_initial();
    let HttpRequest::Begin {
        method,
        upload,
        headers,
        ..
    } = begin
    else {
        panic!("begin")
    };
    assert_eq!(method, "POST");
    assert_eq!(upload, next_request.identity);
    assert!(headers == next_request.headers);
    assert_eq!(next.snapshot().received, 0);
    assert_eq!(next.snapshot().skipped, 0);
    assert!(!next.snapshot().paused);
    drop(wait);
    next.close();
}
#[tokio::test]
async fn stale_source_replaced_native_connection_and_unnegotiated_peer_fail_closed() {
    for condition in 0..5 {
        let (bridge, rx) = bridge();
        let (response, _budget) = delivered(&bridge, &rx).await;
        let owner = Arc::new(());
        let live = Arc::new(AtomicBool::new(true));
        let check = live.clone();
        let receipt = bridge
            .capture_reload(
                &response,
                &owner,
                Arc::new(move || check.load(Ordering::Acquire)),
            )
            .unwrap();
        if condition == 0 {
            live.store(false, Ordering::Release);
            assert!(receipt.retire().is_err());
            assert!(bridge.current());
            continue;
        }
        let receipt = receipt.retire().unwrap();
        let native = if condition == 1 { Arc::new(()) } else { owner };
        let mut scope = config(&bridge, 3);
        if condition == 2 {
            scope.scope.source_generation += 1;
        }
        let sender = if condition == 3 {
            crate::connection::channel().0
        } else {
            bridge.sender.clone()
        };
        assert!(
            HttpBridge::new_successor(
                scope,
                sender,
                Arc::new(|| true),
                Arc::new(|| {}),
                receipt,
                &native,
                condition != 4
            )
            .is_err()
        );
        assert!(matches!(command(&rx),HttpRequest::Close{entry,..}if entry==id(1)));
        assert!(rx.try_recv().is_err());
    }
}
#[tokio::test]
async fn receipt_drop_before_reload_keeps_document_but_abandon_after_retire_closes_exact_old() {
    let (bridge, rx) = bridge();
    let (response, _budget) = delivered(&bridge, &rx).await;
    let native = Arc::new(());
    drop(
        bridge
            .capture_reload(&response, &native, Arc::new(|| true))
            .unwrap(),
    );
    assert!(bridge.current());
    assert!(rx.try_recv().is_err());
    let receipt = bridge
        .capture_reload(&response, &native, Arc::new(|| true))
        .unwrap()
        .retire()
        .unwrap();
    let next = HttpBridge::new_successor(
        config(&bridge, 3),
        bridge.sender.clone(),
        Arc::new(|| true),
        Arc::new(|| {}),
        receipt,
        &native,
        true,
    )
    .unwrap();
    next.close();
    assert!(matches!(command(&rx),HttpRequest::Close{entry,..}if entry==id(1)));
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn only_valid_initial_upload_ready_retires_predecessor_cleanup_obligation() {
    for mode in 0..3 {
        let (bridge, rx) = bridge();
        let (response, _budget) = delivered(&bridge, &rx).await;
        let owner = Arc::new(());
        let receipt = bridge
            .capture_reload(&response, &owner, Arc::new(|| true))
            .unwrap()
            .retire()
            .unwrap();
        let next = HttpBridge::new_successor(
            config(&bridge, 3),
            bridge.sender.clone(),
            Arc::new(|| true),
            Arc::new(|| {}),
            receipt,
            &owner,
            true,
        )
        .unwrap();
        let wait = next.submit(response.request.clone()).unwrap();
        let mut new = id(2);
        new.document = 3;
        next.drive(|| Some(new), Err);
        assert!(matches!(command(&rx), HttpRequest::Reload { .. }));
        if mode == 0 {
            assert!(next.receive(HttpReply::CommandRejected {
                id: new,
                source_generation: 9,
                command: tasty_ipc::html_http::CommandKind::Begin,
                response: None,
                code: tasty_ipc::html_http::FailureCode::Stale,
                reason: "Stale".into()
            }));
        } else {
            assert!(next.receive(HttpReply::UploadReady {
                id: new,
                source_generation: 9,
                offset: 0,
                max_bytes: if mode == 1 { 0 } else { 32 }
            }));
        }
        if mode < 2 {
            assert!(matches!(command(&rx),HttpRequest::Close{entry,..}if entry==id(1)));
            next.drive(|| panic!("no retry"), Err);
            assert!(wait.await.is_err());
        } else {
            assert!(rx.try_recv().is_err());
            next.close();
            assert!(matches!(command(&rx),HttpRequest::Close{entry,..}if entry==new));
            assert!(rx.try_recv().is_err());
            drop(wait);
        }
    }
}
