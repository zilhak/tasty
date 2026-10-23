//! Closed HTTP boundary until native authority, body and cache fences are integrated.
use super::*;
pub(super) fn refuse_http(
    hub: &StreamHub,
    client: u32,
    request: tasty_ipc::html_http::HttpRequest,
) {
    let Some(binding) = hub.client_binding(client) else {
        return;
    };
    let reply = tasty_ipc::html_http::HttpReply::Failure {
        id: request.id(),
        source_generation: request.source_generation(),
        response: None,
        code: tasty_ipc::html_http::FailureCode::UnsupportedScheme,
        reason: "Remote HTTP native integration is unavailable".into(),
    };
    match reply.frame() {
        Ok(frame) => {
            if hub.push_bound(client, &binding, frame) != PushResult::Sent {
                hub.unregister_bound(client, &binding);
            }
        }
        Err(error) => {
            tracing::warn!(%error, "HTTP refusal encoding failed");
            hub.unregister_bound(client, &binding);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_http_boundary_returns_identity_without_contacting_destination() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let hub = StreamHub::new();
        let rx = hub.register(5);
        let id = TransferId {
            epoch: 1,
            surface_id: 2,
            binding: 3,
            document: 4,
            request: 5,
            transfer: 6,
        };
        refuse_http(
            &hub,
            5,
            tasty_ipc::html_http::HttpRequest::Begin {
                id,
                source_generation: 7,
                method: "POST".into(),
                logical_url: format!("http://{}/", listener.local_addr().unwrap()),
                headers: vec![],
                upload: tasty_ipc::html_http::BodyIdentity {
                    bytes: 0,
                    sha256: [0; 32],
                },
                allow_remote_content: false,
            },
        );
        let frame = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let reply: tasty_ipc::html_http::HttpReply =
            serde_json::from_slice(&frame.payload).unwrap();
        assert!(
            matches!(reply, tasty_ipc::html_http::HttpReply::Failure { id: actual, source_generation: 7, code: tasty_ipc::html_http::FailureCode::UnsupportedScheme, .. } if actual == id)
        );
        assert_eq!(
            listener.accept().unwrap_err().kind(),
            std::io::ErrorKind::WouldBlock
        );
    }
}
