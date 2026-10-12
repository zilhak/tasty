use super::dispatch_picker_result;
use crate::app::command::DomainIntent;
use crate::intent::{DispatchedIntent, Intent};
use crate::state::{FilePickerOnConfirm, FilePickerResult};

fn new_html_tab(pane_id: u32) -> FilePickerOnConfirm {
    FilePickerOnConfirm::NewTab {
        pane_id,
        kind: "html".into(),
        param_key: "url".into(),
    }
}

fn is_context_menu(request: &DispatchedIntent) -> bool {
    matches!(
        request.origin,
        crate::intent::IntentOrigin::User {
            source: crate::core::origin::UserSource::ContextMenu
        }
    )
}

fn confirmed(paths: &[String], is_remote: bool) -> FilePickerResult {
    FilePickerResult::Confirmed {
        paths: paths.to_vec(),
        is_remote,
    }
}

fn dispatch_file_count(intents: &[DispatchedIntent]) -> usize {
    intents
        .iter()
        .filter(|r| matches!(r.body, Intent::Domain(DomainIntent::DispatchFile { .. })))
        .count()
}

#[test]
fn confirming_new_html_picker_creates_html_tab_in_that_pane() {
    let (mut state, _engine) = crate::state::tests::test_state();
    let copy = dispatch_picker_result(
        &mut state,
        &new_html_tab(7),
        &confirmed(&["/tmp/x/index.html".to_string()], false),
        |pane| pane == 7,
    );
    assert!(!copy, "탭 생성 동작은 클립보드에 복사하지 않는다");
    let intents = state.take_pending_intents();
    assert_eq!(intents.len(), 1, "{intents:?}");
    let Intent::Domain(DomainIntent::CreateTab {
        pane_id,
        kind,
        surface_params,
        activate,
        cwd,
        name,
    }) = &intents[0].body
    else {
        panic!("expected CreateTab, got {:?}", intents[0].body);
    };
    assert_eq!(*pane_id, 7);
    assert_eq!(kind, "html");
    assert_eq!(
        surface_params,
        &serde_json::json!({"url": "/tmp/x/index.html"})
    );
    assert!(*activate);
    assert!(cwd.is_none());
    assert!(name.is_none());
    assert!(is_context_menu(&intents[0]));
    assert_eq!(dispatch_file_count(&intents), 0);
}

/// 피커는 단일 선택이지만 paths는 목록이다. 경로마다 탭을 하나씩 만든다.
#[test]
fn confirming_new_html_picker_with_two_paths_creates_two_tabs() {
    let (mut state, _engine) = crate::state::tests::test_state();
    dispatch_picker_result(
        &mut state,
        &new_html_tab(7),
        &confirmed(&["/a.html".to_string(), "/b.svg".to_string()], false),
        |_| true,
    );
    let urls: Vec<_> = state
        .take_pending_intents()
        .into_iter()
        .map(|r| match r.body {
            Intent::Domain(DomainIntent::CreateTab {
                pane_id: 7,
                surface_params,
                ..
            }) => surface_params["url"].as_str().map(str::to_string),
            other => panic!("expected CreateTab in pane 7, got {other:?}"),
        })
        .collect();
    assert_eq!(
        urls,
        vec![Some("/a.html".to_string()), Some("/b.svg".to_string())]
    );
}

#[test]
fn confirming_after_target_pane_closed_creates_nothing() {
    let (mut state, _engine) = crate::state::tests::test_state();
    let copy = dispatch_picker_result(
        &mut state,
        &new_html_tab(7),
        &confirmed(&["/tmp/x/index.html".to_string()], false),
        |_| false,
    );
    assert!(!copy);
    assert!(state.take_pending_intents().is_empty());
}

/// mirror pane에서는 피커를 열지 않으므로 원격 경로가 오면 탭도 복사도 만들지 않는다.
#[test]
fn new_tab_confirm_with_remote_path_creates_nothing() {
    let (mut state, _engine) = crate::state::tests::test_state();
    let copy = dispatch_picker_result(
        &mut state,
        &new_html_tab(7),
        &confirmed(&["/remote/index.html".to_string()], true),
        |_| true,
    );
    assert!(!copy);
    assert!(state.take_pending_intents().is_empty());
}

#[test]
fn default_picker_confirm_still_dispatches_file() {
    let (mut state, _engine) = crate::state::tests::test_state();
    let copy = dispatch_picker_result(
        &mut state,
        &FilePickerOnConfirm::OpenFile,
        &confirmed(&["/tmp/x/notes.md".to_string()], false),
        |_| true,
    );
    assert!(!copy);
    let intents = state.take_pending_intents();
    assert_eq!(intents.len(), 1);
    assert_eq!(dispatch_file_count(&intents), 1);
}

#[test]
fn default_picker_remote_confirm_still_copies_to_clipboard() {
    let (mut state, _engine) = crate::state::tests::test_state();
    let copy = dispatch_picker_result(
        &mut state,
        &FilePickerOnConfirm::OpenFile,
        &confirmed(&["/remote/notes.md".to_string()], true),
        |_| true,
    );
    assert!(copy);
    assert!(state.take_pending_intents().is_empty());
}

#[test]
fn cancelling_new_html_picker_creates_nothing() {
    let (mut state, _engine) = crate::state::tests::test_state();
    let copy = dispatch_picker_result(
        &mut state,
        &new_html_tab(7),
        &FilePickerResult::Cancelled,
        |_| true,
    );
    assert!(!copy);
    assert!(state.take_pending_intents().is_empty());
}
