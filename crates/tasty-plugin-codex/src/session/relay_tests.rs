use super::*;
use crate::rpc::Client;
use crate::ws::test_server;

fn run_reply(approval: &str, reject_bind: bool) -> (Result<Value, crate::rpc::RpcError>, usize) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let approval = approval.to_owned();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let head = test_server::accept_handshake(&mut socket);
        assert!(head.contains("Authorization: Bearer backend-secret"));
        let init: Value =
            serde_json::from_str(&test_server::read_text(&mut socket).unwrap()).unwrap();
        test_server::write_text(
            &mut socket,
            &json!({"id":init["id"],"result":{}}).to_string(),
        );
        let initialized: Value =
            serde_json::from_str(&test_server::read_text(&mut socket).unwrap()).unwrap();
        assert_eq!(initialized["method"], "initialized");
        let request: Value =
            serde_json::from_str(&test_server::read_text(&mut socket).unwrap()).unwrap();
        assert_eq!(request["method"], "thread/resume");
        assert_eq!(request["params"]["threadId"], "selected-by-picker");
        assert_eq!(request["params"]["approvalPolicy"], "never");
        assert_eq!(request["params"]["sandbox"], "read-only");
        assert_eq!(request["params"]["custom"], "preserved");
        test_server::write_text(&mut socket, &json!({"id":request["id"],"result":{"thread":{"id":"selected-by-picker"},"approvalPolicy":approval,"sandbox":{"type":"readOnly"}}}).to_string());
        // Keep the backend open until the relay/client closes, including error responses.
        test_server::read_text(&mut socket);
    });
    let called = Arc::new(Mutex::new(0));
    let calls = called.clone();
    let relay = Relay::start(
        Endpoint::Ws {
            port,
            token: "backend-secret".into(),
        },
        ResumeSettings {
            approval: Some("never".into()),
            sandbox: Some("read-only".into()),
            model: None,
        },
        Arc::new(move |result| {
            assert_eq!(result["thread"]["id"], "selected-by-picker");
            *calls.lock().unwrap() += 1;
            if reject_bind {
                Err("stale generation".into())
            } else {
                Ok(())
            }
        }),
    )
    .unwrap();
    let port = relay.url.rsplit(':').next().unwrap().parse().unwrap();
    let mut client = Client::connect(
        &Endpoint::Ws {
            port,
            token: relay.token.clone(),
        },
        Duration::from_secs(3),
    )
    .unwrap();
    let result = client.call(
        "thread/resume",
        json!({"threadId":"selected-by-picker","custom":"preserved"}),
    );
    drop(client);
    drop(relay);
    server.join().unwrap();
    let count = *called.lock().unwrap();
    (result, count)
}

#[test]
fn selected_thread_is_registered_before_success_reaches_the_tui() {
    let (reply, calls) = run_reply("never", false);
    assert_eq!(reply.unwrap()["thread"]["id"], "selected-by-picker");
    assert_eq!(calls, 1);
}
#[test]
fn wrong_permissions_or_failed_registration_are_not_reported_as_success() {
    let (reply, calls) = run_reply("on-request", false);
    assert!(reply.unwrap_err().to_string().contains("requested never"));
    assert_eq!(calls, 0);
    let (reply, calls) = run_reply("never", true);
    assert!(reply.unwrap_err().to_string().contains("stale generation"));
    assert_eq!(calls, 1);
}
#[test]
fn unauthenticated_connections_never_reach_the_backend() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    listener.set_nonblocking(true).unwrap();
    let relay = Relay::start(
        Endpoint::Ws {
            port: listener.local_addr().unwrap().port(),
            token: "secret".into(),
        },
        ResumeSettings::default(),
        Arc::new(|_| panic!("must not bind")),
    )
    .unwrap();
    let port: u16 = relay.url.rsplit(':').next().unwrap().parse().unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .unwrap();
    stream
        .write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n")
        .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.contains("401"));
    assert_eq!(
        listener.accept().unwrap_err().kind(),
        io::ErrorKind::WouldBlock
    );
}

#[test]
fn later_explicit_permission_choices_are_not_replaced_by_launch_defaults() {
    let mut params =
        json!({"approvalPolicy":"on-request","sandbox":"read-only","model":"chosen-model"})
            .as_object()
            .unwrap()
            .clone();
    let original = params.clone();
    let effective = apply_settings(
        &mut params,
        &ResumeSettings {
            approval: Some("never".into()),
            sandbox: Some("danger-full-access".into()),
            model: Some("launch-model".into()),
        },
    );
    assert_eq!(params, original);
    assert_eq!(effective.approval.as_deref(), Some("on-request"));
    assert_eq!(effective.sandbox.as_deref(), Some("read-only"));
    assert_eq!(effective.model.as_deref(), Some("chosen-model"));
}

#[test]
fn explicit_permission_profile_config_is_not_combined_with_launch_bypass() {
    let mut params = json!({"config":{"default_permissions":"read-only","model":"profile-model"}})
        .as_object()
        .unwrap()
        .clone();
    let original = params.clone();
    let effective = apply_settings(
        &mut params,
        &ResumeSettings {
            approval: Some("never".into()),
            sandbox: Some("danger-full-access".into()),
            model: Some("launch-model".into()),
        },
    );
    assert_eq!(params, original);
    assert_eq!(effective, ResumeSettings::default());
}
