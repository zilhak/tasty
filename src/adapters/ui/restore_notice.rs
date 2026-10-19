//! 창 오른쪽 위 안내 열에 띄우는 복원 실패·저장 멈춤 비포커스 안내의 창별 상태와 그리기.
//!
//! 안내 내용은 App이 넘긴다. 안내는 나타날 때 포커스를 가져가지 않고 scrim도 없다. 바깥의
//! 키·선택·스크롤은 터미널로 그대로 간다. ×·Continue·안쪽 컨트롤 포커스의 Esc는 같은 응답이며,
//! 안내를 지우고 그 id를 App이 확인 응답으로 가져가게 남긴다.

use crate::model::PhysicalRect;

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
    /// 직전 프레임에 그린 안내 전체 사각형(논리 좌표). native WebView bounds 축소에 쓴다.
    occupied: Option<egui::Rect>,
    /// 직전 프레임에 안내 안쪽 컨트롤이 키보드 포커스를 가졌는지. 이때만 키를 egui로 보낸다.
    keyboard_inside: bool,
}

impl RestoreNotices {
    /// 안내를 더한다. 같은 안내가 이미 떠 있으면 더하지 않고 false다.
    pub(crate) fn push(&mut self, entry: RestoreNoticeEntry) -> bool {
        if self.entries.iter().any(|shown| shown.same_as(&entry)) {
            return false;
        }
        self.entries.push(entry);
        true
    }

    pub(crate) fn take_acknowledged(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.acknowledged)
    }

    /// 직전 프레임에 그린 안내 영역. 안내가 없으면 `None`이다.
    pub(crate) fn occupied(&self) -> Option<egui::Rect> {
        self.occupied
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
    base: f32,
    inset: f32,
    columns: impl Fn(f32) -> egui::Rect,
    obstacles: &[egui::Rect],
) -> f32 {
    let mut top = base;
    let mut sorted: Vec<egui::Rect> = obstacles.to_vec();
    sorted.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
    for obstacle in sorted {
        let column = columns(top);
        let beside = obstacle.max.x <= column.min.x || obstacle.min.x >= column.max.x;
        if !beside && obstacle.min.y <= column.min.y + inset && obstacle.max.y > top {
            top = obstacle.max.y;
        }
    }
    top
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
    let base = work.top() + tab_bar_height.to_logical(scale).value();
    let columns = |top| tasty_ui_widgets::notice_column_rect(&theme, work, top);
    let top = column_top(
        base,
        theme.notice_column_inset().value(),
        columns,
        obstacles,
    );
    let column = columns(top);
    let gap = theme.notice_column_gap().value();
    let mut next_top = column.top();
    let mut occupied: Option<egui::Rect> = None;
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
    notices.occupied = occupied;
    notices.keyboard_inside = keyboard_inside;
    occupied
}

/// 안내와 겹치는 native WebView의 bounds를 안내 옆으로 줄인다. WebView를 숨기지 않고 포커스·입력
/// 대상도 바꾸지 않는다. 기본은 안내 열 왼쪽 경계(열 x − inset)까지 폭을 줄이는 것이다. pane이 좁아
/// 그 왼쪽에 남는 면적이 안내 아래(안내 bottom + inset부터)에 남는 면적보다 작으면 위쪽을 안내
/// 아래로 내린다. 어느 쪽에도 남는 면적이 없으면(pane 전체가 안내 안) 페이지를 계속 보이고 쓸 수
/// 있도록 bounds를 그대로 둔다. 겹치지 않으면 그대로 돌려준다.
pub(crate) fn shrink_beside_notices(
    bounds: crate::webview::WebViewBounds,
    occupied: Option<egui::Rect>,
    inset: f32,
) -> crate::webview::WebViewBounds {
    let Some(notice) = occupied else {
        return bounds;
    };
    let overlaps = bounds.x < f64::from(notice.right())
        && bounds.x + bounds.width > f64::from(notice.left())
        && bounds.y < f64::from(notice.bottom())
        && bounds.y + bounds.height > f64::from(notice.top());
    if !overlaps {
        return bounds;
    }
    let left_width = (f64::from(notice.left() - inset) - bounds.x).max(0.0);
    let below_top = f64::from(notice.bottom() + inset);
    let below_height = (bounds.y + bounds.height - below_top).max(0.0);
    let left_area = left_width * bounds.height;
    let below_area = below_height * bounds.width;
    if left_area <= 0.0 && below_area <= 0.0 {
        return bounds;
    }
    if left_area >= below_area {
        crate::webview::WebViewBounds {
            width: left_width,
            ..bounds
        }
    } else {
        crate::webview::WebViewBounds {
            y: below_top,
            height: below_height,
            ..bounds
        }
    }
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

    fn columns(top: f32) -> egui::Rect {
        egui::Rect::from_min_max(egui::pos2(632.0, top + 8.0), egui::pos2(992.0, top + 108.0))
    }

    #[test]
    fn the_column_starts_under_a_window_banner_and_an_open_search_bar() {
        let banner = egui::Rect::from_min_max(egui::pos2(0.0, 36.0), egui::pos2(1000.0, 68.0));
        let search = egui::Rect::from_min_max(egui::pos2(700.0, 76.0), egui::pos2(992.0, 108.0));
        assert_eq!(column_top(36.0, 8.0, columns, &[]), 36.0);
        assert_eq!(column_top(36.0, 8.0, columns, &[banner]), 68.0);
        assert_eq!(column_top(36.0, 8.0, columns, &[search, banner]), 108.0);
    }

    #[test]
    fn obstacles_beside_or_below_the_column_do_not_move_it() {
        let left = egui::Rect::from_min_max(egui::pos2(0.0, 36.0), egui::pos2(600.0, 68.0));
        let lower = egui::Rect::from_min_max(egui::pos2(700.0, 400.0), egui::pos2(992.0, 432.0));
        assert_eq!(column_top(36.0, 8.0, columns, &[left, lower]), 36.0);
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

    fn bounds(x: f64, width: f64) -> crate::webview::WebViewBounds {
        crate::webview::WebViewBounds {
            x,
            y: 0.0,
            width,
            height: 400.0,
        }
    }

    #[test]
    fn an_overlapping_webview_ends_left_of_the_notice() {
        let notice = egui::Rect::from_min_max(egui::pos2(600.0, 40.0), egui::pos2(960.0, 240.0));
        let shrunk = shrink_beside_notices(bounds(0.0, 968.0), Some(notice), 8.0);
        assert_eq!(shrunk.width, 592.0);
        assert_eq!(shrunk.x, 0.0);
        assert_eq!(shrunk.height, 400.0);
    }

    #[test]
    fn a_narrow_pane_under_the_notice_moves_below_it_instead_of_vanishing() {
        let notice = egui::Rect::from_min_max(egui::pos2(600.0, 40.0), egui::pos2(960.0, 240.0));
        let shrunk = shrink_beside_notices(bounds(700.0, 268.0), Some(notice), 8.0);
        assert_eq!(shrunk.x, 700.0);
        assert_eq!(shrunk.width, 268.0);
        assert_eq!(shrunk.y, 248.0);
        assert_eq!(shrunk.height, 152.0);
    }

    #[test]
    fn a_pane_entirely_inside_the_notice_keeps_its_bounds() {
        let notice = egui::Rect::from_min_max(egui::pos2(600.0, 0.0), egui::pos2(960.0, 500.0));
        assert_eq!(
            shrink_beside_notices(bounds(700.0, 200.0), Some(notice), 8.0),
            bounds(700.0, 200.0)
        );
    }

    #[test]
    fn a_webview_away_from_the_notice_keeps_its_bounds() {
        let notice = egui::Rect::from_min_max(egui::pos2(600.0, 40.0), egui::pos2(960.0, 240.0));
        assert_eq!(
            shrink_beside_notices(bounds(0.0, 400.0), Some(notice), 8.0),
            bounds(0.0, 400.0)
        );
        assert_eq!(
            shrink_beside_notices(bounds(0.0, 968.0), None, 8.0),
            bounds(0.0, 968.0)
        );
    }
}
