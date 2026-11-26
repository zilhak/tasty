//! 목록 이동 키의 순수 계산. 행 포커스가 어느 행으로 가고 목록 view 가 얼마나 스크롤하는지를 egui 없이 정한다.
//! 규칙의 정본은 tasty-page spec `row-focus-and-list-keys`(start · ↑ ↓ · Home · End · PageUp · PageDown · scroll ·
//! inert rows)다.

/// 행 포커스를 옮기는 키.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MoveKey {
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// 목록 view. 열 머리 아래 스크롤 영역의 위치와 높이다.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ListView {
    /// 스크롤 위치(맨 위 0).
    pub(super) offset: f32,
    /// 보이는 높이.
    pub(super) height: f32,
    /// 행 높이(모든 행이 같다).
    pub(super) row_h: f32,
}

/// 이동 결과: 새 행 포커스 index 와 그 행을 보이게 하는 스크롤 위치.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Moved {
    pub(super) focus: usize,
    pub(super) offset: f32,
}

/// 행 경계 비교 허용치(px). 부동소수 스크롤 위치가 행 경계에 거의 닿은 경우를 닿은 것으로 본다.
const EPS: f32 = 0.5;

impl ListView {
    fn max_offset(&self, rows: usize) -> f32 {
        (rows as f32 * self.row_h - self.height).max(0.0)
    }

    fn at(self, offset: f32, rows: usize) -> Self {
        Self {
            offset: offset.clamp(0.0, self.max_offset(rows)),
            ..self
        }
    }

    /// 온전히 보이는 행 범위(양끝 포함). 한 행도 온전히 들어가지 않으면 None.
    fn fully_visible(&self, rows: usize) -> Option<(usize, usize)> {
        if rows == 0 || self.row_h <= 0.0 {
            return None;
        }
        let first = ((self.offset - EPS) / self.row_h).ceil().max(0.0) as usize;
        let end = ((self.offset + self.height + EPS) / self.row_h).floor() as usize;
        let last = end.min(rows).checked_sub(1)?;
        (first <= last).then_some((first, last))
    }

    /// 한 페이지 = view 에 들어가는 행 수 − 1(최소 1).
    fn page_rows(&self) -> usize {
        ((((self.height + EPS) / self.row_h).floor() as usize).saturating_sub(1)).max(1)
    }
}

/// `index` 행 전체가 보이도록 가장 적게 움직인 스크롤 위치. 이미 보이면 그대로다.
pub(super) fn reveal(view: ListView, rows: usize, index: usize) -> f32 {
    let top = index as f32 * view.row_h;
    let bottom = top + view.row_h;
    let offset = if top < view.offset - EPS {
        top
    } else if bottom > view.offset + view.height + EPS {
        bottom - view.height
    } else {
        view.offset
    };
    offset.clamp(0.0, view.max_offset(rows))
}

/// 행 포커스를 `key` 로 옮긴다. `selectable[i]` 가 거짓인 행(폴더 모드 파일)은 모든 키가 건너뛴다.
/// 고를 수 있는 행이 없으면 None. 행 포커스가 view 밖이면 먼저 최소 거리로 보이게 한 뒤 그 행에서 동작한다.
pub(super) fn step(
    selectable: &[bool],
    focus: Option<usize>,
    view: ListView,
    key: MoveKey,
) -> Option<Moved> {
    let rows = selectable.len();
    let first = selectable.iter().position(|s| *s)?;
    let last = selectable.iter().rposition(|s| *s)?;
    let Some(from) = focus.filter(|i| *i < rows) else {
        let focus = match key {
            MoveKey::Down | MoveKey::PageDown | MoveKey::Home => first,
            MoveKey::Up | MoveKey::PageUp | MoveKey::End => last,
        };
        return Some(Moved {
            focus,
            offset: reveal(view, rows, focus),
        });
    };
    let view = view.at(reveal(view, rows, from), rows);
    let next_after = |i: usize| (i + 1..rows).find(|j| selectable[*j]);
    let prev_before = |i: usize| (0..i).rev().find(|j| selectable[*j]);
    let (focus, view) = match key {
        MoveKey::Down => (next_after(from).unwrap_or(from), view),
        MoveKey::Up => (prev_before(from).unwrap_or(from), view),
        MoveKey::Home => (first, view),
        MoveKey::End => (last, view),
        MoveKey::PageDown => page_down(selectable, from, view),
        MoveKey::PageUp => page_up(selectable, from, view),
    };
    Some(Moved {
        focus,
        offset: reveal(view, rows, focus),
    })
}

/// PageDown: 보이는 마지막 행 앞이면 그 행으로, 이미 그 행이면 한 페이지 내려 새 마지막 행으로. 끝에서는 멈춘다.
fn page_down(selectable: &[bool], from: usize, view: ListView) -> (usize, ListView) {
    let rows = selectable.len();
    let pick = |view: ListView| {
        let (_, last) = view.fully_visible(rows)?;
        (from + 1..=last).rev().find(|j| selectable[*j])
    };
    if let Some(i) = pick(view) {
        return (i, view);
    }
    let paged = view.at(view.offset + view.page_rows() as f32 * view.row_h, rows);
    match pick(paged) {
        Some(i) => (i, paged),
        None => match (from + 1..rows).find(|j| selectable[*j]) {
            Some(i) => (i, paged),
            None => (from, view),
        },
    }
}

/// PageUp: PageDown 의 대칭. 보이는 첫 행 뒤면 그 행으로, 이미 그 행이면 한 페이지 올려 새 첫 행으로.
fn page_up(selectable: &[bool], from: usize, view: ListView) -> (usize, ListView) {
    let rows = selectable.len();
    let pick = |view: ListView| {
        let (first, _) = view.fully_visible(rows)?;
        (first..from).find(|j| selectable[*j])
    };
    if let Some(i) = pick(view) {
        return (i, view);
    }
    let paged = view.at(view.offset - view.page_rows() as f32 * view.row_h, rows);
    match pick(paged) {
        Some(i) => (i, paged),
        None => match (0..from).rev().find(|j| selectable[*j]) {
            Some(i) => (i, paged),
            None => (from, view),
        },
    }
}

/// 피커 목록 키 하나의 조작.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ListKey {
    /// 상위 폴더(Alt+↑).
    Parent,
    /// 확정(Enter).
    Confirm,
    /// 행 포커스 폴더로 들어가기(→).
    Into,
    /// 행 포커스 옮기기.
    Move(MoveKey),
    /// 행 포커스 행 고르기(Space).
    Pick,
    /// 상위 폴더(Backspace).
    Back,
}

/// 피커 노드 키맵. 위에서부터 맞춘다(Alt+↑ 가 ↑ 보다 먼저). 매처는 egui `matches_logically` 라 패턴에 없는
/// Shift·Alt 가 붙은 누름도 같은 조작이다. 세 모드가 같은 표를 쓴다.
const KEYMAP: [(egui::Modifiers, egui::Key, ListKey); 11] = [
    (egui::Modifiers::ALT, egui::Key::ArrowUp, ListKey::Parent),
    (egui::Modifiers::NONE, egui::Key::Enter, ListKey::Confirm),
    (egui::Modifiers::NONE, egui::Key::ArrowRight, ListKey::Into),
    (
        egui::Modifiers::NONE,
        egui::Key::ArrowDown,
        ListKey::Move(MoveKey::Down),
    ),
    (
        egui::Modifiers::NONE,
        egui::Key::ArrowUp,
        ListKey::Move(MoveKey::Up),
    ),
    (
        egui::Modifiers::NONE,
        egui::Key::Home,
        ListKey::Move(MoveKey::Home),
    ),
    (
        egui::Modifiers::NONE,
        egui::Key::End,
        ListKey::Move(MoveKey::End),
    ),
    (
        egui::Modifiers::NONE,
        egui::Key::PageUp,
        ListKey::Move(MoveKey::PageUp),
    ),
    (
        egui::Modifiers::NONE,
        egui::Key::PageDown,
        ListKey::Move(MoveKey::PageDown),
    ),
    (egui::Modifiers::NONE, egui::Key::Space, ListKey::Pick),
    (egui::Modifiers::NONE, egui::Key::Backspace, ListKey::Back),
];

/// 누름 하나의 피커 조작.
pub(super) fn classify(modifiers: egui::Modifiers, key: egui::Key) -> Option<ListKey> {
    KEYMAP
        .iter()
        .find(|(pattern, k, _)| *k == key && modifiers.matches_logically(*pattern))
        .map(|(_, _, op)| *op)
}

/// 키 누름 하나가 피커 노드 키맵에 맞는지. 창 키 경로가 피커 노드에 줄지 정할 때 쓴다. 모드·payload 와 무관하다.
pub(crate) fn list_keymap_receives(modifiers: egui::Modifiers, key: egui::Key) -> bool {
    classify(modifiers, key).is_some()
}

/// 그리기 전에 아는, 키 몫을 가르는 값. 위젯 id 는 직전 프레임에 그린 값이다.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct WidgetIds {
    /// 저장 모드 이름 칸.
    pub(super) name_field: Option<egui::Id>,
    /// 경로 막대 `…` 메뉴 버튼과 그 메뉴의 popup id.
    pub(super) menu_button: Option<egui::Id>,
    pub(super) menu: Option<egui::Id>,
    /// 직전 프레임에 피커가 화살표를 가져갈 때의 위젯 포커스. egui 가 그 화살표로 옮긴 포커스를 되돌린다.
    pub(super) focus_hold: Option<egui::Id>,
}

impl WidgetIds {
    fn key(ui: &egui::Ui) -> egui::Id {
        ui.id().with("file_picker_widget_ids")
    }

    /// 직전 프레임에 저장한 값.
    pub(super) fn load(ui: &egui::Ui) -> Self {
        ui.ctx()
            .data(|d| d.get_temp::<Self>(Self::key(ui)))
            .unwrap_or_default()
    }

    /// 이번 프레임 값을 다음 프레임에 쓰게 둔다.
    pub(super) fn store(self, ui: &egui::Ui) {
        ui.ctx().data_mut(|d| d.insert_temp(Self::key(ui), self));
    }
}

/// 확정(primary) 버튼이 지금 활성인지. footer 가 그리는 버튼과 Enter 가 같은 판정을 쓴다.
pub(super) fn confirm_enabled(p: &super::FilePickerProps<'_>) -> bool {
    use super::{FilePickerMode, FpViewState};
    match p.mode {
        FilePickerMode::Folder { .. } => {
            matches!(p.state, FpViewState::Loaded | FpViewState::Empty)
        }
        // 폴더 하나면 그 안으로 이동하고, 그 외에는 선택한 항목이 모두 파일이어야 한다.
        FilePickerMode::Open { .. } => {
            matches!(p.state, FpViewState::Loaded)
                && !p.selected.is_empty()
                && (super::selected_folder(p).is_some()
                    || p.selected.iter().all(|name| {
                        p.entries
                            .iter()
                            .find(|e| &e.name == name)
                            .is_some_and(|e| !e.is_dir)
                    }))
        }
        FilePickerMode::Save { can_confirm, .. } => can_confirm,
    }
}

/// 그리기 전에 이번 패스 피커 몫 키를 egui 입력에서 빼서 도착 순서대로 돌려준다. 몫 판정은 spec priority 순서다.
/// - 이 인스턴스의 `…` 메뉴가 열려 있으면 메뉴가 모든 키를 갖는다: `…` 버튼이 포커스면 그 버튼이 메뉴를 다시 토글하지
///   않게 Enter·Space 를 빼서 버리고, 나머지는 손대지 않는다. 다른 egui 팝업이 열려 있으면 아무것도 빼지 않는다.
/// - Enter: `…` 버튼이 포커스면 위젯 몫(egui 가 메뉴를 연다)이고 그 뒤 키는 빼지 않는다(메뉴가 열린다). 그 밖에는 확정이
///   활성이거나, 이름 칸이 포커스이거나(칸이 Enter 로 포커스를 놓지 않게), 같은 패스에 앞선 행 키가 있으면 피커 몫이다.
///   확정 가능 여부는 적용 시점에 다시 본다. 셋 다 아니면 위젯 몫(egui 가 Tab 포커스 버튼을 누른다).
/// - Space 는 위젯 포커스가 있으면 위젯 몫. 저장 이름 칸이 포커스면 Space·Backspace·Home·End·→ 는 칸 몫.
/// - 나머지는 피커 몫이라 TextEdit·Button 이 보지 않는다.
///
/// 직전 프레임에 피커가 화살표를 가져갔는데 egui 가 패스 끝에 위젯 포커스를 옮겼으면(`Memory::end_pass` 방향 이동)
/// 그리기 전에 원래 위젯으로 되돌린다. 이번 몫이 이 인스턴스에 결속되지 않았으면 아무것도 하지 않는다.
pub(super) fn take_keys(
    ui: &egui::Ui,
    p: &super::FilePickerProps<'_>,
    keys: &mut crate::adapters::ui::popup::keys::PopupKeys,
    ids: WidgetIds,
) -> Vec<ListKey> {
    if !keys.receives() || !keys.routed_by_window() {
        return Vec::new();
    }
    if let Some(hold) = ids.focus_hold {
        ui.memory_mut(|m| {
            if m.focused() != Some(hold) {
                m.request_focus(hold);
            }
        });
    }
    let focused = ui.memory(|m| m.focused());
    let text_focus = focused.is_some() && focused == ids.name_field;
    let menu_button_focus = focused.is_some() && focused == ids.menu_button;
    let menu_open = ids
        .menu
        .is_some_and(|id| ui.memory(|m| m.is_popup_open(id)));
    if menu_open {
        // 메뉴가 열린 동안 부모 위젯(`…` 버튼·이름 칸·버튼)은 목록 키를 읽지 않는다. 뺀 누름은 버린다.
        let dropped = keys.take_keymap_presses(classify, |_| true);
        drop(dropped);
        return Vec::new();
    }
    if ui.memory(|m| m.any_popup_open()) {
        return Vec::new();
    }
    let confirm = confirm_enabled(p);
    // 앞선 행 키가 선택을 바꿔 확정을 켤 수 있는지. 목록이 Loaded 이고 고를 수 있는 행이 있을 때, 이동은 선택이 따라가는
    // 모드(열기·폴더)에서, Space 는 모든 모드에서다.
    let inert_files = matches!(p.mode, super::FilePickerMode::Folder { .. });
    let rows_ready = matches!(p.state, super::FpViewState::Loaded)
        && p.entries.iter().any(|e| e.is_dir || !inert_files);
    let moves_select = !matches!(p.mode, super::FilePickerMode::Save { .. });
    let mut row_key_before = false;
    let mut menu_opening = false;
    keys.take_keymap_presses(classify, |op| {
        if menu_opening {
            return false;
        }
        let own = match op {
            ListKey::Confirm if menu_button_focus => {
                menu_opening = true;
                false
            }
            ListKey::Confirm => confirm || text_focus || row_key_before,
            ListKey::Pick => focused.is_none(),
            ListKey::Back | ListKey::Into | ListKey::Move(MoveKey::Home | MoveKey::End) => {
                !text_focus
            }
            ListKey::Parent | ListKey::Move(_) => true,
        };
        row_key_before |= own
            && rows_ready
            && match op {
                ListKey::Move(_) => moves_select,
                ListKey::Pick => true,
                _ => false,
            };
        own
    })
}

/// 이번 패스 화살표가 모두 피커 몫(↑ ↓ → Alt+↑)이었으면 그린 뒤의 위젯 포커스를 다음 프레임에 지킬 값으로 돌려준다.
/// egui 는 패스의 마지막 화살표 방향으로 패스 끝에 포커스를 옮기므로, 위젯 몫 화살표(←, 이름 칸의 →)가 남았으면 그
/// 이동은 위젯 몫이라 지키지 않는다. `keys` 는 피커 몫을 뺀 뒤의 통로다.
pub(super) fn focus_to_hold(
    ui: &egui::Ui,
    owned: &[ListKey],
    keys: &crate::adapters::ui::popup::keys::PopupKeys,
) -> Option<egui::Id> {
    let arrow = owned.iter().any(|op| {
        matches!(
            op,
            ListKey::Into | ListKey::Move(MoveKey::Up | MoveKey::Down) | ListKey::Parent
        )
    });
    (arrow && !keys.has_arrow_press())
        .then(|| ui.memory(|m| m.focused()))
        .flatten()
}

/// 그린 뒤 피커 몫 키를 도착 순서대로 조작으로 바꾼다. `view` 는 이번 프레임에 그린 목록 view(목록을 그렸을 때만).
/// 행 키(이동·Space·→)는 목록이 Loaded 일 때만 쓴다. 디렉터리를 옮기거나 확정하는 조작 뒤 키는 버린다.
/// `focus_row` 는 이번 프레임 그리기 조작(클릭)을 반영한 행 포커스다. 같은 패스의 연속 키는 앞 키가 정한 행 포커스와
/// 스크롤 위치에서 이어 계산한다.
pub(super) fn actions(
    p: &super::FilePickerProps<'_>,
    focus_row: Option<&str>,
    owned: &[ListKey],
    view: Option<ListView>,
) -> Vec<super::FilePickerAction> {
    use super::{FilePickerAction as A, FilePickerMode, FpViewState};
    let inert = matches!(p.mode, FilePickerMode::Folder { .. });
    let selectable: Vec<bool> = p.entries.iter().map(|e| !inert || e.is_dir).collect();
    let select_moves = !matches!(p.mode, FilePickerMode::Save { .. });
    let mut focus = focus_row
        .and_then(|name| p.entries.iter().position(|e| e.name == name))
        .filter(|i| selectable[*i]);
    let mut view = view.filter(|_| matches!(p.state, FpViewState::Loaded));
    let mut out = Vec::new();
    for op in owned {
        match *op {
            ListKey::Parent | ListKey::Back => {
                // 상위 폴더가 없으면(루트) 옮기지 않으므로 뒤 키를 이어 읽는다.
                if p.up_enabled && p.crumbs.len() > 1 {
                    out.push(A::NavigateUp);
                    break;
                }
            }
            ListKey::Confirm => {
                out.push(A::Confirm);
                break;
            }
            ListKey::Move(key) => {
                let Some(v) = view else { continue };
                if let Some(m) = step(&selectable, focus, v, key) {
                    // 끝에서 멈춘 이동은 행 포커스·선택을 다시 쓰지 않고 링과 스크롤만 갱신한다.
                    out.push(if focus == Some(m.focus) {
                        A::ShowRing { scroll: m.offset }
                    } else {
                        A::FocusRow {
                            name: p.entries[m.focus].name.clone(),
                            select: select_moves,
                            scroll: m.offset,
                        }
                    });
                    focus = Some(m.focus);
                    view = Some(ListView {
                        offset: m.offset,
                        ..v
                    });
                }
            }
            ListKey::Pick => {
                let (Some(v), Some(i)) = (view, focus) else {
                    continue;
                };
                let offset = reveal(v, p.entries.len(), i);
                out.push(A::PickRow {
                    name: p.entries[i].name.clone(),
                    scroll: offset,
                });
                view = Some(ListView { offset, ..v });
            }
            ListKey::Into => {
                let (Some(v), Some(i)) = (view, focus) else {
                    continue;
                };
                if p.entries[i].is_dir {
                    out.push(A::NavigateInto(p.entries[i].name.clone()));
                    break;
                }
                let offset = reveal(v, p.entries.len(), i);
                out.push(A::ShowRing { scroll: offset });
                view = Some(ListView { offset, ..v });
            }
        }
    }
    out
}

#[cfg(test)]
#[path = "list_keys_tests.rs"]
mod tests;
