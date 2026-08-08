//! popup 닫힘 뒷정리(현재는 `draw_popups`, `src/adapters/ui/notification.rs:231-347`)의
//! 현재 동작을 고정하는 회귀 테스트 — **프로덕션 코드는 한 줄도 바꾸지 않는다.**
//!
//! 이후 이 뒷정리를 `on_close` 훅으로 이관하는 작업(popup close 리팩터 체인)이
//! 이 테스트를 안전망으로 쓴다. 커버 대상은 9개 팝업 각각의 "draw_fn → Close"
//! 경로(`dispatch_closed`)와 "X 버튼/외부 클릭" 경로(`draw_result.closed`) —
//! 두 경로가 서로 다른 코드(`content_fn` 콜백 vs `PopupManager::draw` 자체의
//! 포인터 처리)로 채워지므로 각각 별도로 exercise 해야 wiring 자체가 검증된다.
//!
//! ## 테스트 하네스
//!
//! `egui::Context::default()` + `ctx.run(raw_input, |ctx| { draw_popups(...) })`
//! 로 실제 GUI/디스플레이 없이 popup 시스템을 구동한다 — 이 패턴은 이미
//! `src/adapters/ui/dialog.rs` 의 `mod tests`(`run_with_input`)가 쓰고 있는 기존
//! 선례를 그대로 따른다. Close 트리거는 두 갈래:
//!
//! - **path 1 (draw_fn Close)**: 대부분의 draw_fn 은 `Key::Escape` 를 직접 체크해
//!   `PopupAction::Close` 를 반환한다(`convert`/`rename`/`rail_category`/
//!   `confirm_delete_category`/`file_handler_picker`/`file_picker`). `approval`
//!   은 의도적으로 Escape 를 받지 않으므로(주석 참고) 대신 "큐가 빈" 상태를 직접
//!   구성해 같은 반환을 유도한다. `transfer_progress` 는 Escape 를 아예 받지
//!   않고(진행 중 실수 dismiss 방지) rows 가 비면 self-close 하므로 그 상태로
//!   유도한다.
//! - **path 2 (X 버튼/외부 클릭)**: `close_on_outside_click=true` 인 팝업
//!   (`convert_surface`/`rail_category`/`transfer_error`/`confirm_delete_category`)
//!   은 팝업 바깥 좌표에 pointer press 이벤트를 주입한다. 나머지 non-headless +
//!   `close_on_outside_click=false` 팝업(`rename`/`approval`/`file_handler_picker`/
//!   `file_picker`)은 X 버튼(타이틀바 우측 닫기 아이콘)의 정확한 좌표에 press
//!   이벤트를 주입한다 — 좌표 계산은 `PopupState`의 **공개** 필드(`pos`/`size`)와
//!   **공개** 함수(`title_bar_height()`)만으로 유도한다(popup.rs 의 private
//!   `close_btn_rect()` 공식을 그대로 복제 — popup.rs 를 건드리지 않고 접근할 수
//!   있는 유일한 방법). `transfer_progress` 는 headless(타이틀바 없음) 이면서
//!   `close_on_outside_click=false` 라 draw() 내장 포인터 경로로는 **원천적으로
//!   도달 불가능** — path 2 테스트가 없다(아래 각주 참고).
//!
//! `open_at_focused(id, pos)` 로 팝업을 **고정 좌표**에 연다 — `open_centered_focused`
//! 를 쓰면 `request_center` 가 `draw()` 내부에서 그 프레임에 소비되어(포인터
//! hit-test 는 그보다 앞서 일어남) 첫 프레임의 위치가 불확실해진다. 고정 좌표를
//! 쓰면 등록 직후(draw_popups 호출 전)부터 `pos`/`size` 가 확정적이다. 단
//! `sizer: Some(..)` 가 있는 팝업(approval/file_handler_picker/file_picker 등)은
//! `size` 가 `draw_popups` 최초 호출 시 sizer 로 재계산되므로, X 버튼 좌표 계산
//! 전에 입력 없는 "priming" 프레임을 한 번 돌려 `size` 를 확정한 뒤 읽는다.

use super::tests::test_state;
use crate::adapters::ui::info_modal::{INFO_MODAL_ID, InfoModal, InfoModalAction};
use crate::adapters::ui::notification::draw_popups;
use crate::adapters::ui::popup::approval::APPROVAL_POPUP_ID;
use crate::adapters::ui::popup::confirm_delete_category::CONFIRM_DELETE_CATEGORY_POPUP_ID;
use crate::adapters::ui::popup::file_handler_picker::PICKER_POPUP_ID;
use crate::adapters::ui::popup::file_picker::FILE_PICKER_POPUP_ID;
use crate::adapters::ui::popup::port_scanner::{
    PORT_SCANNER_POPUP_ID, PortRowView, PortScanState, SourceTag,
};
use crate::adapters::ui::popup::preset_apply::APPLY_WORKSPACE_POPUP_ID;
use crate::adapters::ui::popup::rail_category::RAIL_CATEGORY_POPUP_ID;
use crate::adapters::ui::popup::transfer::{
    TRANSFER_ERROR_POPUP_ID, TRANSFER_PROGRESS_POPUP_ID, TransferError, TransferProgress,
};
use crate::adapters::ui::popup::{PopupId, title_bar_height};
use crate::intent::{Intent, UiIntent};
use crate::model::{PhysicalPx, PhysicalRect};
use crate::state::{
    FileHandlerPickerData, FileHandlerPickerResult, FilePickerData, FilePickerResult, FpLoadState,
    RenameTarget,
};
use tasty_approval::{ApprovalId, ApprovalRequest, Requester, Severity};

const CONVERT_SURFACE_POPUP_ID: PopupId = "convert_surface";
const RENAME_POPUP_ID: PopupId = "rename";

/// popup 클램프에 넉넉한 여유를 주는 가상 화면 rect (1920x1080).
fn term_rect() -> PhysicalRect {
    PhysicalRect {
        x: PhysicalPx(0.0),
        y: PhysicalPx(0.0),
        width: PhysicalPx(1920.0),
        height: PhysicalPx(1080.0),
    }
}

fn screen_rect() -> egui::Rect {
    egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920.0, 1080.0))
}

fn empty_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(screen_rect()),
        ..Default::default()
    }
}

fn key_input(key: egui::Key) -> egui::RawInput {
    let mut raw = empty_input();
    raw.events.push(egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    raw
}

fn press_input(pos: egui::Pos2) -> egui::RawInput {
    let mut raw = empty_input();
    raw.events.push(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    raw
}

/// popup 바깥, 화면 안의 한 점. `pos`(좌상단)보다 왼쪽 위로 충분히 떨어뜨려
/// popup_rect(=[pos, pos+size]) 바깥임을 보장한다.
fn outside_point(popup_pos: egui::Pos2) -> egui::Pos2 {
    egui::pos2(
        (popup_pos.x - 200.0).max(0.0),
        (popup_pos.y - 200.0).max(0.0),
    )
}

/// popup.rs 의 private `close_btn_rect()` 공식 복제(X 버튼 중심) — 공개 필드
/// (`pos`/`size`)와 공개 함수(`title_bar_height()`)만 사용. 상단 doc 참고.
fn close_button_point(popup_pos: egui::Pos2, popup_size: egui::Vec2) -> egui::Pos2 {
    egui::pos2(
        popup_pos.x + popup_size.x - 14.0,
        popup_pos.y + title_bar_height() / 2.0,
    )
}

fn run_frame(
    raw: egui::RawInput,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
) {
    let ctx = egui::Context::default();
    drop(ctx.run(raw, |ctx| {
        draw_popups(ctx, state, engine, &[], term_rect(), 1.0);
    }));
}

/// X 버튼 테스트 전용 — priming 프레임으로 sizer 반영 후의 실제 pos/size 를 읽는다.
fn primed_popup_geometry(
    id: PopupId,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
) -> (egui::Pos2, egui::Vec2) {
    run_frame(empty_input(), state, engine);
    let p = state.popups.get_mut(id).expect("popup registered");
    (p.pos, p.size)
}

const FIXED_POS: egui::Pos2 = egui::pos2(500.0, 500.0);

// ───────────────────────── convert_surface ─────────────────────────

#[test]
fn convert_surface_escape_close_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    let surface_id = engine.workspaces[0].all_surface_ids()[0];
    state.dialogs.convert_popup = Some(surface_id);
    state.dialogs.convert_popup_selected = Some(0);
    state
        .popups
        .open_at_focused(CONVERT_SURFACE_POPUP_ID, FIXED_POS);

    run_frame(key_input(egui::Key::Escape), &mut state, &mut engine);

    assert!(!state.popups.is_open(CONVERT_SURFACE_POPUP_ID));
    assert!(state.dialogs.convert_popup.is_none());
    assert!(state.dialogs.convert_popup_selected.is_none());
}

#[test]
fn convert_surface_outside_click_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    let surface_id = engine.workspaces[0].all_surface_ids()[0];
    state.dialogs.convert_popup = Some(surface_id);
    state.dialogs.convert_popup_selected = Some(0);
    state
        .popups
        .open_at_focused(CONVERT_SURFACE_POPUP_ID, FIXED_POS);

    run_frame(
        press_input(outside_point(FIXED_POS)),
        &mut state,
        &mut engine,
    );

    assert!(!state.popups.is_open(CONVERT_SURFACE_POPUP_ID));
    assert!(state.dialogs.convert_popup.is_none());
    assert!(state.dialogs.convert_popup_selected.is_none());
}

/// `on_close` 훅 이관 후 — 이전엔 못 잡던 `UiIntent::ClosePopup` 경로도 이제
/// 뒷정리가 돈다(`close_intent_now_clears_cleanup_after_next_frame` 이 고정한
/// rename 의 동일 패턴과 대조). 인텐트 핸들러 자체는 drain 을 안 하므로, 실제
/// 프레임과 동일하게 그 뒤에 `run_frame` 을 한 번 더 돌려야 훅이 발화한다.
#[test]
fn convert_surface_close_intent_now_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    let surface_id = engine.workspaces[0].all_surface_ids()[0];
    state.dialogs.convert_popup = Some(surface_id);
    state.dialogs.convert_popup_selected = Some(0);
    state
        .popups
        .open_at_focused(CONVERT_SURFACE_POPUP_ID, FIXED_POS);

    crate::intent::popup::handle(
        &mut state,
        &UiIntent::ClosePopup {
            id: CONVERT_SURFACE_POPUP_ID,
        }
        .from_user_menu("test"),
    );
    assert!(!state.popups.is_open(CONVERT_SURFACE_POPUP_ID));
    // 인텐트 핸들러 직후엔 아직 drain 전 — 큐에만 쌓여 있다.
    assert!(state.dialogs.convert_popup.is_some());

    run_frame(empty_input(), &mut state, &mut engine);

    assert!(state.dialogs.convert_popup.is_none());
    assert!(state.dialogs.convert_popup_selected.is_none());
}

// ───────────────────────────── rename ─────────────────────────────

#[test]
fn rename_escape_close_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    state.dialogs.rename = Some((RenameTarget::NewCategory, "abc".to_string()));
    state.popups.open_at_focused(RENAME_POPUP_ID, FIXED_POS);

    run_frame(key_input(egui::Key::Escape), &mut state, &mut engine);

    assert!(!state.popups.is_open(RENAME_POPUP_ID));
    assert!(state.dialogs.rename.is_none());
}

#[test]
fn rename_x_button_close_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    state.dialogs.rename = Some((RenameTarget::NewCategory, "abc".to_string()));
    state.popups.open_at_focused(RENAME_POPUP_ID, FIXED_POS);
    // rename 은 sizer 가 없어(defs.rs) priming 없이도 size 가 이미 확정이지만,
    // 다른 팝업과 동일 절차를 쓰기 위해 그대로 priming 헬퍼를 재사용한다.
    let (pos, size) = primed_popup_geometry(RENAME_POPUP_ID, &mut state, &mut engine);

    run_frame(
        press_input(close_button_point(pos, size)),
        &mut state,
        &mut engine,
    );

    assert!(!state.popups.is_open(RENAME_POPUP_ID));
    assert!(state.dialogs.rename.is_none());
}

// ─────────────────────────── rail_category ───────────────────────────

#[test]
fn rail_category_escape_close_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    let cat_id = engine.categories()[0].id;
    state.dialogs.rail_category_popup = Some(cat_id);
    state
        .popups
        .open_at_focused(RAIL_CATEGORY_POPUP_ID, FIXED_POS);

    run_frame(key_input(egui::Key::Escape), &mut state, &mut engine);

    assert!(!state.popups.is_open(RAIL_CATEGORY_POPUP_ID));
    assert!(state.dialogs.rail_category_popup.is_none());
}

/// `on_close` 훅 이관 후 — `UiIntent::ClosePopup` 경로도 다음 프레임의 drain 을
/// 거쳐 뒷정리가 돈다(`close_intent_now_clears_cleanup_after_next_frame` 과 동일 패턴).
#[test]
fn rail_category_close_intent_now_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    let cat_id = engine.categories()[0].id;
    state.dialogs.rail_category_popup = Some(cat_id);
    state
        .popups
        .open_at_focused(RAIL_CATEGORY_POPUP_ID, FIXED_POS);

    crate::intent::popup::handle(
        &mut state,
        &UiIntent::ClosePopup {
            id: RAIL_CATEGORY_POPUP_ID,
        }
        .from_user_menu("test"),
    );
    assert!(!state.popups.is_open(RAIL_CATEGORY_POPUP_ID));
    assert!(state.dialogs.rail_category_popup.is_some());

    run_frame(empty_input(), &mut state, &mut engine);

    assert!(state.dialogs.rail_category_popup.is_none());
}

#[test]
fn rail_category_outside_click_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    let cat_id = engine.categories()[0].id;
    state.dialogs.rail_category_popup = Some(cat_id);
    state
        .popups
        .open_at_focused(RAIL_CATEGORY_POPUP_ID, FIXED_POS);

    run_frame(
        press_input(outside_point(FIXED_POS)),
        &mut state,
        &mut engine,
    );

    assert!(!state.popups.is_open(RAIL_CATEGORY_POPUP_ID));
    assert!(state.dialogs.rail_category_popup.is_none());
}

// ──────────────────────────── transfer_progress ────────────────────────────
// headless(타이틀바 없음) + close_on_outside_click=false 라 draw() 내장 포인터
// 경로(X 버튼/외부 클릭)로는 원천적으로 도달 불가능 — path 2 테스트는 없다
// (파일 상단 doc 참고). path 1(rows 가 비면 self-close)만 검증한다.

#[test]
fn transfer_progress_empty_rows_close_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    state.dialogs.transfer_progress = Some(TransferProgress { rows: vec![] });
    state
        .popups
        .open_at_focused(TRANSFER_PROGRESS_POPUP_ID, FIXED_POS);

    run_frame(empty_input(), &mut state, &mut engine);

    assert!(!state.popups.is_open(TRANSFER_PROGRESS_POPUP_ID));
    assert!(state.dialogs.transfer_progress.is_none());
}

/// `on_close` 훅 이관 후 — `UiIntent::ClosePopup` 경로도 다음 프레임의 drain 을
/// 거쳐 뒷정리가 돈다(`close_intent_now_clears_cleanup_after_next_frame` 과 동일 패턴).
#[test]
fn transfer_progress_close_intent_now_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    state.dialogs.transfer_progress = Some(TransferProgress { rows: vec![] });
    state
        .popups
        .open_at_focused(TRANSFER_PROGRESS_POPUP_ID, FIXED_POS);

    crate::intent::popup::handle(
        &mut state,
        &UiIntent::ClosePopup {
            id: TRANSFER_PROGRESS_POPUP_ID,
        }
        .from_user_menu("test"),
    );
    assert!(!state.popups.is_open(TRANSFER_PROGRESS_POPUP_ID));
    assert!(state.dialogs.transfer_progress.is_some());

    run_frame(empty_input(), &mut state, &mut engine);

    assert!(state.dialogs.transfer_progress.is_none());
}

// ──────────────────────────── transfer_error ────────────────────────────

fn xfer_err(name: &str) -> TransferError {
    TransferError {
        name: name.to_string(),
        reason: "boom".to_string(),
        retry: None,
    }
}

#[test]
fn transfer_error_escape_close_pops_single_entry() {
    let (mut state, mut engine) = test_state();
    state.dialogs.transfer_error.push_back(xfer_err("a.txt"));
    state
        .popups
        .open_at_focused(TRANSFER_ERROR_POPUP_ID, FIXED_POS);

    run_frame(key_input(egui::Key::Escape), &mut state, &mut engine);

    // draw_fn(Escape) 자체가 head 를 pop 한다 — 큐가 마저 비므로 self-close.
    assert!(!state.popups.is_open(TRANSFER_ERROR_POPUP_ID));
    assert!(state.dialogs.transfer_error.is_empty());
}

#[test]
fn transfer_error_outside_click_with_single_entry_closes_without_reopen() {
    let (mut state, mut engine) = test_state();
    state.dialogs.transfer_error.push_back(xfer_err("a.txt"));
    state
        .popups
        .open_at_focused(TRANSFER_ERROR_POPUP_ID, FIXED_POS);

    run_frame(
        press_input(outside_point(FIXED_POS)),
        &mut state,
        &mut engine,
    );

    assert!(!state.popups.is_open(TRANSFER_ERROR_POPUP_ID));
    assert!(state.dialogs.transfer_error.is_empty());
}

/// 큐에 2건 있을 때 외부 클릭으로 닫으면(= draw_fn 을 거치지 않는 경로) head 만
/// pop 되고, 남은 실패가 있으므로 팝업이 다시 열린다 — TODO 40 체크리스트의
/// "transfer_error 재오픈 동작 테스트".
#[test]
fn transfer_error_outside_click_with_two_entries_pops_head_and_reopens() {
    let (mut state, mut engine) = test_state();
    state.dialogs.transfer_error.push_back(xfer_err("a.txt"));
    state.dialogs.transfer_error.push_back(xfer_err("b.txt"));
    state
        .popups
        .open_at_focused(TRANSFER_ERROR_POPUP_ID, FIXED_POS);

    run_frame(
        press_input(outside_point(FIXED_POS)),
        &mut state,
        &mut engine,
    );

    assert_eq!(state.dialogs.transfer_error.len(), 1);
    assert_eq!(state.dialogs.transfer_error.front().unwrap().name, "b.txt");
    // 재오픈은 `open_centered_focused` 직접 호출(Intent 경유 아님) — 동기 반영.
    assert!(state.popups.is_open(TRANSFER_ERROR_POPUP_ID));
}

/// `on_close` 훅 이관 후 — `UiIntent::ClosePopup` 경로로 닫아도(scrim/외부 클릭과
/// 동일하게 draw_fn 을 거치지 않으므로) 다음 프레임의 drain 이 head 를 dismiss 하고
/// 재오픈까지 수행한다(`close_intent_now_clears_cleanup_after_next_frame` 과 동일
/// 패턴 + 재진입 재오픈 확인).
#[test]
fn transfer_error_close_intent_now_pops_head_and_reopens() {
    let (mut state, mut engine) = test_state();
    state.dialogs.transfer_error.push_back(xfer_err("a.txt"));
    state.dialogs.transfer_error.push_back(xfer_err("b.txt"));
    state
        .popups
        .open_at_focused(TRANSFER_ERROR_POPUP_ID, FIXED_POS);

    crate::intent::popup::handle(
        &mut state,
        &UiIntent::ClosePopup {
            id: TRANSFER_ERROR_POPUP_ID,
        }
        .from_user_menu("test"),
    );
    assert!(!state.popups.is_open(TRANSFER_ERROR_POPUP_ID));
    // handle() 직후 — 아직 drain 전, 큐엔 여전히 2건.
    assert_eq!(state.dialogs.transfer_error.len(), 2);

    run_frame(empty_input(), &mut state, &mut engine);

    assert_eq!(state.dialogs.transfer_error.len(), 1);
    assert_eq!(state.dialogs.transfer_error.front().unwrap().name, "b.txt");
    assert!(state.popups.is_open(TRANSFER_ERROR_POPUP_ID));
}

// ──────────────────────────── port_scanner ────────────────────────────

/// 결정 고정: 바깥 클릭(draw_fn 을 거치지 않는 경로)으로 닫아도 스캔 결과를
/// 초기화하지 않는다 — `on_close: None`(defs.rs 근거 주석). 재오픈 시 이전 결과를
/// 그대로 보여주는 것이 의도된 동작이다(Close 버튼 경로만 draw_fn 내부에서 명시적
/// `Idle` 리셋 — 이 팝업은 close_on_outside_click=true 라 outside-click 경로가
/// 실제로 도달 가능하다).
#[test]
fn port_scanner_outside_click_close_preserves_scan_results() {
    let (mut state, mut engine) = test_state();
    state.port_scan = PortScanState::Ready {
        rows: vec![PortRowView {
            port: 8080,
            addr_display: "127.0.0.1".to_string(),
            pid: Some(1234),
            process_name: Some("node".to_string()),
            source: SourceTag::External,
            state: tasty_portscan::PortState::Listen,
            favorited: false,
        }],
        scope: crate::adapters::ui::popup::port_scanner::ScanScope::Tasty,
    };
    state
        .popups
        .open_at_focused(PORT_SCANNER_POPUP_ID, FIXED_POS);

    run_frame(
        press_input(outside_point(FIXED_POS)),
        &mut state,
        &mut engine,
    );

    assert!(!state.popups.is_open(PORT_SCANNER_POPUP_ID));
    match &state.port_scan {
        PortScanState::Ready { rows, .. } => assert_eq!(rows.len(), 1),
        _ => panic!("expected Ready to survive close, got a different state variant instead"),
    }
}

// ──────────────────────────── info_modal ────────────────────────────

fn info_modal_entry(body: &str) -> InfoModal {
    InfoModal {
        title: "Boot".to_string(),
        body: body.to_string(),
        on_close: InfoModalAction::Continue,
    }
}

/// `on_close` 훅 — X 버튼(draw_fn 을 거치지 않는 경로)으로 닫으면 head 가 pop 되고,
/// 남은 안내가 있으므로 팝업이 다시 열린다(transfer_error 의 재진입 패턴과 동형).
/// 훅 도입 전엔 head 가 pop 되지 않아 남은 큐가 영영 뜨지 않았다 — 그 버그의 회귀 방지.
#[test]
fn info_modal_close_intent_now_pops_head_and_reopens() {
    let (mut state, mut engine) = test_state();
    state
        .dialogs
        .info_modal_queue
        .push_back(info_modal_entry("a"));
    state
        .dialogs
        .info_modal_queue
        .push_back(info_modal_entry("b"));
    state.popups.open_at_focused(INFO_MODAL_ID, FIXED_POS);

    crate::intent::popup::handle(
        &mut state,
        &UiIntent::ClosePopup { id: INFO_MODAL_ID }.from_user_menu("test"),
    );
    assert!(!state.popups.is_open(INFO_MODAL_ID));
    // handle() 직후 — 아직 drain 전, 큐엔 여전히 2건.
    assert_eq!(state.dialogs.info_modal_queue.len(), 2);

    run_frame(empty_input(), &mut state, &mut engine);

    assert_eq!(state.dialogs.info_modal_queue.len(), 1);
    assert_eq!(state.dialogs.info_modal_queue.front().unwrap().body, "b");
    assert!(state.popups.is_open(INFO_MODAL_ID));
}

/// 큐에 1건뿐이면 pop 후 비므로 재오픈하지 않는다.
#[test]
fn info_modal_close_intent_with_single_entry_pops_and_does_not_reopen() {
    let (mut state, mut engine) = test_state();
    state
        .dialogs
        .info_modal_queue
        .push_back(info_modal_entry("only"));
    state.popups.open_at_focused(INFO_MODAL_ID, FIXED_POS);

    crate::intent::popup::handle(
        &mut state,
        &UiIntent::ClosePopup { id: INFO_MODAL_ID }.from_user_menu("test"),
    );
    run_frame(empty_input(), &mut state, &mut engine);

    assert!(state.dialogs.info_modal_queue.is_empty());
    assert!(!state.popups.is_open(INFO_MODAL_ID));
}

// ────────────────────────── confirm_delete_category ──────────────────────────

#[test]
fn confirm_delete_category_escape_close_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    let cat_id = engine.create_category("Services").unwrap();
    state.dialogs.pending_category_delete = Some(cat_id);
    state
        .popups
        .open_at_focused(CONFIRM_DELETE_CATEGORY_POPUP_ID, FIXED_POS);

    run_frame(key_input(egui::Key::Escape), &mut state, &mut engine);

    assert!(!state.popups.is_open(CONFIRM_DELETE_CATEGORY_POPUP_ID));
    assert!(state.dialogs.pending_category_delete.is_none());
}

#[test]
fn confirm_delete_category_outside_click_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    let cat_id = engine.create_category("Services").unwrap();
    state.dialogs.pending_category_delete = Some(cat_id);
    state
        .popups
        .open_at_focused(CONFIRM_DELETE_CATEGORY_POPUP_ID, FIXED_POS);

    run_frame(
        press_input(outside_point(FIXED_POS)),
        &mut state,
        &mut engine,
    );

    assert!(!state.popups.is_open(CONFIRM_DELETE_CATEGORY_POPUP_ID));
    assert!(state.dialogs.pending_category_delete.is_none());
}

/// `on_close` 훅 이관 후 — `UiIntent::ClosePopup` 경로도 다음 프레임의 drain 을
/// 거쳐 뒷정리가 돈다(`close_intent_now_clears_cleanup_after_next_frame` 과 동일 패턴).
#[test]
fn confirm_delete_category_close_intent_now_clears_dialog_state() {
    let (mut state, mut engine) = test_state();
    let cat_id = engine.create_category("Services").unwrap();
    state.dialogs.pending_category_delete = Some(cat_id);
    state
        .popups
        .open_at_focused(CONFIRM_DELETE_CATEGORY_POPUP_ID, FIXED_POS);

    crate::intent::popup::handle(
        &mut state,
        &UiIntent::ClosePopup {
            id: CONFIRM_DELETE_CATEGORY_POPUP_ID,
        }
        .from_user_menu("test"),
    );
    assert!(!state.popups.is_open(CONFIRM_DELETE_CATEGORY_POPUP_ID));
    assert!(state.dialogs.pending_category_delete.is_some());

    run_frame(empty_input(), &mut state, &mut engine);

    assert!(state.dialogs.pending_category_delete.is_none());
}

// ────────────────────────── file_handler_picker ──────────────────────────

fn mk_picker_data() -> FileHandlerPickerData {
    FileHandlerPickerData {
        target: crate::file::format::FileTarget::new("/tmp/popup-close-test.txt"),
        target_display: "/tmp/popup-close-test.txt".to_string(),
        detector: None,
        candidates: vec![],
        candidates_are_fallback: false,
        recent: vec![],
        selected: None,
        result: None,
        ignore_size_limit: false,
    }
}

#[test]
fn file_handler_picker_escape_close_marks_cancelled() {
    let (mut state, mut engine) = test_state();
    state.dialogs.file_handler_picker = Some(mk_picker_data());
    state.popups.open_at_focused(PICKER_POPUP_ID, FIXED_POS);

    run_frame(key_input(egui::Key::Escape), &mut state, &mut engine);

    assert!(!state.popups.is_open(PICKER_POPUP_ID));
    assert!(matches!(
        state.dialogs.file_handler_picker.as_ref().unwrap().result,
        Some(FileHandlerPickerResult::Cancelled)
    ));
}

#[test]
fn file_handler_picker_x_button_close_marks_cancelled() {
    let (mut state, mut engine) = test_state();
    state.dialogs.file_handler_picker = Some(mk_picker_data());
    state.popups.open_at_focused(PICKER_POPUP_ID, FIXED_POS);
    let (pos, size) = primed_popup_geometry(PICKER_POPUP_ID, &mut state, &mut engine);

    run_frame(
        press_input(close_button_point(pos, size)),
        &mut state,
        &mut engine,
    );

    assert!(!state.popups.is_open(PICKER_POPUP_ID));
    assert!(matches!(
        state.dialogs.file_handler_picker.as_ref().unwrap().result,
        Some(FileHandlerPickerResult::Cancelled)
    ));
}

/// `on_close` 훅 이관 후 — `UiIntent::ClosePopup` 경로도 다음 프레임의 drain 을
/// 거쳐 뒷정리가 돈다(`close_intent_now_clears_cleanup_after_next_frame` 과 동일 패턴).
#[test]
fn file_handler_picker_close_intent_now_marks_cancelled() {
    let (mut state, mut engine) = test_state();
    state.dialogs.file_handler_picker = Some(mk_picker_data());
    state.popups.open_at_focused(PICKER_POPUP_ID, FIXED_POS);

    crate::intent::popup::handle(
        &mut state,
        &UiIntent::ClosePopup {
            id: PICKER_POPUP_ID,
        }
        .from_user_menu("test"),
    );
    assert!(!state.popups.is_open(PICKER_POPUP_ID));
    assert!(
        state
            .dialogs
            .file_handler_picker
            .as_ref()
            .unwrap()
            .result
            .is_none()
    );

    run_frame(empty_input(), &mut state, &mut engine);

    assert!(matches!(
        state.dialogs.file_handler_picker.as_ref().unwrap().result,
        Some(FileHandlerPickerResult::Cancelled)
    ));
}

// ────────────────────────────── file_picker ──────────────────────────────

fn mk_file_picker_data() -> FilePickerData {
    FilePickerData {
        mirror_ws_id: None,
        remote_host: None,
        current_dir: "/tmp".to_string(),
        load: FpLoadState::Loaded,
        entries: vec![],
        selected: vec![],
        result: None,
        requester: None,
        filters: vec![],
    }
}

#[test]
fn file_picker_escape_close_marks_cancelled() {
    let (mut state, mut engine) = test_state();
    state.dialogs.file_picker = Some(mk_file_picker_data());
    state
        .popups
        .open_at_focused(FILE_PICKER_POPUP_ID, FIXED_POS);

    run_frame(key_input(egui::Key::Escape), &mut state, &mut engine);

    assert!(!state.popups.is_open(FILE_PICKER_POPUP_ID));
    assert!(matches!(
        state.dialogs.file_picker.as_ref().unwrap().result,
        Some(FilePickerResult::Cancelled)
    ));
}

#[test]
fn file_picker_x_button_close_marks_cancelled() {
    let (mut state, mut engine) = test_state();
    state.dialogs.file_picker = Some(mk_file_picker_data());
    state
        .popups
        .open_at_focused(FILE_PICKER_POPUP_ID, FIXED_POS);
    let (pos, size) = primed_popup_geometry(FILE_PICKER_POPUP_ID, &mut state, &mut engine);

    run_frame(
        press_input(close_button_point(pos, size)),
        &mut state,
        &mut engine,
    );

    assert!(!state.popups.is_open(FILE_PICKER_POPUP_ID));
    assert!(matches!(
        state.dialogs.file_picker.as_ref().unwrap().result,
        Some(FilePickerResult::Cancelled)
    ));
}

/// `on_close` 훅 이관 후 — `UiIntent::ClosePopup` 경로도 다음 프레임의 drain 을
/// 거쳐 뒷정리가 돈다(`close_intent_now_clears_cleanup_after_next_frame` 과 동일 패턴).
#[test]
fn file_picker_close_intent_now_marks_cancelled() {
    let (mut state, mut engine) = test_state();
    state.dialogs.file_picker = Some(mk_file_picker_data());
    state
        .popups
        .open_at_focused(FILE_PICKER_POPUP_ID, FIXED_POS);

    crate::intent::popup::handle(
        &mut state,
        &UiIntent::ClosePopup {
            id: FILE_PICKER_POPUP_ID,
        }
        .from_user_menu("test"),
    );
    assert!(!state.popups.is_open(FILE_PICKER_POPUP_ID));
    assert!(state.dialogs.file_picker.as_ref().unwrap().result.is_none());

    run_frame(empty_input(), &mut state, &mut engine);

    assert!(matches!(
        state.dialogs.file_picker.as_ref().unwrap().result,
        Some(FilePickerResult::Cancelled)
    ));
}

// ──────────────────────────────── approval ────────────────────────────────

fn push_approval(
    engine: &mut crate::core::CoreState,
    state: &mut crate::state::AppState,
    id: &str,
) {
    let record = engine
        .approval_store
        .request(ApprovalRequest {
            id: ApprovalId(id.to_string()),
            requester: Requester::Plugin {
                id: "test".to_string(),
            },
            workspace_id: None,
            surface_id: None,
            title: "Test approval".to_string(),
            body: None,
            choices: vec![],
            default_choice: None,
            timeout_ms: None,
            severity: Severity::Info,
            created_at: 1,
            metadata: serde_json::Value::Null,
        })
        .expect("approval request");
    state
        .dialogs
        .pending_approval_ids
        .push_back(record.record.request.id.clone());
}

#[test]
fn approval_empty_queue_close_clears_comment_buffer() {
    let (mut state, mut engine) = test_state();
    state.dialogs.approval_comment_buffer = "draft comment".to_string();
    state.popups.open_at_focused(APPROVAL_POPUP_ID, FIXED_POS);
    // pending_approval_ids 를 비워둔 채로 열림 — draw_fn 이 즉시 Close 반환(path 1).

    run_frame(empty_input(), &mut state, &mut engine);

    assert!(!state.popups.is_open(APPROVAL_POPUP_ID));
    assert!(state.dialogs.approval_comment_buffer.is_empty());
}

#[test]
fn approval_x_button_close_with_empty_queue_does_not_refire() {
    let (mut state, mut engine) = test_state();
    state.dialogs.approval_comment_buffer = "draft comment".to_string();
    state.popups.open_at_focused(APPROVAL_POPUP_ID, FIXED_POS);
    push_approval(&mut engine, &mut state, "req-1");
    let (pos, size) = primed_popup_geometry(APPROVAL_POPUP_ID, &mut state, &mut engine);
    state.dialogs.pending_approval_ids.clear();

    run_frame(
        press_input(close_button_point(pos, size)),
        &mut state,
        &mut engine,
    );

    assert!(!state.popups.is_open(APPROVAL_POPUP_ID));
    assert!(state.dialogs.approval_comment_buffer.is_empty());
    let pending = state.take_pending_intents();
    assert!(
        !pending
            .iter()
            .any(|d| matches!(&d.body, Intent::Ui(UiIntent::OpenPopup { id, .. }) if *id == APPROVAL_POPUP_ID)),
        "empty queue must not refire OpenPopup"
    );
}

/// X 버튼으로 닫혔는데 큐에 응답 대기 항목이 남아 있으면 다음 head 를 위해
/// OpenPopup intent 가 재발화한다 — TODO 40 체크리스트의 "approval 재발화 테스트".
#[test]
fn approval_x_button_close_with_pending_queue_refires_open_popup() {
    let (mut state, mut engine) = test_state();
    push_approval(&mut engine, &mut state, "req-1");
    push_approval(&mut engine, &mut state, "req-2");
    state.popups.open_at_focused(APPROVAL_POPUP_ID, FIXED_POS);
    let (pos, size) = primed_popup_geometry(APPROVAL_POPUP_ID, &mut state, &mut engine);

    run_frame(
        press_input(close_button_point(pos, size)),
        &mut state,
        &mut engine,
    );

    // X 버튼 경로는 draw_fn(큐 head pop 로직)을 거치지 않으므로 큐 자체는 그대로.
    assert_eq!(state.dialogs.pending_approval_ids.len(), 2);
    let pending = state.take_pending_intents();
    assert!(
        pending
            .iter()
            .any(|d| matches!(&d.body, Intent::Ui(UiIntent::OpenPopup { id, .. }) if *id == APPROVAL_POPUP_ID)),
        "non-empty queue must refire OpenPopup for the next head"
    );
}

/// `on_close` 훅 이관 후 — `UiIntent::ClosePopup` 경로로 닫아도(X 버튼과 동일하게
/// draw_fn 을 거치지 않으므로) 다음 프레임의 drain 이 재발화 intent 를 dispatch
/// 한다(`close_intent_now_clears_cleanup_after_next_frame` 과 동일 패턴 + 재진입
/// 재발화 확인).
#[test]
fn approval_close_intent_now_refires_open_popup() {
    let (mut state, mut engine) = test_state();
    push_approval(&mut engine, &mut state, "req-1");
    push_approval(&mut engine, &mut state, "req-2");
    state.popups.open_at_focused(APPROVAL_POPUP_ID, FIXED_POS);

    crate::intent::popup::handle(
        &mut state,
        &UiIntent::ClosePopup {
            id: APPROVAL_POPUP_ID,
        }
        .from_user_menu("test"),
    );
    assert!(!state.popups.is_open(APPROVAL_POPUP_ID));
    // handle() 직후 — 아직 drain 전, 재발화 intent 도 아직 없다.
    assert!(state.take_pending_intents().is_empty());

    run_frame(empty_input(), &mut state, &mut engine);

    assert_eq!(state.dialogs.pending_approval_ids.len(), 2);
    let pending = state.take_pending_intents();
    assert!(
        pending
            .iter()
            .any(|d| matches!(&d.body, Intent::Ui(UiIntent::OpenPopup { id, .. }) if *id == APPROVAL_POPUP_ID)),
        "non-empty queue must refire OpenPopup for the next head"
    );
}

// ────────────────────── path 3 (`UiIntent::ClosePopup`) ──────────────────────
// `rename` 은 on_close 훅으로 이관됐다 — `state.popups.close()` 가 큐에 쌓고,
// 다음 프레임의 drain(`draw_popups` → `drain_on_close_hooks`)이 훅을 발화한다.
// 인텐트 핸들러 자체는 drain 하지 않으므로 handle() 직후엔 아직 안 비워진다
// (큐잉과 drain 이 분리된 설계의 자연스러운 결과 — 버그 아님).
#[test]
fn close_intent_now_clears_cleanup_after_next_frame() {
    let (mut state, mut engine) = test_state();
    state.dialogs.rename = Some((RenameTarget::NewCategory, "abc".to_string()));
    state.popups.open_at_focused(RENAME_POPUP_ID, FIXED_POS);

    let dispatched = UiIntent::ClosePopup {
        id: RENAME_POPUP_ID,
    }
    .from_user_menu("test");
    crate::intent::popup::handle(&mut state, &dispatched);

    assert!(!state.popups.is_open(RENAME_POPUP_ID));
    // handle() 직후 — 아직 drain 전, 큐에만 쌓여 있다.
    assert!(state.dialogs.rename.is_some());

    run_frame(empty_input(), &mut state, &mut engine);

    // 다음 프레임의 drain 이 훅을 발화 — 뒷정리 완료.
    assert!(state.dialogs.rename.is_none());
}

// ─────────────────────────── preset_apply (버그 재현) ───────────────────────────
// X 버튼/외부 클릭(둘 다 draw_fn 의 Cancel 액션을 거치지 않음)으로 닫으면
// `preset_apply_target_category`/`preset_picker_selected` 가 남는 버그의 재현.
// 3개 팝업(workspace/tab/pane) 모두 같은 on_close 훅을 쓰므로 대표로
// APPLY_WORKSPACE_POPUP_ID 하나만 검증한다.

#[test]
fn preset_apply_x_button_close_clears_selection_and_target_category() {
    let (mut state, mut engine) = test_state();
    let cat_id = engine.categories()[0].id;
    state.dialogs.preset_apply_target_category = Some(cat_id);
    state.dialogs.preset_picker_selected = Some("my-preset".to_string());
    state
        .popups
        .open_at_focused(APPLY_WORKSPACE_POPUP_ID, FIXED_POS);
    let (pos, size) = primed_popup_geometry(APPLY_WORKSPACE_POPUP_ID, &mut state, &mut engine);

    run_frame(
        press_input(close_button_point(pos, size)),
        &mut state,
        &mut engine,
    );

    assert!(!state.popups.is_open(APPLY_WORKSPACE_POPUP_ID));
    assert!(state.dialogs.preset_apply_target_category.is_none());
    assert!(state.dialogs.preset_picker_selected.is_none());
}

#[test]
fn preset_apply_outside_click_close_clears_selection_and_target_category() {
    let (mut state, mut engine) = test_state();
    let cat_id = engine.categories()[0].id;
    state.dialogs.preset_apply_target_category = Some(cat_id);
    state.dialogs.preset_picker_selected = Some("my-preset".to_string());
    state
        .popups
        .open_at_focused(APPLY_WORKSPACE_POPUP_ID, FIXED_POS);

    run_frame(
        press_input(outside_point(FIXED_POS)),
        &mut state,
        &mut engine,
    );

    assert!(!state.popups.is_open(APPLY_WORKSPACE_POPUP_ID));
    assert!(state.dialogs.preset_apply_target_category.is_none());
    assert!(state.dialogs.preset_picker_selected.is_none());
}

/// Cancel 액션 경로 — 훅 이관 전에도 이미 동작하던 경로라 회귀가 없는지 확인.
#[test]
fn preset_apply_cancel_action_close_clears_selection_and_target_category() {
    let (mut state, mut engine) = test_state();
    let cat_id = engine.categories()[0].id;
    state.dialogs.preset_apply_target_category = Some(cat_id);
    state.dialogs.preset_picker_selected = Some("my-preset".to_string());
    state
        .popups
        .open_at_focused(APPLY_WORKSPACE_POPUP_ID, FIXED_POS);

    run_frame(key_input(egui::Key::Escape), &mut state, &mut engine);

    assert!(!state.popups.is_open(APPLY_WORKSPACE_POPUP_ID));
    assert!(state.dialogs.preset_apply_target_category.is_none());
    assert!(state.dialogs.preset_picker_selected.is_none());
}
