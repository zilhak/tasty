//! workspace.create·update가 입력 오류로 거절할 때 상태를 하나도 바꾸지 않는지 확인한다.
//! 오류 응답은 멱등 키와 함께 저장되므로, 일부만 적용된 변경은 재시도로도 회수할 수 없다.

use serde_json::json;

use super::{handle_workspace_create, handle_workspace_update};

fn names(engine: &crate::core::CoreState) -> Vec<String> {
    engine
        .workspaces()
        .into_iter()
        .map(|w| w.name.clone())
        .collect()
}

#[test]
fn create_with_an_unknown_category_creates_nothing() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let mut core = crate::ipc::handler::cli_entry_tests::test_core();
    let before = names(&engine);

    let res = handle_workspace_create(
        &mut core,
        &mut state,
        &mut engine,
        json!(1),
        &json!({ "name": "partial-create", "category": "no-such-category" }),
    );

    let err = res.error.expect("없는 카테고리는 거절해야 한다");
    assert!(err.message.contains("unknown category"), "{}", err.message);
    assert_eq!(names(&engine), before, "거절했는데 workspace가 생겼다");
}

#[test]
fn create_with_a_malformed_attach_mapping_creates_nothing() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let mut core = crate::ipc::handler::cli_entry_tests::test_core();
    let before = names(&engine);

    let res = handle_workspace_create(
        &mut core,
        &mut state,
        &mut engine,
        json!(1),
        &json!({
            "name": "partial-create",
            "attach_profile": "p",
            "attach_remote_workspace": "not-a-number",
        }),
    );

    assert!(
        res.error.is_some(),
        "잘못된 원격 workspace 값은 거절해야 한다"
    );
    assert_eq!(names(&engine), before, "거절했는데 workspace가 생겼다");
}

#[test]
fn update_with_an_unknown_category_keeps_the_name() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let mut core = crate::ipc::handler::cli_entry_tests::test_core();
    let target = engine.workspace_at(0).expect("workspace index is valid").id;
    let before = names(&engine);

    let res = handle_workspace_update(
        &mut core,
        &mut state,
        &mut engine,
        json!(1),
        &json!({ "id": target, "name": "renamed", "category": "no-such-category" }),
    );

    assert!(res.error.is_some(), "없는 카테고리는 거절해야 한다");
    assert_eq!(names(&engine), before, "거절했는데 이름이 바뀌었다");
}

#[test]
fn update_with_a_malformed_attach_mapping_keeps_the_name_and_category() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let mut core = crate::ipc::handler::cli_entry_tests::test_core();
    let target = engine.workspace_at(0).expect("workspace index is valid").id;
    let category_before = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .category;
    let other = engine.create_category("other").expect("카테고리 생성");
    let before = names(&engine);

    let res = handle_workspace_update(
        &mut core,
        &mut state,
        &mut engine,
        json!(1),
        &json!({
            "id": target,
            "name": "renamed",
            "category": other,
            "attach_profile": "p",
            "attach_remote_workspace": "not-a-number",
        }),
    );

    assert!(
        res.error.is_some(),
        "잘못된 원격 workspace 값은 거절해야 한다"
    );
    assert_eq!(names(&engine), before, "거절했는데 이름이 바뀌었다");
    assert_eq!(
        engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .category,
        category_before,
        "거절했는데 카테고리가 바뀌었다"
    );
    assert_eq!(
        engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .attach_mapping,
        None
    );
}

#[test]
fn attach_clear_still_ignores_a_malformed_remote_workspace() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let mut core = crate::ipc::handler::cli_entry_tests::test_core();
    let target = engine.workspace_at(0).expect("workspace index is valid").id;

    let res = handle_workspace_update(
        &mut core,
        &mut state,
        &mut engine,
        json!(1),
        &json!({
            "id": target,
            "name": "renamed",
            "attach_clear": true,
            "attach_remote_workspace": "not-a-number",
        }),
    );

    assert!(res.error.is_none(), "{:?}", res.error);
    assert_eq!(
        engine
            .workspace_at(0)
            .expect("workspace index is valid")
            .name,
        "renamed"
    );
}

#[test]
fn valid_create_still_applies_category_and_mapping() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    let mut core = crate::ipc::handler::cli_entry_tests::test_core();
    let other = engine.create_category("other").expect("카테고리 생성");

    let res = handle_workspace_create(
        &mut core,
        &mut state,
        &mut engine,
        json!(1),
        &json!({
            "name": "full-create",
            "category": other,
            "attach_profile": "p",
            "attach_remote_workspace": 3,
        }),
    );

    let result = res.result.unwrap_or_else(|| panic!("{:?}", res.error));
    let index = result["index"].as_u64().expect("index") as usize;
    let ws = engine
        .workspace_at(index)
        .expect("workspace index is valid");
    assert_eq!(ws.name, "full-create");
    assert_eq!(ws.category, other);
    assert_eq!(
        ws.attach_mapping.as_ref().and_then(|m| m.remote_workspace),
        Some(3)
    );
}
