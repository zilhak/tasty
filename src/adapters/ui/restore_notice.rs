//! 창 오른쪽 위 안내 열에 띄우는 복원 실패·저장 멈춤 비포커스 안내의 창별 상태와 그리기.
//!
//! 안내 내용은 App이 넘긴다. 안내는 나타날 때 포커스를 가져가지 않고 scrim도 없다. 바깥의
//! 키·선택·스크롤은 터미널로 그대로 간다. ×·Continue·안쪽 컨트롤 포커스의 Esc는 같은 응답이며,
//! 안내를 지우고 그 id를 App이 확인 응답으로 가져가게 남긴다.

use crate::model::{LogicalPx, LogicalRect, PhysicalRect};

/// 안내 한 장의 표시 문구. 모두 번역된 문자열이다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RestoreNoticeEntry {
    pub(crate) id: u64,
    pub(crate) title: String,
    pub(crate) cause: String,
    pub(crate) label: String,
    pub(crate) path: String,
    pub(crate) body: String,
}

impl RestoreNoticeEntry {
    /// 같은 안내인지 가린다. 제목·원인·경로가 같으면 열에 다시 쌓지 않는다.
    fn same_as(&self, other: &Self) -> bool {
        self.title == other.title && self.cause == other.cause && self.path == other.path
    }
}

#[derive(Default)]
pub(crate) struct RestoreNotices {
    entries: Vec<RestoreNoticeEntry>,
    /// 사용자가 닫은 안내 id. App이 가져간다.
    acknowledged: Vec<u64>,
    /// 직전 프레임에 그린 안내 카드별 사각형(논리 좌표). 카드 사이 간격은 들어가지 않는다.
    /// native WebView가 이 영역만 그림과 포인터에서 빼서 안내가 보이게 한다.
    cards: Vec<crate::model::LogicalRect>,
    /// 직전 프레임에 안내 안쪽 컨트롤이 키보드 포커스를 가졌는지. 이때만 키를 egui로 보낸다.
    keyboard_inside: bool,
    /// 더한 뒤 아직 그리지 않은 안내가 있는지. 프레임을 그리지 않는 halt 중에 창을 다시 그리게 한다.
    undrawn: bool,
}

impl RestoreNotices {
    /// 안내를 더한다. 같은 안내가 이미 떠 있으면 더하지 않고 false다.
    pub(crate) fn push(&mut self, entry: RestoreNoticeEntry) -> bool {
        if self.entries.iter().any(|shown| shown.same_as(&entry)) {
            return false;
        }
        self.entries.push(entry);
        self.undrawn = true;
        true
    }

    /// 아직 그리지 않은 안내가 있는지.
    pub(crate) fn has_undrawn(&self) -> bool {
        self.undrawn
    }

    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    /// 직전 프레임에 그린 안내가 이 위치(논리 좌표)를 덮는지.
    pub(crate) fn covers(&self, pos: egui::Pos2) -> bool {
        let x = crate::model::LogicalPx(pos.x);
        let y = crate::model::LogicalPx(pos.y);
        self.cards.iter().any(|rect| {
            x >= rect.x && x <= rect.x + rect.width && y >= rect.y && y <= rect.y + rect.height
        })
    }

    pub(crate) fn take_acknowledged(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.acknowledged)
    }

    /// 직전 프레임에 그린 안내 카드들. 안내가 없으면 비어 있다.
    pub(crate) fn card_rects(&self) -> &[crate::model::LogicalRect] {
        &self.cards
    }

    /// 안내 안쪽 컨트롤이 키보드 포커스를 가졌는지. 안내가 나타나는 것만으로는 true가 되지 않는다.
    pub(crate) fn has_keyboard_focus(&self) -> bool {
        self.keyboard_inside
    }

    fn dismiss(&mut self, id: u64) {
        self.entries.retain(|entry| entry.id != id);
        self.acknowledged.push(id);
    }
}

/// 열이 시작할 y. 탭 바 아래(`base`)에서 시작하되, 열과 가로로 겹치고 열 시작점보다 위에서
/// 시작하는 장애물(창 범위 배너, 오른쪽 위 pane의 열린 검색 막대)이 있으면 그 아래로 내린다.
/// 장애물은 옮기지 않는다. 반환값에는 `notice_column_rect`가 inset을 더한다.
fn column_top(
    base: LogicalPx,
    inset: LogicalPx,
    columns: impl Fn(LogicalPx) -> LogicalRect,
    obstacles: &[LogicalRect],
) -> LogicalPx {
    let mut top = base;
    let mut sorted: Vec<LogicalRect> = obstacles.to_vec();
    sorted.sort_by(|a, b| a.y.value().total_cmp(&b.y.value()));
    for obstacle in sorted {
        let column = columns(top);
        let beside =
            obstacle.x + obstacle.width <= column.x || obstacle.x >= column.x + column.width;
        let bottom = obstacle.y + obstacle.height;
        if !beside && obstacle.y <= column.y + inset && bottom > top {
            top = bottom;
        }
    }
    top
}

/// egui가 돌려준 논리 사각형을 배치 계산용 타입으로 옮긴다.
fn logical_rect(rect: egui::Rect) -> LogicalRect {
    LogicalRect {
        x: LogicalPx(rect.min.x),
        y: LogicalPx(rect.min.y),
        width: LogicalPx(rect.width()),
        height: LogicalPx(rect.height()),
    }
}

/// 작업 영역(`terminal_rect`) 오른쪽 위, 탭 바 아래의 안내 열에 안내를 위에서부터 쌓아 그린다.
/// `obstacles`는 같은 프레임에 먼저 그린 창 범위 배너와 열린 검색 막대의 사각형이다. 열은 그 아래에서
/// 시작한다.
/// 이번 프레임에 그린 안내 전체 사각형(논리 좌표)을 돌려준다. 같은 열 아래에 이어 그리는 attach
/// 대기 칩은 이 값의 아래쪽 + `notice-column-gap`에서 시작한다. 안내가 없으면 `None`이다.
pub(crate) fn draw_notice_column(
    ctx: &egui::Context,
    notices: &mut RestoreNotices,
    terminal_rect: PhysicalRect,
    tab_bar_height: crate::model::PhysicalPx,
    scale: f32,
    obstacles: &[egui::Rect],
) -> Option<egui::Rect> {
    let theme = crate::theme::theme();
    let work = super::to_egui_rect(terminal_rect, scale);
    let base = LogicalPx(work.top()) + tab_bar_height.to_logical(scale);
    let columns = |top: LogicalPx| tasty_ui_widgets::notice_column_rect(&theme, work, top.value());
    let obstacles: Vec<LogicalRect> = obstacles.iter().copied().map(logical_rect).collect();
    let top = column_top(
        base,
        theme.notice_column_inset(),
        |top| logical_rect(columns(top)),
        &obstacles,
    );
    let column = columns(top);
    let gap = theme.notice_column_gap().value();
    let mut next_top = column.top();
    let mut occupied: Option<egui::Rect> = None;
    let mut cards = Vec::new();
    let mut dismissed = Vec::new();
    let mut keyboard_inside = false;
    for entry in &notices.entries {
        let view = tasty_ui_widgets::RestoreNoticeView {
            id_salt: egui::Id::new("restore_notice").with(entry.id),
            title: &entry.title,
            close_label: crate::i18n::t("notice.close"),
            cause: &entry.cause,
            label: &entry.label,
            path: &entry.path,
            copy_label: crate::i18n::t("notice.copy_path"),
            body: &entry.body,
            continue_label: crate::i18n::t("notice.continue"),
            title_bar_height: super::popup::title_bar_height(),
        };
        // 포커스를 가져가지 않는 Area다. 안내 상자만 포인터를 받는다.
        let output = egui::Area::new(egui::Id::new("restore_notice_area").with(entry.id))
            .order(egui::Order::Foreground)
            .fixed_pos(egui::pos2(column.left(), next_top))
            .constrain(false)
            .show(ctx, |ui| {
                tasty_ui_widgets::restore_notice(ui, &theme, &view, column.width())
            })
            .inner;
        if let Some(error) = &output.glyph_error {
            tracing::warn!("restore notice glyph failed to load: {error}");
        }
        if output.copy_path {
            ctx.copy_text(entry.path.clone());
        }
        if output.dismissed {
            dismissed.push(entry.id);
        } else {
            keyboard_inside |= output.focused_inside;
            cards.push(crate::model::LogicalRect {
                x: crate::model::LogicalPx(output.rect.min.x),
                y: crate::model::LogicalPx(output.rect.min.y),
                width: crate::model::LogicalPx(output.rect.width()),
                height: crate::model::LogicalPx(output.rect.height()),
            });
        }
        occupied = Some(occupied.map_or(output.rect, |rect| rect.union(output.rect)));
        next_top = output.rect.bottom() + gap;
    }
    for id in dismissed {
        notices.dismiss(id);
    }
    if notices.entries.is_empty() {
        occupied = None;
    }
    notices.cards = cards;
    notices.keyboard_inside = keyboard_inside;
    notices.undrawn = false;
    occupied
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: u64, path: &str) -> RestoreNoticeEntry {
        RestoreNoticeEntry {
            id,
            title: "title".into(),
            cause: "cause".into(),
            label: "label".into(),
            path: path.into(),
            body: "body".into(),
        }
    }

    fn rect(x: f32, y: f32, width: f32, height: f32) -> LogicalRect {
        LogicalRect {
            x: LogicalPx(x),
            y: LogicalPx(y),
            width: LogicalPx(width),
            height: LogicalPx(height),
        }
    }

    fn columns(top: LogicalPx) -> LogicalRect {
        rect(632.0, top.value() + 8.0, 360.0, 100.0)
    }

    const INSET: LogicalPx = LogicalPx(8.0);

    #[test]
    fn the_column_starts_under_a_window_banner_and_an_open_search_bar() {
        let banner = rect(0.0, 36.0, 1000.0, 32.0);
        let search = rect(700.0, 76.0, 292.0, 32.0);
        let base = LogicalPx(36.0);
        assert_eq!(column_top(base, INSET, columns, &[]), base);
        assert_eq!(column_top(base, INSET, columns, &[banner]), LogicalPx(68.0));
        assert_eq!(
            column_top(base, INSET, columns, &[search, banner]),
            LogicalPx(108.0)
        );
    }

    #[test]
    fn obstacles_beside_or_below_the_column_do_not_move_it() {
        let left = rect(0.0, 36.0, 600.0, 32.0);
        let lower = rect(700.0, 400.0, 292.0, 32.0);
        let base = LogicalPx(36.0);
        assert_eq!(column_top(base, INSET, columns, &[left, lower]), base);
    }

    #[test]
    fn the_same_notice_is_not_stacked_twice() {
        let mut notices = RestoreNotices::default();
        assert!(notices.push(entry(1, "/a")));
        assert!(!notices.push(entry(2, "/a")));
        assert!(notices.push(entry(3, "/b")));
        notices.dismiss(1);
        assert_eq!(notices.take_acknowledged(), vec![1]);
        assert!(notices.take_acknowledged().is_empty());
    }
}
