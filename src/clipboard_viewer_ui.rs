//! 클립보드 히스토리 뷰어의 공유 렌더링 로직. Popup과 Surface 모두에서 동일한
//! UI를 보여주기 위해 한 함수를 재사용한다.

use crate::clipboard_history::{ClipboardEntry, ClipboardHistory, ClipboardSource};
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::PopupAction;

/// 표시 텍스트 길이 상한(UI용, 전체 텍스트는 tooltip에서 확인).
const PREVIEW_MAX_CHARS: usize = 200;
/// 검색 매칭 시 비교하는 최대 문자수. 너무 긴 텍스트의 검색 비용 방어.
const SEARCH_MATCH_MAX_CHARS: usize = 1000;

/// Popup/Surface 양쪽에서 유지하는 뷰어 상태.
#[derive(Debug, Default, Clone)]
pub struct ClipboardViewerState {
    /// 검색어. 빈 문자열이면 전체 표시.
    pub search: String,
    /// 키보드 선택 인덱스 (필터된 결과 기준).
    pub selected: Option<usize>,
    /// 전체 비우기 확인 대기 플래그.
    pub pending_clear: bool,
}

struct ViewerConfig {
    /// Popup 모드에서 "Esc로 닫기" 등 안내 힌트를 추가로 보여줄지.
    show_close_hint: bool,
}

/// 공통 draw 결과. 호출자가 외부 동작으로 변환한다.
enum ViewerOutcome {
    None,
    Close,
    Pasted,
}

/// 검색어/선택 초기화 후 뷰어 popup을 중앙에 연다.
pub fn open_clipboard_viewer_popup(state: &mut AppState) {
    state.dialogs.clipboard_viewer = ClipboardViewerState::default();
    state.popups.open_centered_focused("clipboard_viewer");
}

/// Popup 래퍼. `PopupDef::draw_fn`로 등록.
pub fn draw_clipboard_viewer_popup(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    let outcome = draw_inner(
        ui,
        &mut state.engine.clipboard_history,
        &mut state.dialogs.clipboard_viewer,
        ViewerConfig { show_close_hint: true },
    );
    if let ViewerOutcome::Pasted = outcome {
        let index = state.dialogs.clipboard_viewer.selected.unwrap_or(0);
        paste_from_history(state, index);
        return PopupAction::Close;
    }
    if matches!(outcome, ViewerOutcome::Close) {
        return PopupAction::Close;
    }
    PopupAction::None
}

/// Surface 래퍼. 닫기 개념 없음.
pub fn draw_clipboard_viewer_surface(
    ui: &mut egui::Ui,
    history: &mut ClipboardHistory,
    viewer: &mut ClipboardViewerState,
) -> Option<usize> {
    let outcome = draw_inner(
        ui,
        history,
        viewer,
        ViewerConfig { show_close_hint: false },
    );
    if let ViewerOutcome::Pasted = outcome {
        Some(viewer.selected.unwrap_or(0))
    } else {
        None
    }
}

/// 시스템 클립보드에 해당 index의 항목을 복사하고 `Internal`로 재기록한다.
pub fn paste_from_history(state: &mut AppState, filtered_orig_index: usize) {
    let text = match state.engine.clipboard_history.get(filtered_orig_index) {
        Some(e) => e.text.clone(),
        None => return,
    };
    match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(text.clone())) {
        Ok(()) => {
            state.engine.record_internal_copy(&text);
        }
        Err(e) => {
            tracing::warn!("clipboard set_text failed: {e}");
        }
    }
}

fn draw_inner(
    ui: &mut egui::Ui,
    history: &mut ClipboardHistory,
    viewer: &mut ClipboardViewerState,
    config: ViewerConfig,
) -> ViewerOutcome {
    let th = theme::theme();
    let ctx = ui.ctx().clone();
    let margin = 8.0;
    let inner = ui.available_rect_before_wrap().shrink2(egui::vec2(margin, 0.0));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    let ui = &mut child;

    // ── 검색 입력 ──
    let resp = ui.add(
        egui::TextEdit::singleline(&mut viewer.search)
            .hint_text(t("clipboard_viewer.search_placeholder"))
            .desired_width(f32::INFINITY),
    );
    if !resp.has_focus() && viewer.search.is_empty() {
        resp.request_focus();
    }
    ui.add_space(4.0);

    // ── 필터 (history에서 필요한 정보를 소유 복사하여 이후 mut borrow와 충돌 회피) ──
    #[derive(Clone)]
    struct Row {
        orig: usize,
        text: String,
        source: ClipboardSource,
        captured_at: std::time::Instant,
    }
    let total = history.len();
    let owned: Vec<Row> = history
        .entries()
        .enumerate()
        .map(|(i, e)| Row {
            orig: i,
            text: e.text.clone(),
            source: e.source,
            captured_at: e.captured_at,
        })
        .collect();
    let filtered: Vec<&Row> = if viewer.search.is_empty() {
        owned.iter().collect()
    } else {
        let q = viewer.search.to_lowercase();
        owned
            .iter()
            .filter(|r| {
                let capped: String = r.text.chars().take(SEARCH_MATCH_MAX_CHARS).collect();
                capped.to_lowercase().contains(&q)
            })
            .collect()
    };

    // 카운터
    if !viewer.search.is_empty() {
        let msg = crate::i18n::t_fmt2(
            "clipboard_viewer.result_counter",
            &filtered.len().to_string(),
            &total.to_string(),
        );
        ui.label(egui::RichText::new(msg).small().color(th.subtext0));
        ui.add_space(2.0);
    }

    // ── 키보드 처리 ──
    let mut outcome = ViewerOutcome::None;
    let filtered_len = filtered.len();
    ctx.input(|i| {
        if i.key_pressed(egui::Key::Escape) {
            if !viewer.search.is_empty() {
                viewer.search.clear();
                viewer.selected = None;
            } else if config.show_close_hint {
                outcome = ViewerOutcome::Close;
            }
        }
        if i.key_pressed(egui::Key::ArrowDown) && filtered_len > 0 {
            viewer.selected = Some(match viewer.selected {
                None => 0,
                Some(n) => (n + 1).min(filtered_len - 1),
            });
        }
        if i.key_pressed(egui::Key::ArrowUp) && filtered_len > 0 {
            viewer.selected = Some(match viewer.selected {
                None => 0,
                Some(n) => n.saturating_sub(1),
            });
        }
        if i.key_pressed(egui::Key::Home) && filtered_len > 0 {
            viewer.selected = Some(0);
        }
        if i.key_pressed(egui::Key::End) && filtered_len > 0 {
            viewer.selected = Some(filtered_len - 1);
        }
        if (i.key_pressed(egui::Key::Enter)
            || (i.key_pressed(egui::Key::C) && i.modifiers.ctrl))
            && filtered_len > 0
        {
            let n = viewer.selected.unwrap_or(0);
            if let Some(row) = filtered.get(n) {
                viewer.selected = Some(row.orig);
                outcome = ViewerOutcome::Pasted;
            }
        }
    });

    // Delete 처리 (history mut borrow).
    let (delete_pressed, shift_delete) = ctx.input(|i| (
        i.key_pressed(egui::Key::Delete),
        i.key_pressed(egui::Key::Delete) && i.modifiers.shift,
    ));
    let delete_target: Option<usize> = if delete_pressed && !shift_delete && filtered_len > 0 {
        viewer.selected.and_then(|n| filtered.get(n).map(|r| r.orig))
    } else {
        None
    };
    if shift_delete {
        viewer.pending_clear = true;
    }
    if let Some(orig) = delete_target {
        history.remove_at(orig);
        viewer.selected = None;
    }

    // ── 목록 ──
    let is_history_empty = history.is_empty();
    let mut paste_orig: Option<usize> = None;
    if is_history_empty {
        ui.add_space(12.0);
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new(t("clipboard_viewer.empty_message"))
                    .color(th.subtext0),
            );
        });
    } else if filtered.is_empty() {
        ui.add_space(12.0);
        ui.centered_and_justified(|ui| {
            ui.label(
                egui::RichText::new(t("clipboard_viewer.no_match"))
                    .color(th.subtext0),
            );
        });
    } else {
        // 힌트 바 영역을 위한 여유 공간
        let bottom_h = if config.show_close_hint { 26.0 } else { 0.0 };
        let list_h = (ui.available_height() - bottom_h - 4.0).max(60.0);
        egui::ScrollArea::vertical()
            .max_height(list_h)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (pos, row) in filtered.iter().enumerate() {
                    let is_selected = viewer.selected == Some(pos);
                    let row_h = 26.0;
                    let (rect, resp) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), row_h),
                        egui::Sense::click(),
                    );
                    if is_selected || resp.hovered() {
                        ui.painter().rect_filled(rect, 2.0, th.hover_overlay);
                    }
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }

                    let badge = source_badge(row.source);
                    let time = format_relative_time(row.captured_at);
                    let prefix = format!("{}  {:<4}  ", badge, time);
                    let preview = first_line_preview(&row.text);

                    ui.painter().text(
                        egui::pos2(rect.min.x + 4.0, rect.center().y - th.font_size_body / 2.0),
                        egui::Align2::LEFT_TOP,
                        format!("{}{}", prefix, preview),
                        egui::FontId::proportional(th.font_size_body),
                        th.text,
                    );
                    resp.clone().on_hover_text(&row.text);

                    if resp.clicked() {
                        viewer.selected = Some(pos);
                        if config.show_close_hint {
                            paste_orig = Some(row.orig);
                        }
                    }
                    if resp.double_clicked() {
                        paste_orig = Some(row.orig);
                    }
                }
            });
    }
    if let Some(orig) = paste_orig {
        viewer.selected = Some(orig);
        outcome = ViewerOutcome::Pasted;
    }

    // ── 전체 비우기 확인 바 ──
    if viewer.pending_clear {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(t("clipboard_viewer.clear_confirm"))
                    .color(th.yellow),
            );
            if ui.button(t("button.ok")).clicked() {
                history.clear();
                viewer.pending_clear = false;
                viewer.selected = None;
            }
            if ui.button(t("button.cancel")).clicked() {
                viewer.pending_clear = false;
            }
        });
    }

    // ── 하단 힌트 (Popup 전용) ──
    if config.show_close_hint {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(t("clipboard_viewer.hint_bar"))
                .small()
                .color(th.overlay1),
        );
    }

    outcome
}

/// 첫 줄을 추출 + 프리뷰 최대 길이로 자르고 `…` 추가.
fn first_line_preview(text: &str) -> String {
    let line = text.lines().next().unwrap_or("");
    truncate_chars(line, PREVIEW_MAX_CHARS)
}

fn truncate_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    format!("{}\u{2026}", truncated)
}

fn source_badge(src: ClipboardSource) -> &'static str {
    match src {
        ClipboardSource::System => "[S]",
        ClipboardSource::Internal => "[I]",
    }
}

/// "5s" / "3m" / "2h" / "4d" 스타일로 포맷.
pub fn format_relative_time(captured_at: std::time::Instant) -> String {
    let elapsed = captured_at.elapsed();
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

/// 히스토리를 검색어로 필터. 반환 `(원본 인덱스, 엔트리 참조)`.
pub fn filter_entries<'a>(
    entries: &'a [&'a ClipboardEntry],
    query: &str,
) -> Vec<(usize, &'a ClipboardEntry)> {
    if query.is_empty() {
        return entries.iter().enumerate().map(|(i, e)| (i, *e)).collect();
    }
    let q = query.to_lowercase();
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| {
            let capped: String = e.text.chars().take(SEARCH_MATCH_MAX_CHARS).collect();
            capped.to_lowercase().contains(&q)
        })
        .map(|(i, e)| (i, *e))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard_history::{ClipboardEntry, ClipboardSource};
    use std::time::Instant;

    fn make_entry(s: &str) -> ClipboardEntry {
        ClipboardEntry {
            text: s.to_string(),
            captured_at: Instant::now(),
            source: ClipboardSource::System,
        }
    }

    #[test]
    fn filter_empty_query_returns_all() {
        let a = make_entry("hello");
        let b = make_entry("world");
        let v = vec![&a, &b];
        let result = filter_entries(&v, "");
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].0, 0);
        assert_eq!(result[1].0, 1);
    }

    #[test]
    fn filter_case_insensitive() {
        let a = make_entry("Hello World");
        let b = make_entry("RUST");
        let v = vec![&a, &b];
        let result = filter_entries(&v, "world");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.text, "Hello World");

        let result = filter_entries(&v, "RuSt");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].1.text, "RUST");
    }

    #[test]
    fn filter_preserves_original_index() {
        let a = make_entry("apple");
        let b = make_entry("banana");
        let c = make_entry("cherry");
        let v = vec![&a, &b, &c];
        let result = filter_entries(&v, "rry");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, 2); // cherry는 원본 인덱스 2
    }

    #[test]
    fn truncate_chars_short_unchanged() {
        assert_eq!(truncate_chars("hi", 10), "hi");
    }

    #[test]
    fn truncate_chars_long_adds_ellipsis() {
        let s = truncate_chars("0123456789", 5);
        assert_eq!(s, "01234\u{2026}");
    }

    #[test]
    fn truncate_chars_unicode() {
        // 한글 5자 + 영문 3자 = 8 chars
        let s = truncate_chars("가나다라마abc", 5);
        assert_eq!(s, "가나다라마\u{2026}");
    }
}
