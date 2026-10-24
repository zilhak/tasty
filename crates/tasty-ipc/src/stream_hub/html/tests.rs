use super::*;
use crate::{html_http, html_transfer, stream::StreamTag};

fn id() -> html_transfer::TransferId {
    html_transfer::TransferId {
        epoch: 1,
        surface_id: 2,
        binding: 3,
        document: 4,
        request: 5,
        transfer: 6,
    }
}
fn begin() -> HttpRequest {
    HttpRequest::Begin {
        id: id(),
        source_generation: 7,
        method: "POST".into(),
        logical_url: "http://example.test/".into(),
        headers: vec![],
        upload: html_http::BodyIdentity {
            bytes: 0,
            sha256: [0; 32],
        },
        allow_remote_content: false,
    }
}
fn send(tx: &Sender<StreamInbound>, value: &impl serde::Serialize) {
    tx.send(StreamInbound::Frame {
        client_id: 5,
        frame: StreamFrame::new(StreamTag::Control, serde_json::to_vec(value).unwrap()),
    })
    .unwrap();
}
fn capabilities(tx: &Sender<StreamInbound>, values: &[&str]) {
    send(
        tx,
        &HtmlRequest::HtmlCapabilities {
            capabilities: values.iter().map(|s| (*s).into()).collect(),
        },
    );
}
#[test]
fn file_http_upload_credit_cancel_stay_in_one_wire_order() {
    let hub = StreamHub::new();
    let _sink = hub.register(5);
    let (tx, rx) = mpsc::channel();
    capabilities(&tx, &[html_transfer::CAPABILITY, html_http::CAPABILITY]);
    send(&tx, &HtmlRequest::HtmlEntryRequest { id: id() });
    send(&tx, &begin());
    send(
        &tx,
        &HttpRequest::Upload {
            id: id(),
            source_generation: 7,
            offset: 0,
            data_b64: "YQ==".into(),
        },
    );
    send(
        &tx,
        &HtmlRequest::HtmlRead {
            id: id(),
            offset: 0,
            allow: false,
        },
    );
    send(
        &tx,
        &HttpRequest::Commit {
            id: id(),
            source_generation: 7,
        },
    );
    send(
        &tx,
        &HttpRequest::Cancel {
            id: id(),
            source_generation: 7,
        },
    );
    send(&tx, &HtmlRequest::HtmlCancel { id: id() });
    let commands = hub.pump_inbound(&rx).html_requests;
    assert_eq!(commands.len(), 8);
    assert!(matches!(
        commands[0].1,
        HtmlControl::File(HtmlRequest::HtmlCapabilities { .. })
    ));
    assert!(matches!(
        commands[1].1,
        HtmlControl::File(HtmlRequest::HtmlEntryRequest { .. })
    ));
    assert!(matches!(
        commands[2].1,
        HtmlControl::Http(HttpRequest::Begin { .. })
    ));
    assert!(matches!(
        commands[3].1,
        HtmlControl::Http(HttpRequest::Upload { .. })
    ));
    assert!(matches!(
        commands[4].1,
        HtmlControl::File(HtmlRequest::HtmlRead { .. })
    ));
    assert!(matches!(
        commands[5].1,
        HtmlControl::Http(HttpRequest::Commit { .. })
    ));
    assert!(matches!(
        commands[6].1,
        HtmlControl::Http(HttpRequest::Cancel { .. })
    ));
    assert!(matches!(
        commands[7].1,
        HtmlControl::File(HtmlRequest::HtmlCancel { .. })
    ));
}
#[test]
fn declarations_malformed_commands_and_replaced_registration_fail_closed() {
    let hub = StreamHub::new();
    let _sink = hub.register(5);
    let (tx, rx) = mpsc::channel();
    for caps in [
        vec![],
        vec![html_transfer::CAPABILITY],
        vec![html_http::CAPABILITY],
    ] {
        capabilities(&tx, &caps);
        send(&tx, &begin());
        let out = hub.pump_inbound(&rx);
        assert_eq!(out.html_requests.len(), 1);
    }
    capabilities(&tx, &[html_transfer::CAPABILITY, html_http::CAPABILITY]);
    let mut bad = begin();
    if let HttpRequest::Begin { method, .. } = &mut bad {
        *method = "bad method".into();
    }
    send(&tx, &bad);
    send(
        &tx,
        &HttpRequest::Upload {
            id: id(),
            source_generation: 7,
            offset: 0,
            data_b64: "YR==".into(),
        },
    );
    assert_eq!(hub.pump_inbound(&rx).html_requests.len(), 1);
    let _replacement = hub.register(5);
    send(&tx, &begin());
    assert!(hub.pump_inbound(&rx).html_requests.is_empty());
}

#[test]
fn both_identity_counters_increase_and_rejected_claims_change_neither() {
    let hub = StreamHub::new();
    let _rx = hub.register(5);
    let binding = hub.client_binding(5).unwrap();
    let next = |request, transfer| html_transfer::TransferId {
        request,
        transfer,
        ..id()
    };
    assert!(hub.admit_html_request(5, &binding, next(1, 1)));
    for (request, transfer) in [(1, 2), (2, 1), (100, 1), (1, 100)] {
        assert!(!hub.admit_html_request(5, &binding, next(request, transfer)));
    }
    assert!(hub.admit_html_request(5, &binding, next(2, 2)));
    assert!(!hub.admit_html_request(5, &binding, next(2, 2)));
    assert!(hub.admit_html_request(5, &binding, next(3, 3)));
}

#[test]
fn only_current_registration_can_reset_and_advance_the_cursor() {
    let hub = StreamHub::new();
    let _old_rx = hub.register(5);
    let old = hub.client_binding(5).unwrap();
    assert!(hub.admit_html_request(5, &old, id()));
    let _new_rx = hub.register(5);
    let new = hub.client_binding(5).unwrap();
    let low = html_transfer::TransferId {
        request: 1,
        transfer: 1,
        ..id()
    };
    assert!(!hub.admit_html_request(5, &old, low));
    assert!(hub.admit_html_request(5, &new, low));
    assert!(!hub.admit_html_request(5, &old, id()));
    assert!(!hub.admit_html_request(5, &new, low));
    hub.unregister(5);
    assert!(!hub.admit_html_request(5, &new, id()));
}

#[test]
fn ordered_file_and_http_admissions_use_one_registration_cursor() {
    let hub = StreamHub::new();
    let _sink = hub.register(5);
    let binding = hub.client_binding(5).unwrap();
    let (tx, rx) = mpsc::channel();
    capabilities(&tx, &[html_transfer::CAPABILITY, html_http::CAPABILITY]);
    let next = |request, transfer| html_transfer::TransferId {
        request,
        transfer,
        ..id()
    };
    send(&tx, &HtmlRequest::HtmlEntryRequest { id: next(1, 1) });
    for (request, transfer) in [(1, 2), (2, 2)] {
        let mut msg = begin();
        if let HttpRequest::Begin { id, .. } = &mut msg {
            *id = next(request, transfer);
        }
        send(&tx, &msg);
    }
    send(
        &tx,
        &HtmlRequest::HtmlRead {
            id: next(100, 100),
            offset: 0,
            allow: false,
        },
    );
    send(&tx, &HtmlRequest::HtmlEntryRequest { id: next(3, 2) });
    send(&tx, &HtmlRequest::HtmlEntryRequest { id: next(3, 3) });
    let claims: Vec<_> = hub
        .pump_inbound(&rx)
        .html_requests
        .into_iter()
        .filter_map(|(client, msg)| {
            let id = match msg {
                HtmlControl::File(HtmlRequest::HtmlEntryRequest { id })
                | HtmlControl::Http(HttpRequest::Begin { id, .. }) => id,
                _ => return None,
            };
            Some(hub.admit_html_request(client, &binding, id))
        })
        .collect();
    assert_eq!(claims, [true, false, true, false, true]);
}

#[test]
fn http_and_https_require_the_current_peer_native_declarations() {
    let hub = StreamHub::new();
    let _sink = hub.register(5);
    let (tx, rx) = mpsc::channel();
    capabilities(&tx, &[html_transfer::CAPABILITY, html_http::CAPABILITY]);
    hub.pump_inbound(&rx);
    assert!(!hub.html_http_enabled(5, false));
    capabilities(
        &tx,
        &[
            html_transfer::CAPABILITY,
            html_transfer::VIEWER_CAPABILITY,
            html_http::CAPABILITY,
        ],
    );
    hub.pump_inbound(&rx);
    assert!(hub.html_http_enabled(5, false));
    assert!(!hub.html_http_enabled(5, true));
    capabilities(
        &tx,
        &[
            html_transfer::CAPABILITY,
            html_transfer::VIEWER_CAPABILITY,
            html_http::CAPABILITY,
            html_http::HTTPS_CAPABILITY,
        ],
    );
    hub.pump_inbound(&rx);
    assert!(hub.html_http_enabled(5, true));
    let _replacement = hub.register(5);
    assert!(!hub.html_http_enabled(5, false));
    assert!(!hub.html_http_enabled(5, true));
}
