//! plugin popup ↔ host popup 부모-자식 소유권(ADR-0082) 회귀 테스트.
//!
//! 커버 범위:
//! - 소유 관계 조회(`AppState::plugin_popup_has_open_child`) — 자식이 자진 신고한
//!   부모 instance 만 단일 진실로 쓰는지.
//! - 연쇄 정리(`app::dispatch::plugin_popup_events::cancel_child_file_picker`) —
//!   부모가 어떤 사유로 닫히든 자식이 고아로 남지 않고, 이미 확정된 결과는 덮이지
//!   않는지.
//! - Esc 소유권 후보 선정(`PopupManager::topmost_visible_open`) — 최상단 하나만
//!   고르는지, scope 로 숨은 popup 이 후보에서 빠지는지.
//!
//! 좌표 히트테스트 쪽 판정은 `adapters::ui::popup::occlusion` 의 단위 테스트가,
//! popup 닫힘 뒷정리 전반은 [`super::popup_close_tests`] 가 담당한다.

use super::tests::test_state;
use crate::adapters::ui::LayoutContext;
use crate::adapters::ui::popup::PopupManager;
use crate::adapters::ui::popup::file_picker::FILE_PICKER_POPUP_ID;
use crate::adapters::ui::popup::{PopupScope, defs};
use crate::app::dispatch::plugin_popup_events::cancel_child_file_picker;
use crate::state::{AppState, FilePickerData, FilePickerRequester, FilePickerResult, FpLoadState};
use tasty_plugin_protocol::PopupCloseReason;

const OWNER_IID: u64 = 7;
const PORT_SCANNER_ID: &str = "port_scanner";

/// `file_picker` 가 열려 있고 `owner` 인스턴스가 부모로 기록된 상태를 만든다.
/// `owner: None` 은 Tools 메뉴 진입(requester 자체가 없는 경로)을 재현한다.
fn state_with_picker(owner: Option<u64>) -> AppState {
    let (mut state, _engine) = test_state();
    state.dialogs.file_picker = Some(FilePickerData {
        mirror_ws_id: None,
        remote_host: None,
        current_dir: "/tmp".to_string(),
        load: FpLoadState::Empty,
        entries: Vec::new(),
        selected: Vec::new(),
        result: None,
        requester: owner.map(|iid| FilePickerRequester {
            plugin_id: "com.example.any".to_string(),
            request_id: 1,
            owner_popup_instance: Some(iid),
        }),
        filters: Vec::new(),
    });
    state
}

#[test]
fn owner_instance_is_reported_as_having_an_open_child() {
    let state = state_with_picker(Some(OWNER_IID));
    assert!(state.plugin_popup_has_open_child(OWNER_IID));
    assert!(!state.plugin_popup_has_open_child(OWNER_IID + 1));
}

/// Tools 메뉴로 연 피커(requester=None)는 어떤 plugin popup 의 자식도 아니다 —
/// 그 경로가 plugin popup 의 dismiss 를 막아버리면 회귀다.
#[test]
fn tools_menu_picker_is_nobodys_child() {
    let state = state_with_picker(None);
    assert!(!state.plugin_popup_has_open_child(OWNER_IID));
}

#[test]
fn no_picker_open_means_no_child() {
    let (state, _engine) = test_state();
    assert!(!state.plugin_popup_has_open_child(OWNER_IID));
}

/// 부모가 닫히면 자식 피커에 취소 결과가 채워진다 — 그래야 기존 result drain 이
/// 평소 경로로 돌아 plugin 에 결과를 정확히 한 번 보내고 피커도 정리된다.
#[test]
fn closing_the_owner_cancels_its_child_picker() {
    let mut state = state_with_picker(Some(OWNER_IID));
    cancel_child_file_picker(&mut state, &[(OWNER_IID, PopupCloseReason::OutsideClick)]);
    assert!(matches!(
        state.dialogs.file_picker.as_ref().unwrap().result,
        Some(FilePickerResult::Cancelled)
    ));
}

/// 어떤 사유로 닫히든(Escape / 명시적 close) 동일하게 정리된다.
///
/// **사유(reason) 무관함일 뿐 경로(path) 커버가 아니다** — 이 테스트는
/// `cancel_child_file_picker` 를 직접 호출하므로, 어떤 호출처가 이 함수에 아예 닿지
/// 않는(=`plugin_popup_closes` 큐를 건너뛰고 매니저를 직접 치는) 결함은 못 잡는다.
/// 실제로 `PluginRequest` 경로가 그랬다. 그 모양은
/// `tests/plugin_popup_close_chokepoint.rs` 가 소스 수준에서 막는다.
#[test]
fn cascade_cleanup_is_independent_of_the_close_reason() {
    for reason in [
        PopupCloseReason::Escape,
        PopupCloseReason::OutsideClick,
        PopupCloseReason::PluginRequest,
        PopupCloseReason::HostShutdown,
    ] {
        let mut state = state_with_picker(Some(OWNER_IID));
        cancel_child_file_picker(&mut state, &[(OWNER_IID, reason)]);
        assert!(
            state.dialogs.file_picker.as_ref().unwrap().result.is_some(),
            "close reason {reason:?} should still clean up the child"
        );
    }
}

#[test]
fn closing_an_unrelated_plugin_popup_leaves_the_picker_alone() {
    let mut state = state_with_picker(Some(OWNER_IID));
    cancel_child_file_picker(&mut state, &[(OWNER_IID + 1, PopupCloseReason::Escape)]);
    assert!(state.dialogs.file_picker.as_ref().unwrap().result.is_none());
}

/// 사용자가 이미 파일을 고른 프레임에 부모가 닫혔다면 그 확정을 취소로 덮지 않는다.
#[test]
fn cascade_cleanup_does_not_overwrite_a_settled_result() {
    let mut state = state_with_picker(Some(OWNER_IID));
    state.dialogs.file_picker.as_mut().unwrap().result = Some(FilePickerResult::Confirmed {
        paths: vec!["/tmp/a.md".to_string()],
        is_remote: false,
    });
    cancel_child_file_picker(&mut state, &[(OWNER_IID, PopupCloseReason::Escape)]);
    assert!(matches!(
        state.dialogs.file_picker.as_ref().unwrap().result,
        Some(FilePickerResult::Confirmed { .. })
    ));
}

/// 연쇄 정리가 성공하면 부모-자식 링크를 끊는다 — 남겨두면 다음 tick 의 result push 가
/// "소유 popup 이 사라진 채 결과 도착"(ADR-0082 Decision 4)으로 오인해 경고를 낸다.
/// 그 경고는 정리가 실패했을 때만 나와야 진단 가치가 있다.
#[test]
fn successful_cascade_cleanup_severs_the_owner_link() {
    let mut state = state_with_picker(Some(OWNER_IID));
    cancel_child_file_picker(&mut state, &[(OWNER_IID, PopupCloseReason::PluginRequest)]);
    let req = state
        .dialogs
        .file_picker
        .as_ref()
        .unwrap()
        .requester
        .as_ref()
        .unwrap();
    assert_eq!(req.owner_popup_instance, None);
    // 결과 라우팅에 필요한 나머지는 그대로여야 한다 — 결과는 여전히 정확히 한 번 간다.
    assert_eq!(req.request_id, 1);
    assert_eq!(req.plugin_id, "com.example.any");
}

/// 반대로 사용자가 이미 파일을 확정한 뒤 부모가 죽은 경우엔 링크를 남긴다 — 고른 파일을
/// 받을 popup 이 사라진 것이라 그 경고가 진짜 신호다.
#[test]
fn a_settled_result_keeps_the_owner_link_for_the_loss_warning() {
    let mut state = state_with_picker(Some(OWNER_IID));
    state.dialogs.file_picker.as_mut().unwrap().result = Some(FilePickerResult::Confirmed {
        paths: vec!["/tmp/a.md".to_string()],
        is_remote: false,
    });
    cancel_child_file_picker(&mut state, &[(OWNER_IID, PopupCloseReason::PluginRequest)]);
    assert_eq!(
        state
            .dialogs
            .file_picker
            .as_ref()
            .unwrap()
            .requester
            .as_ref()
            .unwrap()
            .owner_popup_instance,
        Some(OWNER_IID)
    );
}

/// 링크가 끊긴 뒤에는 그 instance 가 더 이상 "열린 자식을 가진 부모" 가 아니다 —
/// dismiss 억제(스택 유지)가 죽은 부모 때문에 계속 걸려 있으면 안 된다.
#[test]
fn severed_link_stops_reporting_an_open_child() {
    let mut state = state_with_picker(Some(OWNER_IID));
    assert!(state.plugin_popup_has_open_child(OWNER_IID));
    cancel_child_file_picker(&mut state, &[(OWNER_IID, PopupCloseReason::Escape)]);
    assert!(!state.plugin_popup_has_open_child(OWNER_IID));
}

/// Tools 메뉴 피커는 plugin popup 이 닫혀도 영향을 받지 않는다(회귀 확인).
#[test]
fn tools_menu_picker_survives_plugin_popup_closes() {
    let mut state = state_with_picker(None);
    cancel_child_file_picker(&mut state, &[(OWNER_IID, PopupCloseReason::Escape)]);
    assert!(state.dialogs.file_picker.as_ref().unwrap().result.is_none());
}

/// 모든 `PopupDef` 가 등록된 매니저 — 실제 popup 시스템과 같은 레지스트리를 쓴다.
fn registered_manager() -> PopupManager {
    let mut mgr = PopupManager::new();
    for def in defs::all_defs() {
        mgr.register_def(def, 1.0);
    }
    mgr
}

fn layout_ctx(active_workspace: usize) -> LayoutContext {
    LayoutContext {
        active_workspace,
        pane_rects: Vec::new(),
        surface_rects: Vec::new(),
        active_tabs: Vec::new(),
    }
}

#[test]
fn topmost_visible_open_picks_the_highest_z() {
    let mut mgr = registered_manager();
    mgr.open(FILE_PICKER_POPUP_ID);
    mgr.open(PORT_SCANNER_ID);
    let (id, _) = mgr
        .topmost_visible_open(None)
        .expect("two popups are open, one must be topmost");
    assert_eq!(id, PORT_SCANNER_ID, "later open wins");

    // 위에 있던 popup 이 닫히면 Esc 소유권 후보가 그 아래로 내려온다.
    mgr.close(PORT_SCANNER_ID);
    let (id, _) = mgr.topmost_visible_open(None).unwrap();
    assert_eq!(id, FILE_PICKER_POPUP_ID);
}

#[test]
fn topmost_visible_open_is_none_without_open_popups() {
    let mgr = registered_manager();
    assert!(mgr.topmost_visible_open(None).is_none());
}

/// scope 로 가려진 popup 은 그려지지도 않으므로 Esc 후보가 아니다.
#[test]
fn scope_hidden_popup_is_not_the_escape_candidate() {
    let mut mgr = registered_manager();
    mgr.open(FILE_PICKER_POPUP_ID);
    // 활성 workspace 가 아닌 스코프로 열어 이번 프레임에 숨긴다.
    mgr.open_with_scope(PORT_SCANNER_ID, PopupScope::Workspace(9));
    let ctx = layout_ctx(0);
    let (id, _) = mgr.topmost_visible_open(Some(&ctx)).unwrap();
    assert_eq!(id, FILE_PICKER_POPUP_ID);
    // 같은 workspace 가 활성이면 다시 후보가 된다.
    let ctx = layout_ctx(9);
    let (id, _) = mgr.topmost_visible_open(Some(&ctx)).unwrap();
    assert_eq!(id, PORT_SCANNER_ID);
}
