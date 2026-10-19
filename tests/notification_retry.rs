//! Exercise the real GUI router and the headless handler using the same retry contract.
mod common;

use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;

fn keyed(tasty: &common::TastyInstance, method: &str, params: Value, key: &str, id: u64) -> Value {
    let mut stream = TcpStream::connect(("127.0.0.1", tasty.port())).unwrap();
    stream
        .set_read_timeout(Some(common::RESPONSE_WAIT))
        .unwrap();
    writeln!(
        stream,
        "{}",
        json!({
            "jsonrpc": "2.0", "method": method, "params": params,
            "id": id, "idempotency_key": key,
        })
    )
    .unwrap();
    let mut line = String::new();
    BufReader::new(stream).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

#[test]
fn notification_read_retry_replays_without_consuming_new_arrivals() {
    let tasty = common::shared();
    #[cfg(feature = "gui")]
    assert!(
        !tasty
            .call("window.list", json!({}))
            .as_array()
            .unwrap()
            .is_empty(),
        "this build must exercise the GUI routing path"
    );
    for scoped in [false, true] {
        check_retry(tasty, scoped);
    }
}

fn check_retry(tasty: &common::TastyInstance, scoped: bool) {
    let ws = tasty.create_workspace(if scoped {
        "scoped-retry"
    } else {
        "global-retry"
    });
    check_creation_retry(tasty, ws.id, scoped);

    let mut params = json!({"all":true});
    if scoped {
        params["workspace_id"] = json!(ws.id);
    }
    let key = format!("notification-read-{scoped}");
    let first = keyed(tasty, "notification.read", params.clone(), &key, 3);
    assert_eq!(first["result"], json!({"read":true,"changed":1}), "{first}");
    let again = keyed(tasty, "notification.read", params.clone(), &key, 4);
    assert_eq!(again["idempotent_replay"], true, "{again}");
    assert_eq!(again["id"], 4);
    assert_eq!(again["result"], first["result"]);

    tasty.call(
        "notification.create",
        json!({"workspace_id":ws.id,"level":"warning","title":"New arrival"}),
    );
    let after_arrival = keyed(tasty, "notification.read", params, &key, 5);
    assert_eq!(after_arrival["idempotent_replay"], true, "{after_arrival}");
    assert_eq!(after_arrival["result"], first["result"]);
    assert_eq!(
        tasty.call("notification.counts", json!({"workspace_id":ws.id}))["warning"],
        1,
        "a retry must not acknowledge the new arrival"
    );
    let conflict = keyed(
        tasty,
        "notification.read",
        json!({"all":true,"workspace_id":u32::MAX}),
        &key,
        6,
    );
    assert_eq!(
        conflict["error"]["code"],
        tasty_ipc::protocol::ERR_IDEMPOTENCY_KEY_CONFLICT,
        "{conflict}"
    );
    assert_eq!(
        tasty.call("notification.counts", json!({"workspace_id":ws.id}))["warning"],
        1
    );
    tracing::info!(
        "notification retry scoped={scoped}: first={first}, replay={again}, new-arrival replay={after_arrival}, conflict={conflict}"
    );
    tasty.call(
        "notification.read",
        json!({"all":true,"workspace_id":ws.id}),
    );
}

fn check_creation_retry(tasty: &common::TastyInstance, workspace_id: u64, scoped: bool) {
    let create_key = format!("notification-create-{scoped}");
    let create = json!({"workspace_id": workspace_id, "level": "error", "title": "First"});
    let first = keyed(tasty, "notification.create", create.clone(), &create_key, 1);
    let again = keyed(tasty, "notification.create", create, &create_key, 2);
    assert_eq!(
        again["idempotent_replay"], true,
        "creation positive control: {again}"
    );
    assert_eq!(first["result"], again["result"]);
    assert_eq!(
        tasty.call("notification.counts", json!({"workspace_id":workspace_id}))["error"],
        1
    );
}
