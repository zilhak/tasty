//! 터미널 링크 우클릭 메뉴 — 수식키 hover 링크 위 우클릭에서 "선택 / 복사 / 연결 동작".
//!
//! 트리거는 사용자 마우스 조작 하나다(release 배선, IPC/CLI 비노출 — identity §1).
//! 대상은 드래그로 확정한 selection 이 아니라 hover 가 판정해 둔 `LinkSpan` 이다.
//! 명세: `docs/features/terminal-link/index.md`.

use winit::event::ElementState;

use super::MainView;
use crate::selection::{SelectionMode, SelectionPoint, TextSelection};
use crate::state::{PendingNativeMenu, TerminalLinkMenu};
use crate::terminal_link::LinkSegment;
use crate::view::ui::View;
use tasty_type_geometry::length::PhysicalPx;

/// 메뉴 항목 id.
const ITEM_SELECT: u32 = 1;
const ITEM_COPY: u32 = 2;
const ITEM_OPEN_WITH: u32 = 3;

impl MainView {
    /// 우클릭한 surface 에서 링크 메뉴를 띄울 hover 링크가 있으면 그 스냅샷을 만든다.
    ///
    /// `LinkModifier::None` 이어도 막지 않는다 — `hovered_link` 는 이미 수식키 게이트를
    /// 통과한 결과이고, 좌클릭이 `None` 을 배제하는 이유("수식키 없는 클릭이 링크를
    /// **열어버리는** 사고")가 우클릭 → 메뉴 → 항목 선택이라는 명시적 3단 조작에는
    /// 성립하지 않는다.
    ///
    /// 포커스는 옮기지 않는다 — 대상은 id 로 스냅샷에 담기고, 컨텍스트 메뉴는 포커스를
    /// 건드리지 않는 관례다(좌클릭 링크 경로의 focus 호출을 따라 붙이지 않는다).
    pub(super) fn terminal_link_menu_target(&self, surface_id: u32) -> Option<TerminalLinkMenu> {
        let hovered = self.hovered_link.as_ref();
        if !link_menu_gate(
            hovered.map(|h| h.surface_id),
            surface_id,
            self.core_state.attach.is_hard_occupied(surface_id),
        ) {
            return None;
        }
        let hovered = hovered?;
        let (start, end) = link_selection_bounds(&hovered.highlight.segments)?;
        let terminal = self.core_state.visible_terminal(surface_id)?;
        let text = crate::selection::extract_selected_text(
            terminal,
            &link_selection(surface_id, start, end),
        );
        // 자식 PTY 가 없는 terminal 은 원격 attach mirror 다 — 화면 경로가 원격 호스트 경로.
        let is_mirror = self
            .core_state
            .find_terminal_by_id(surface_id)
            .is_some_and(|t| t.process_id().is_none());
        let (open_with, remote_path) = link_open_target(&hovered.uri, is_mirror);
        Some(TerminalLinkMenu {
            surface_id,
            start,
            end,
            text,
            open_with,
            remote_path,
        })
    }

    /// 링크 스냅샷이 있는 우클릭 사이클을 로컬 소비한다. 메뉴는 플랫폼별 버튼 상태
    /// (`terminal_menu_open_state`)에서만 세우고, 반대쪽 상태도 tracking 앱에 보고하지
    /// 않는다 — press 만 로컬 소비하고 release 를 앱에 흘리면 앱이 짝 없는 release 를 받는다.
    pub(super) fn queue_terminal_link_menu(
        &mut self,
        link: TerminalLinkMenu,
        button_state: ElementState,
        x: f32,
        y: f32,
    ) {
        if button_state == super::mouse::terminal_menu_open_state() {
            let sf = self.base.gpu.scale_factor();
            self.state.dialogs.pending_native_menu = Some(PendingNativeMenu::TerminalLink {
                link,
                // 네이티브 메뉴 좌표는 logical — 물리 마우스 좌표를 변환 API 로 내린다.
                x: PhysicalPx(x).to_logical(sf).value(),
                y: PhysicalPx(y).to_logical(sf).value(),
            });
        }
        self.mark_dirty();
    }

    pub(super) fn handle_terminal_link_native_menu(
        &mut self,
        link: TerminalLinkMenu,
        x: f32,
        y: f32,
    ) {
        use crate::platform::native_menu::MenuItem;
        let items: Vec<MenuItem> = link_menu_items(link.open_with.is_some())
            .into_iter()
            .map(|(id, key)| MenuItem::new(id, crate::i18n::t(key)))
            .collect();
        self.open_native_menu(x, y, &items, move |this, result| {
            // Linux 는 continuation 이 여러 프레임 뒤다 — 그 사이 surface 가 닫혔을 수 있다.
            if !this.core_state.has_surface(link.surface_id) {
                return;
            }
            match result {
                Some(ITEM_SELECT) => this.apply_link_selection(&link),
                Some(ITEM_COPY) => {
                    if link.text.is_empty() {
                        return;
                    }
                    if let Some(cb) = &mut this.clipboard {
                        cb.set_text(&link.text);
                    }
                    this.state.toasts.push_info(
                        crate::i18n::t("toast.copied"),
                        crate::adapters::ui::ToastScope::Surface(link.surface_id),
                    );
                }
                Some(ITEM_OPEN_WITH) => this.open_link_handler_picker(&link),
                _ => {}
            }
        });
    }

    /// "선택" — 우클릭 시점의 범위를 selection 으로 세운다. 메뉴가 떠 있는 동안 새 출력·
    /// scrollback 트림·resize 로 같은 좌표가 다른 글자를 가리키게 됐으면 조용히 아무것도
    /// 안 한다. 판정은 같은 범위를 다시 추출해 스냅샷 문자열과 견주는 것이다 — "복사" 가
    /// 넣는 값과 "선택" 이 가리키는 값이 갈리지 않게 한다.
    fn apply_link_selection(&mut self, link: &TerminalLinkMenu) {
        let sel = link_selection(link.surface_id, link.start, link.end);
        let Some(terminal) = self.core_state.visible_terminal(link.surface_id) else {
            return;
        };
        if crate::selection::extract_selected_text(terminal, &sel) != link.text {
            return;
        }
        self.text_selection = Some(sel);
        self.mark_dirty();
    }

    /// "연결 동작" — 자동 1순위 실행을 건너뛰고 핸들러 picker 를 강제로 연다. 식별은 태우지
    /// 않고(detector 없음) 전체 핸들러를 fallback 후보로 싣는다 — 자동 경로의 empty-state 와
    /// 같은 화면이고 즉시 뜬다. 원격 경로는 후보도 recent 도 없는 빈 picker 다.
    fn open_link_handler_picker(&mut self, link: &TerminalLinkMenu) {
        let Some(target) = link.open_with.clone() else {
            return;
        };
        match target {
            crate::file::dispatch::DispatchTarget::File(file) if link.remote_path => {
                crate::file::dispatch::open_remote_placeholder_picker(&mut self.state, file);
            }
            target => {
                let all = self.core_state.file_handler.all_handlers();
                crate::file::dispatch::open_picker(
                    &mut self.state,
                    &mut self.core_state,
                    target,
                    None,
                    all,
                    true,
                    false,
                );
            }
        }
        self.mark_dirty();
    }
}

/// 링크 메뉴 게이트: hover 링크가 우클릭한 그 surface 의 것이고, 그 surface 가 hard 점유
/// mirror 가 아니어야 한다. hard 점유 화면은 최대 3초 지연된 스냅샷이라 보이는 링크가
/// 실제 PTY 상태와 다를 수 있다(ADR-0049 — 좌클릭 링크 오픈과 같은 배제).
fn link_menu_gate(hovered_surface: Option<u32>, clicked_surface: u32, hard_occupied: bool) -> bool {
    hovered_surface == Some(clicked_surface) && !hard_occupied
}

/// 링크 세그먼트(화면 순서, `end_col` 포함)를 selection 양끝으로. 선택 범위 판정도 양끝
/// 포함이라 그대로 맞물린다. 소프트 wrap 링크는 첫 세그먼트 시작 ~ 마지막 세그먼트 끝.
fn link_selection_bounds(segments: &[LinkSegment]) -> Option<(SelectionPoint, SelectionPoint)> {
    let first = segments.first()?;
    let last = segments.last()?;
    Some((
        SelectionPoint {
            col: first.start_col,
            absolute_row: first.absolute_row,
        },
        SelectionPoint {
            col: last.end_col,
            absolute_row: last.absolute_row,
        },
    ))
}

/// 드래그로 만든 선택과 구분되지 않는 selection.
fn link_selection(surface_id: u32, start: SelectionPoint, end: SelectionPoint) -> TextSelection {
    TextSelection {
        anchor: start,
        cursor: end,
        mode: SelectionMode::Normal,
        surface_id,
        dragging: false,
    }
}

/// 링크 URI → ("연결 동작" 대상, 원격 경로 여부). 경로 링크는 파일 대상, `http(s)` 는 URL
/// 대상, 그 밖의 scheme(mailto 등)은 열 핸들러가 없어 항목을 노출하지 않는다. URL 은
/// mirror 여부와 무관하다 — 원격 호스트 경로가 아니다.
fn link_open_target(
    uri: &str,
    is_mirror: bool,
) -> (Option<crate::file::dispatch::DispatchTarget>, bool) {
    use crate::file::dispatch::{DispatchTarget, LinkKind, parse_link};
    match parse_link(uri) {
        LinkKind::FileTarget(path) => (
            Some(DispatchTarget::File(crate::file::format::FileTarget::new(
                path,
            ))),
            is_mirror,
        ),
        LinkKind::External(uri) => (DispatchTarget::http_url(&uri), false),
    }
}

/// 메뉴 항목 (id, i18n 키). 기존 터미널 메뉴 항목은 싣지 않는다 — 링크 위 우클릭은 링크
/// 전용 메뉴로 대체된다.
fn link_menu_items(has_open_with: bool) -> Vec<(u32, &'static str)> {
    let mut items = vec![
        (ITEM_SELECT, "terminal_context_menu.link_select"),
        (ITEM_COPY, "terminal_context_menu.link_copy"),
    ];
    if has_open_with {
        items.push((ITEM_OPEN_WITH, "terminal_context_menu.link_open_with"));
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::dispatch::DispatchTarget;

    fn seg(absolute_row: usize, start_col: usize, end_col: usize) -> LinkSegment {
        LinkSegment {
            absolute_row,
            start_col,
            end_col,
        }
    }

    fn pt(col: usize, absolute_row: usize) -> SelectionPoint {
        SelectionPoint { col, absolute_row }
    }

    /// 좌클릭과 대칭 — 이 게이트는 tracking 모드를 보지 않는다. `handle_right_button` 은
    /// 이 판정을 `right_click_delegates_to_app` 보다 먼저 부른다.
    #[test]
    fn link_menu_wins_over_tracking_delegation() {
        assert!(link_menu_gate(Some(7), 7, false));
    }

    #[test]
    fn link_menu_skipped_when_hover_surface_differs() {
        assert!(!link_menu_gate(Some(8), 7, false));
        assert!(!link_menu_gate(None, 7, false));
    }

    #[test]
    fn link_menu_skipped_on_hard_occupied_mirror() {
        assert!(!link_menu_gate(Some(7), 7, true));
    }

    #[test]
    fn selection_from_single_segment_link_covers_exact_columns() {
        assert_eq!(
            link_selection_bounds(&[seg(5, 3, 20)]),
            Some((pt(3, 5), pt(20, 5)))
        );
    }

    #[test]
    fn selection_from_wrapped_link_spans_first_to_last_segment() {
        assert_eq!(
            link_selection_bounds(&[seg(5, 70, 79), seg(6, 0, 12)]),
            Some((pt(70, 5), pt(12, 6)))
        );
        assert_eq!(link_selection_bounds(&[]), None);
    }

    #[test]
    fn menu_items_hide_open_with_when_no_target() {
        let ids = |v: Vec<(u32, &str)>| v.into_iter().map(|(id, _)| id).collect::<Vec<_>>();
        assert_eq!(
            ids(link_menu_items(true)),
            vec![ITEM_SELECT, ITEM_COPY, ITEM_OPEN_WITH]
        );
        assert_eq!(ids(link_menu_items(false)), vec![ITEM_SELECT, ITEM_COPY]);
    }

    #[test]
    fn open_target_by_link_kind() {
        assert_eq!(
            link_open_target("https://example.com/page", false),
            (
                Some(DispatchTarget::Url("https://example.com/page".into())),
                false
            )
        );
        // URL 은 mirror 여도 원격 경로가 아니다.
        assert!(!link_open_target("https://example.com/page", true).1);
        assert_eq!(link_open_target("mailto:a@b.com", false), (None, false));
        let (target, remote) = link_open_target("file:///home/u/a.md", true);
        assert_eq!(
            target,
            Some(DispatchTarget::File(crate::file::format::FileTarget::new(
                "/home/u/a.md"
            )))
        );
        assert!(remote);
        assert!(!link_open_target("file:///home/u/a.md", false).1);
    }
}
