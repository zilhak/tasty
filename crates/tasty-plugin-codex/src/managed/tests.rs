use super::*;
use std::cell::RefCell;

struct Host {
    instance: &'static str,
    calls: RefCell<Vec<String>>,
}
impl HostCall for Host {
    fn call(
        &self,
        method: &str,
        _: Value,
    ) -> std::result::Result<Value, tasty_plugin_sdk::PluginError> {
        self.calls.borrow_mut().push(method.into());
        assert_eq!(
            method, "system.info",
            "metadata helper must never read tasks with plugin authority"
        );
        Ok(json!({"instance_id":self.instance}))
    }
}
fn host() -> Host {
    Host {
        instance: "owned-instance",
        calls: RefCell::new(Vec::new()),
    }
}
fn params(root: &Path) -> Value {
    let exe = root.join("tasty executable");
    let port = root.join("port file");
    std::fs::write(&exe, "").unwrap();
    std::fs::write(&port, "1234").unwrap();
    json!({"action":"prepare","workspace_id":514,"task_id":"task with 'quotes' ; $(false)",
        "parent_thread":"11111111-1111-1111-1111-111111111111","expected_auth_mode":"local",
        "cli_path":exe,"cli_home":root,"port_file":port})
}
#[test]
fn lifecycle_is_metadata_only_and_attach_is_idempotent() {
    let _home = tasty_test_support::IsolatedHome::new();
    let root_path = tasty_utils::path::tasty_home().unwrap();
    let root = root_path.as_path();
    let host = host();
    let prepared = execute(&host, root, &params(root)).unwrap();
    assert_eq!(prepared["phase"], "prepared");
    assert_eq!(prepared["task_verified"], false);
    assert!(
        prepared["create_thread_args"]["prompt"]
            .as_str()
            .unwrap()
            .len()
            <= 1000
    );
    assert!(prepared["create_thread_args"].get("model").is_none());
    let id = prepared["relay_id"].as_str().unwrap();
    assert!(execute(&host, root, &json!({"action":"callback","id":id})).is_err());
    let attach =
        json!({"action":"attach","id":id,"thread_id":"22222222-2222-2222-2222-222222222222"});
    let attached = execute(&host, root, &attach).unwrap();
    assert_eq!(attached["phase"], "attached");
    assert_eq!(execute(&host, root, &attach).unwrap(), attached);
    assert!(
        execute(
            &host,
            root,
            &json!({"action":"attach","id":id,"thread_id":"33333333-3333-3333-3333-333333333333"})
        )
        .is_err()
    );
    let callback = execute(&host, root, &json!({"action":"callback","id":id})).unwrap();
    assert_eq!(callback["sent"], false);
    assert_eq!(callback["task_verified"], false);
    assert!(
        callback["callback"]["arguments"]["prompt"]
            .as_str()
            .unwrap()
            .len()
            <= 1000
    );
    assert_eq!(
        callback["callback"]["arguments"]["threadId"],
        "11111111-1111-1111-1111-111111111111"
    );
    assert_eq!(
        attached["task_get_argv"][8],
        "task with 'quotes' ; $(false)"
    );
    assert!(
        attached["instructions"]
            .as_str()
            .unwrap()
            .contains("never unset")
    );
    assert_eq!(
        execute(&host, root, &json!({"action":"forget","id":id})).unwrap()["task_cancelled"],
        false
    );
    assert!(execute(&host, root, &json!({"action":"get","id":id})).is_err());
}
#[test]
fn invalid_identity_restart_and_traversal_fail_closed() {
    let _home = tasty_test_support::IsolatedHome::new();
    let root_path = tasty_utils::path::tasty_home().unwrap();
    let root = root_path.as_path();
    let host = host();
    let mut input = params(root);
    input["parent_thread"] = json!("1297");
    assert!(execute(&host, root, &input).is_err());
    input = params(root);
    input["expected_auth_mode"] = json!("agent");
    assert!(execute(&host, root, &input).is_err());
    assert!(store::path(root, "../../escape").is_err());
    let id = execute(&host, root, &params(root)).unwrap()["relay_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let other = Host {
        instance: "replacement-instance",
        calls: RefCell::new(Vec::new()),
    };
    assert!(
        execute(&other, root, &json!({"action":"get","id":id}))
            .unwrap_err()
            .to_string()
            .contains("instance_changed")
    );
}
#[test]
fn byte_limit_is_enforced_before_record_write() {
    let _home = tasty_test_support::IsolatedHome::new();
    let root_path = tasty_utils::path::tasty_home().unwrap();
    let root = root_path.as_path();
    let mut record = store::Record {
        relay_id: "relay-test".into(),
        parent_thread: "11111111-1111-1111-1111-111111111111".into(),
        workspace_id: 1,
        task_id: "task".into(),
        cli_path: "/tasty".into(),
        cli_home: "/home".into(),
        port_file: "/port".into(),
        instance_id: "instance".into(),
        relay_thread: None,
    };
    record.cli_path = format!(
        "/home/user/workspace/{}/target/debug/tasty",
        "long-worktree-name-".repeat(7)
    );
    record.port_file = format!(
        "/home/user/workspace/{}/isolated/tasty.port",
        "long-session-name-".repeat(7)
    );
    assert!(
        prompt::create(&record).unwrap()["prompt"]
            .as_str()
            .unwrap()
            .len()
            <= 1000
    );
    record.cli_path = "한".repeat(1000);
    assert!(prompt::create(&record).is_err());
    assert!(!store::path(root, &record.relay_id).unwrap().exists());
}
