//! IME (Input Method Editor) handling — OS별 분리.
//!
//! OS마다 winit의 IME 이벤트 모델이 다르다:
//! - **macOS (NSTextInputClient)**: composition 세션당 `Enabled` 1회, 조합 중 `Preedit` 여러 번,
//!   마지막에 `Commit`, 세션 종료 시 `Disabled`. 즉 `Disabled`는 실제 IME OFF 시그널.
//! - **Windows (IMM/TSF)**: **매 글자마다** `Enabled` → `Preedit(...)` ... → `Preedit("")`
//!   → `Commit(...)` → `Disabled` 사이클. `Disabled`는 "이번 글자 composition 종료"일 뿐,
//!   IME 전체 상태 리셋이 아니다.
//! - **Linux (ibus / fcitx 등 text-input)**: Windows와 유사하게 매 commit 전후로 빈 `Preedit("")`이
//!   여러 번 발사된다. 즉 빈 Preedit을 "세션 종료"로 해석하면 안 되며, advance 보정 상태(advance/base)는
//!   유지하고 preedit overlay만 비워야 한다.
//!
//! 이 차이 때문에 "composition 종료(빈 Preedit, Disabled)에서 advance 보정 상태를 완전히
//! 리셋할지"가 OS별로 달라져야 한다. 그 외 로직(reconcile, commit 시 advance 누적 등)은
//! 공통이다.
//!
//! PTY 에코 지연 보정에 쓰이는 두 상태:
//! - `ime_cursor_advance`: 마지막 Commit 이후 전송된 문자들의 누적 display width.
//!   PTY 에코가 반영되기 전까지 이 offset만큼 anchor를 앞으로 밀어준다.
//! - `ime_advance_base`: advance가 마지막으로 갱신된 시점의 raw cursor 위치.
//!   이후 raw cursor가 이 위치를 지나갔다면 그만큼 advance를 차감한다.

use tasty_plugin_protocol::ImeWire;
use winit::event::Ime;

use super::MainView;
use crate::core::intent::{DomainIntent, SendPayload};
use crate::gpu::ImePreeditState;
use crate::view::ui::View as _;

/// IME 입력의 preedit/commit text 를 Intent 큐로 보낸다. surface_id 가
/// None 이거나 text 가 비어있으면 no-op.
fn dispatch_send_text(w: &mut MainView, surface_id: Option<u32>, text: &str) {
    let Some(sid) = surface_id else { return };
    if text.is_empty() {
        return;
    }
    w.state.dispatch_intent(
        DomainIntent::SendToSurface {
            surface_id: sid,
            payload: SendPayload::Text(text.to_string()),
        }
        .from_user_shortcut("ime_commit"),
    );
}

// =============================================================================
// Public entry points
// =============================================================================

pub(super) fn handle_event(w: &mut MainView, event: Ime, egui_consumed: bool) {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    if egui_consumed {
        w.mark_dirty();
        return;
    }

    // 팝업/오버레이가 열려 있으면 IME 이벤트를 터미널로 전달하지 않는다.
    // Enabled/Disabled는 IME 상태 추적용이므로 허용하고, Preedit/Commit만 차단.
    // 키 게이트와 같은 단일 출처 — plugin egui-mesh popup 이 열려 있을 때도 IME
    // Preedit/Commit 이 터미널로 새면 안 된다(그 조합은 popup 이 받아야 한다).
    let overlay_open = w.state.keyboard_overlay_open();
    if overlay_open {
        match event {
            Ime::Enabled => {
                w.ime_active = true;
            }
            Ime::Disabled => {
                w.ime_active = false;
            }
            Ime::Preedit(..) | Ime::Commit(..) => {}
        }
        w.mark_dirty();
        return;
    }

    // 포커스가 egui-mesh surface(markdown/image 등)면 IME 를 plugin 으로 forward 해
    // 라이브 preedit 을 그 surface 의 egui TextEdit 이 인라인 표시하게 한다(터미널
    // overlay 경로 대신). commit-only 가 아니라 조합 중 preedit 문자열도 나른다.
    if let Some(sid) = w.focused_egui_mesh_surface_id() {
        forward_ime_to_egui_mesh(w, sid, event);
        w.mark_dirty();
        return;
    }
    // attach mesh mirror surface — 위와 동형이되 목적지가 원격.
    if let Some(sid) = w.focused_attach_mesh_surface_id() {
        forward_ime_to_attach_mesh(w, sid, event);
        w.mark_dirty();
        return;
    }

    match event {
        Ime::Enabled => w.ime_active = true,
        Ime::Disabled => on_disabled(w),
        Ime::Preedit(text, cursor) => on_preedit(w, text, cursor),
        Ime::Commit(text) => on_commit(w, text),
    }
}

/// winit IME 이벤트를 egui-mesh surface 로 forward. terminal overlay 상태
/// (`ime_preedit`/advance)는 건드리지 않고, `ime_active` 만 갱신한다 — keyboard.rs 의
/// Text 억제 판정([`super::keyboard`])이 이 플래그를 읽기 때문.
fn forward_ime_to_egui_mesh(w: &mut MainView, surface_id: u32, event: Ime) {
    let wire = match event {
        Ime::Enabled => {
            w.ime_active = true;
            ImeWire::Enabled
        }
        Ime::Disabled => {
            w.ime_active = false;
            ImeWire::Disabled
        }
        // winit preedit 의 cursor byte-range 는 egui `ImeEvent::Preedit(String)` 이
        // 담지 않으므로 문자열만 나른다(candidate 위치는 host 가 별도 관리).
        Ime::Preedit(text, _cursor) => ImeWire::Preedit { text },
        Ime::Commit(text) => ImeWire::Commit { text },
    };
    w.egui_mesh_push_ime(surface_id, wire);
}

/// [`forward_ime_to_egui_mesh`]의 attach mesh mirror 대응 — 목적지가 원격
/// plugin 이라는 점만 다르다.
fn forward_ime_to_attach_mesh(w: &mut MainView, surface_id: u32, event: Ime) {
    let wire = match event {
        Ime::Enabled => {
            w.ime_active = true;
            ImeWire::Enabled
        }
        Ime::Disabled => {
            w.ime_active = false;
            ImeWire::Disabled
        }
        Ime::Preedit(text, _cursor) => ImeWire::Preedit { text },
        Ime::Commit(text) => ImeWire::Commit { text },
    };
    w.attach_mesh_push_ime(surface_id, wire);
}

/// PTY 출력이 도착해 terminal cursor(또는 TUI의 fake cursor)가 움직였을 수 있을
/// 때 호출. advance가 차감되어 0이 되거나, fake cursor가 최신 위치로 갱신된 순간을
/// 포착해 preedit anchor를 재계산한다.
pub(super) fn recalc_anchor(w: &mut MainView) {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    if w.ime_cursor_advance == 0 {
        return;
    }
    let Some(preedit) = &w.ime_preedit else {
        return;
    };
    let surface_id = preedit.surface_id;
    let Some(terminal) = w.core_state.find_terminal_by_id(surface_id) else {
        return;
    };

    let (col, row) = reference_cursor(terminal);
    let cols = terminal.cols();
    let (base_col, base_row) = w.ime_advance_base;
    let raw_advance = compute_raw_advance(col, row, base_col, base_row, cols);

    if raw_advance >= w.ime_cursor_advance {
        w.ime_cursor_advance = 0;
    } else {
        w.ime_cursor_advance -= raw_advance;
    }
    w.ime_advance_base = (col, row);

    let (anchor_col, anchor_row) = advanced_anchor(col, row, cols, w.ime_cursor_advance);
    if let Some(p) = &mut w.ime_preedit {
        p.anchor_col = anchor_col;
        p.anchor_row = anchor_row;
    }
}

/// 현재 preedit이 있으면 확정해서 PTY로 보낸다 (단축키 소비 전 호출).
pub(super) fn flush_preedit(w: &mut MainView) {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    let preedit = match w.ime_preedit.take() {
        Some(p) if !p.text.is_empty() => p,
        _ => {
            w.ime_cursor_advance = 0;
            w.ime_advance_base = (0, 0);
            return;
        }
    };
    dispatch_send_text(w, Some(preedit.surface_id), &preedit.text);
    w.core_state.record_typing(preedit.surface_id);
    w.ime_cursor_advance = 0;
    w.ime_advance_base = (0, 0);
    w.mark_dirty();
}

/// 현재 preedit을 PTY로 보내지 않고 버린다.
/// 팝업/오버레이가 열릴 때 조합 중 문자가 터미널로 전달되지 않도록 사용.
pub(super) fn clear_preedit(w: &mut MainView) {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    w.ime_preedit = None;
    w.ime_cursor_advance = 0;
    w.ime_advance_base = (0, 0);
    w.mark_dirty();
}

/// 완전 리셋 — composition 세션 종료(`Disabled`/`Preedit("")`)에서 advance까지 0으로 미는 경로.
/// macOS만 호출한다 (Windows/Linux는 매 글자마다 빈 시그널이 들어와 advance를 보존해야 함).
#[cfg(target_os = "macos")]
fn clear_all(w: &mut MainView) {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    w.ime_preedit = None;
    w.ime_cursor_advance = 0;
    w.ime_advance_base = (0, 0);
}

// =============================================================================
// IPC helpers — debug/automation용. OS 분기 없이 macOS 모델(full session)로 동작.
// =============================================================================

pub(crate) fn ipc_set_preedit(
    w: &mut MainView,
    text: String,
    cursor: Option<(usize, usize)>,
) -> Option<(usize, usize, u32)> {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    let surface_id = w.state.focused_surface_id(engine)?;
    let (col, row, cols) = {
        let terminal = w.state.focused_terminal(engine)?;
        // Snapshot cursor and cols under one state lock so they share a
        // generation (ADR-0002).
        terminal.with_surface(|s| {
            let (col, row) = s.cursor_position();
            let (cols, _) = s.dimensions();
            (col, row, cols)
        })
    };

    if w.ime_cursor_advance > 0 {
        let (base_col, base_row) = w.ime_advance_base;
        let raw_advance = compute_raw_advance(col, row, base_col, base_row, cols);
        if raw_advance >= w.ime_cursor_advance {
            w.ime_cursor_advance = 0;
        } else {
            w.ime_cursor_advance -= raw_advance;
        }
        w.ime_advance_base = (col, row);
    }

    let (anchor_col, anchor_row) = advanced_anchor(col, row, cols, w.ime_cursor_advance);
    w.ime_preedit = Some(ImePreeditState {
        text,
        cursor,
        anchor_col,
        anchor_row,
        surface_id,
    });
    w.update_ime_cursor_area();
    w.mark_dirty();
    Some((anchor_col, anchor_row, surface_id))
}

pub(crate) fn ipc_commit(w: &mut MainView, text: &str) {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    if w.ime_cursor_advance == 0
        && let Some(terminal) = w.state.focused_terminal(engine)
    {
        w.ime_advance_base = terminal.cursor_position();
    }
    for ch in text.chars() {
        w.ime_cursor_advance += crate::renderer::unicode_width(ch);
    }
    w.ime_preedit = None;
    let sid = w.state.focused_surface_id(engine);
    dispatch_send_text(w, sid, text);
    if let Some(sid) = sid {
        w.core_state.record_typing(sid);
    }
    w.mark_dirty();
}

// =============================================================================
// Event handlers
// =============================================================================

/// composition 종료(`Ime::Disabled`, `Preedit("")`). OS별 분기의 유일한 지점.
///
/// Windows / Linux: 매 글자마다 이 시그널이 오므로 preedit overlay만 지우고 advance/base는
/// 다음 글자의 PTY 에코 보정을 위해 유지한다.
/// macOS: 실제 IME 세션 종료이므로 advance/base까지 완전 리셋.
fn on_composition_end(w: &mut MainView) {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    #[cfg(windows)]
    {
        w.ime_preedit = None;
    }
    #[cfg(target_os = "linux")]
    {
        w.ime_preedit = None;
    }
    #[cfg(target_os = "macos")]
    {
        clear_all(w);
    }
}

fn on_disabled(w: &mut MainView) {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    w.ime_active = false;
    on_composition_end(w);
}

fn on_preedit(w: &mut MainView, text: String, cursor: Option<(usize, usize)>) {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    if text.is_empty() {
        on_composition_end(w);
        w.mark_dirty();
        return;
    }

    let surface_id = w.state.focused_surface_id(engine);
    let anchor = reconcile_and_compute_anchor(w);

    w.ime_preedit = match (surface_id, anchor) {
        (Some(sid), Some((anchor_col, anchor_row))) => Some(ImePreeditState {
            text,
            cursor,
            anchor_col,
            anchor_row,
            surface_id: sid,
        }),
        _ => None,
    };
    w.update_ime_cursor_area();
    w.mark_dirty();
}

fn on_commit(w: &mut MainView, text: String) {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    if w.ime_cursor_advance == 0
        && let Some(terminal) = w.state.focused_terminal(engine)
    {
        w.ime_advance_base = reference_cursor(terminal);
    }
    for ch in text.chars() {
        w.ime_cursor_advance += crate::renderer::unicode_width(ch);
    }
    w.ime_preedit = None;

    let sid = w.state.focused_surface_id(engine);
    dispatch_send_text(w, sid, &text);
    if let Some(sid) = sid {
        w.core_state.record_typing(sid);
    }
    w.mark_dirty();
}

// =============================================================================
// Shared helpers
// =============================================================================

fn compute_raw_advance(
    col: usize,
    row: usize,
    base_col: usize,
    base_row: usize,
    cols: usize,
) -> usize {
    if row > base_row {
        (row - base_row) * cols + col.saturating_sub(base_col)
    } else {
        col.saturating_sub(base_col)
    }
}

fn advanced_anchor(col: usize, row: usize, cols: usize, advance: usize) -> (usize, usize) {
    let adjusted_col = col + advance;
    if cols > 0 && adjusted_col >= cols {
        (adjusted_col % cols, row + adjusted_col / cols)
    } else {
        (adjusted_col, row)
    }
}

fn reconcile_and_compute_anchor(w: &mut MainView) -> Option<(usize, usize)> {
    let engine = &mut w.core_state;
    let _ = &mut *engine; // engine alias: 일부 분기/cfg 에서 미사용 — reborrow 로 unused 경고 억제(값 drop, Result 아님).
    let terminal = w.state.focused_terminal(engine)?;
    let cols = terminal.cols();

    // 참조 좌표 선택: TUI(cursor 숨김 + 단일 reverse-video 셀)면 fake cursor,
    // 아니면 실제 terminal cursor.
    let (ref_col, ref_row) = reference_cursor(terminal);

    if w.ime_cursor_advance > 0 {
        let (base_col, base_row) = w.ime_advance_base;
        let raw_advance = compute_raw_advance(ref_col, ref_row, base_col, base_row, cols);
        if raw_advance >= w.ime_cursor_advance {
            w.ime_cursor_advance = 0;
        } else {
            w.ime_cursor_advance -= raw_advance;
        }
        w.ime_advance_base = (ref_col, ref_row);
    }

    Some(advanced_anchor(
        ref_col,
        ref_row,
        cols,
        w.ime_cursor_advance,
    ))
}

/// Preedit/commit 모두가 사용하는 "입력 위치" 좌표.
/// Ink 기반 TUI가 `\e[?25l`로 real cursor를 숨기고 `\e[7m`으로 그린 fake cursor가
/// 있으면 그걸 우선 사용. 없으면 real cursor.
fn reference_cursor(terminal: &tasty_terminal::Terminal) -> (usize, usize) {
    if !terminal.cursor_visible()
        && let Some(fake) = terminal.find_fake_cursor_cell()
    {
        return fake;
    }
    terminal.cursor_position()
}
