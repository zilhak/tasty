//! 탭 스트립 메뉴 "새 HTML..."이 여는 피커의 대상 pane·필터·출처와 기존 진입점의 확정 동작을 검사한다.

use super::{FILE_PICKER_POPUP_ID, FilePickerStart, open, open_for_new_tab};
use crate::core::origin::{AgentSource, IntentOrigin, UserSource};
use crate::intent::{DispatchedIntent, Intent, UiIntent};
use crate::state::{FilePickerOnConfirm, FilePickerRequester};

const HTML_EXTENSIONS: &[&str] = &["html", "htm", "svg"];

fn open_popup_requests(intents: &[DispatchedIntent]) -> Vec<&DispatchedIntent> {
    intents
        .iter()
        .filter(|r| {
            matches!(
                r.body,
                Intent::Ui(UiIntent::OpenPopup {
                    id: FILE_PICKER_POPUP_ID,
                    ..
                })
            )
        })
        .collect()
}

#[test]
fn new_html_picker_filters_html_htm_svg_and_targets_the_pane() {
    let (mut state, mut engine_session) = crate::state::tests::two_pane_fixture();
    let engine = engine_session.borrow_mut();
    open_for_new_tab(
        &mut state,
        &engine.read(),
        20,
        "html",
        "url",
        HTML_EXTENSIONS,
    );

    let data = state.dialogs.file_picker.as_ref().expect("picker opened");
    assert_eq!(data.filters, vec!["html", "htm", "svg"]);
    assert_eq!(
        data.on_confirm,
        FilePickerOnConfirm::NewTab {
            pane_id: 20,
            kind: "html".into(),
            param_key: "url".into(),
        }
    );
    assert!(data.requester.is_none());
    assert!(data.mirror_ws_id.is_none());
    let intents = state.take_pending_intents();
    let opens = open_popup_requests(&intents);
    assert_eq!(opens.len(), 1);
    assert!(matches!(
        opens[0].origin,
        IntentOrigin::User {
            source: UserSource::ContextMenu
        }
    ));
}

/// 메뉴를 연 pane이 활성 pane이 아니어도, 피커가 열린 동안 활성 pane이 바뀌어도 대상은 그 pane이다.
#[test]
fn new_html_targets_menu_pane_not_active_pane() {
    let (mut state, mut engine_session) = crate::state::tests::two_pane_fixture();
    let engine = engine_session.borrow_mut();
    assert_eq!(state.focused_pane_id(&engine), 20);
    open_for_new_tab(
        &mut state,
        &engine.read(),
        10,
        "html",
        "url",
        HTML_EXTENSIONS,
    );
    state
        .navigation
        .select_pane(engine.workspace_at(0).unwrap(), 20);
    state.take_pending_intents();

    let on_confirm = state
        .dialogs
        .file_picker
        .as_ref()
        .unwrap()
        .on_confirm
        .clone();
    crate::app::dispatch::file_picker::dispatch_picker_result(
        &mut state,
        &on_confirm,
        &crate::state::FilePickerResult::Confirmed {
            paths: vec!["/tmp/x/a.svg".to_string()],
            is_remote: false,
        },
        |pane| engine.has_pane(pane),
    );
    let intents = state.take_pending_intents();
    assert_eq!(intents.len(), 1);
    assert!(
        matches!(
            intents[0].body,
            Intent::Domain(crate::app::command::DomainIntent::CreateTab { pane_id: 10, .. })
        ),
        "{:?}",
        intents[0].body
    );
}

/// mirror pane에서는 피커를 열지 않고 원격 열기 불가 토스트만 띄운다.
#[test]
fn new_html_on_mirror_pane_does_not_open_picker() {
    let (mut state, mut engine_session) = crate::state::tests::test_mirror_state();
    let engine = engine_session.borrow_mut();
    let ws = engine.workspace_at(0).expect("mirror workspace");
    assert!(ws.mirror);
    let pane_id = ws.pane_layout().all_pane_ids()[0];

    open_for_new_tab(
        &mut state,
        &engine.read(),
        pane_id,
        "html",
        "url",
        HTML_EXTENSIONS,
    );

    assert!(state.dialogs.file_picker.is_none());
    assert!(open_popup_requests(&state.take_pending_intents()).is_empty());
    assert_eq!(state.toasts.len(), 1);
}

#[test]
fn tools_menu_picker_keeps_open_file_confirm() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let engine = engine_session.borrow_mut();
    open(
        &mut state,
        &engine.read(),
        None,
        Vec::new(),
        FilePickerStart::from_surface(&engine.read(), None),
    );
    let data = state.dialogs.file_picker.as_ref().expect("picker opened");
    assert_eq!(data.on_confirm, FilePickerOnConfirm::OpenFile);
    let intents = state.take_pending_intents();
    let opens = open_popup_requests(&intents);
    assert_eq!(opens.len(), 1);
    assert!(matches!(
        opens[0].origin,
        IntentOrigin::User {
            source: UserSource::Menu("tools_menu")
        }
    ));
}

/// 플러그인 요청은 결과 이벤트를 받을 requester를 유지하고 기본 확정 동작을 쓴다.
#[test]
fn plugin_requested_picker_keeps_requester_and_open_file_confirm() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let engine = engine_session.borrow_mut();
    open(
        &mut state,
        &engine.read(),
        Some(FilePickerRequester {
            plugin_id: "com.example.p".into(),
            request_id: 3,
            owner_popup_instance: None,
        }),
        vec!["md".into()],
        FilePickerStart::from_surface(&engine.read(), None),
    );
    let data = state.dialogs.file_picker.as_ref().expect("picker opened");
    assert_eq!(data.on_confirm, FilePickerOnConfirm::OpenFile);
    assert_eq!(
        data.requester.as_ref().map(|r| r.request_id),
        Some(3),
        "file_picker.result 이벤트를 보낼 requester를 유지한다"
    );
    let intents = state.take_pending_intents();
    let opens = open_popup_requests(&intents);
    assert_eq!(opens.len(), 1);
    assert!(matches!(
        &opens[0].origin,
        IntentOrigin::Agent {
            source: AgentSource::Plugin(id)
        } if id == "com.example.p"
    ));
}
