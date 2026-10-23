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
