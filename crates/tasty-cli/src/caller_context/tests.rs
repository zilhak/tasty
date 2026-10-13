use super::*;
use serde_json::json;

const THREAD: &str = "019f55e7-3dfa-7c21-9a0b-2f6c4d1e8a90";

fn thread(home: Option<&str>) -> CodexCaller {
    codex_caller_from(Some(THREAD.into()), home.map(OsString::from))
}

fn params(value: Value) -> Map<String, Value> {
    value.as_object().expect("object").clone()
}

/// plugin 인자 그룹이 surface·caller_surface 를 둘 다 선언한 경우.
fn both(name: &str) -> bool {
    matches!(name, "surface" | "caller_surface")
}

#[test]
fn a_thread_id_is_read_only_in_the_codex_uuid_shape() {
    tasty_i18n::init("en");
    assert_eq!(codex_caller_from(None, None), CodexCaller::Absent);
    assert_eq!(
        codex_caller_from(Some("".into()), None),
        CodexCaller::Absent
    );
    assert_eq!(
        thread(None),
        CodexCaller::Thread {
            thread_id: THREAD.into(),
            codex_home: None
        }
    );
    for bad in [
        "surface-1297",
        "019f55e7",
        "019f55e7-3dfa-7c21-9a0b-2f6c4d1e8a9g",
    ] {
        let CodexCaller::Invalid(message) = codex_caller_from(Some(bad.into()), None) else {
            panic!("{bad} must not be accepted");
        };
        assert!(message.contains(bad), "{message}");
    }
}

#[test]
fn codex_home_is_carried_only_when_set_and_made_absolute() {
    assert_eq!(thread(Some("")), thread(None));
    let CodexCaller::Thread { codex_home, .. } = thread(Some("rel/codex")) else {
        panic!("relative CODEX_HOME must still yield a thread caller");
    };
    let home = codex_home.expect("home");
    assert!(std::path::Path::new(&home).is_absolute(), "{home}");
    assert!(
        home.ends_with("rel/codex") || home.ends_with("rel\\codex"),
        "{home}"
    );
}

#[test]
fn a_plain_shell_keeps_the_env_surface_fill_and_marks_it_implicit() {
    let mut p = params(json!({}));
    apply_to_plugin_params(&mut p, both, &CodexCaller::Absent, Some("42")).unwrap();
    assert_eq!(p["surface"], 42);
    assert_eq!(p["surface_id"], 42);
    assert_eq!(p["caller_surface"], 42);
    assert_eq!(
        p[CONTEXT_KEY],
        json!({ "implicit": ["surface", "surface_id", "caller_surface"] })
    );

    // 인자를 선언하지 않았고 채운 것도 없으면 예약 키를 붙이지 않는다.
    let mut p = params(json!({ "text": "x" }));
    apply_to_plugin_params(&mut p, |_| false, &CodexCaller::Absent, Some("42")).unwrap();
    assert_eq!(p, params(json!({ "text": "x" })));

    // 환경 값이 없으면 기존처럼 아무것도 채우지 않는다.
    let mut p = params(json!({}));
    apply_to_plugin_params(&mut p, both, &CodexCaller::Absent, None).unwrap();
    assert!(p.is_empty(), "{p:?}");
}

#[test]
fn an_explicit_surface_is_kept_and_not_reported_as_implicit() {
    for codex in [CodexCaller::Absent, thread(None)] {
        let mut p = params(json!({ "surface": 7 }));
        apply_to_plugin_params(&mut p, |n| n == "surface", &codex, Some("42")).unwrap();
        assert_eq!(p["surface"], 7, "{codex:?}");
        assert_eq!(p["surface_id"], 7, "{codex:?}");
        let implicit = p.get(CONTEXT_KEY).and_then(|c| c.get("implicit"));
        assert!(implicit.is_none(), "{codex:?}: {p:?}");
    }
}

#[test]
fn a_codex_thread_with_a_stale_env_surface_gets_no_surface_fill() {
    let mut p = params(json!({ "child": 3 }));
    apply_to_plugin_params(&mut p, both, &thread(Some("/codex-home")), Some("42")).unwrap();
    assert!(!p.contains_key("surface"), "{p:?}");
    assert!(!p.contains_key("surface_id"), "{p:?}");
    assert!(!p.contains_key("caller_surface"), "{p:?}");
    assert_eq!(p["child"], 3);
    assert_eq!(
        p[CONTEXT_KEY],
        json!({
            "provider": "codex",
            "thread_id": THREAD,
            "codex_home": std::path::absolute("/codex-home").unwrap().to_str().unwrap(),
        })
    );
}

#[test]
fn an_invalid_thread_refuses_instead_of_using_the_env_surface() {
    tasty_i18n::init("en");
    let codex = codex_caller_from(Some("not-a-thread".into()), None);
    let mut p = params(json!({}));
    let err = apply_to_plugin_params(&mut p, both, &codex, Some("42")).unwrap_err();
    assert!(err.contains("not-a-thread"), "{err}");
    assert!(p.is_empty(), "{p:?}");
    assert!(env_surface(&codex, Some("42")).is_err());
}

#[test]
fn the_reserved_key_cannot_come_from_arguments() {
    tasty_i18n::init("en");
    let mut p = params(json!({ CONTEXT_KEY: { "provider": "codex" } }));
    let err = apply_to_plugin_params(&mut p, both, &CodexCaller::Absent, None).unwrap_err();
    assert!(err.contains(CONTEXT_KEY), "{err}");
}
