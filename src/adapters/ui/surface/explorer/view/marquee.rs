//! 목록 빈 곳에서 끌어 사각형에 걸친 항목을 고르는 영역 선택. 사각형의 시작점은 목록 좌표로 두어
//! 스크롤해도 처음 누른 항목 자리에 남고, 화면 밖 항목도 칸 배치(`ListLayout`)로 계산해 고른다.

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;

use tasty_type_appearance::theme::Theme;

use super::ExplorerView;
use super::cursor::Drawn;

/// 목록 끝과 그 밖에서의 자동 스크롤 속도(보기의 줄/초). 디자인이 토큰 없는 규칙 상수로 정했다.
const AUTOSCROLL_SPEED_ROWS_PER_SEC: f32 = 20.0;

#[derive(Default)]
pub(crate) struct MarqueeState {
    press: Option<Press>,
    /// 다음 프레임 목록 ScrollArea 에 줄 스크롤 양(논리 px, 양수면 아래로).
    scroll: f32,
}

struct Press {
    /// 누른 자리. 목록 좌표(`ListLayout::origin` 기준)다.
    at: egui::Pos2,
    /// Ctrl·Cmd·Shift 를 누르고 시작했으면 그때의 선택에 더한다.
    base: HashSet<PathBuf>,
    active: bool,
    last: Vec<Range<usize>>,
}

impl MarqueeState {
    /// 영역 선택 중이다. 그동안 항목 드래그와 키보드 이동이 끼어들지 않는다.
    pub(crate) fn active(&self) -> bool {
        self.press.as_ref().is_some_and(|p| p.active)
    }

    /// 목록 ScrollArea 를 그리기 전에 부른다. 띠 안이나 목록 밖에서 끌고 있으면 스크롤 양을 넣는다.
    /// 휠 입력과 달리 포인터가 목록 밖에 있어도 다음에 닫히는 ScrollArea(목록)가 받는다.
    pub(crate) fn feed_scroll(&mut self, ui: &egui::Ui) {
        let dy = std::mem::take(&mut self.scroll);
        if dy != 0.0 && self.active() {
            // egui 는 내용이 아래로 움직이는 방향을 양수로 받는다.
            ui.scroll_with_delta_animation(
                egui::vec2(0.0, -dy),
                egui::style::ScrollAnimation::none(),
            );
        }
    }
}

/// 이번 프레임 스크롤 양(논리 px). 위 띠면 음수, 아래 띠면 양수, 띠 사이면 0 이다.
/// 속도는 `20 줄/초 × t²` 이고 t 는 띠 안쪽 경계에서 0, 목록 끝에서 1 이다. 목록 밖에서는 t 를 1 로
/// 둔다. 줄은 보기의 한 줄(Detail·List 행, Grid 칸 줄)이라 `row` 는 줄 간격이다.
pub(crate) fn autoscroll(list: egui::Rangef, y: f32, zone: f32, row: f32, dt: f32) -> f32 {
    if zone <= 0.0 || list.span() <= zone * 2.0 {
        return 0.0;
    }
    let depth = if y < list.min + zone {
        -((list.min + zone - y) / zone).min(1.0)
    } else if y > list.max - zone {
        ((y - (list.max - zone)) / zone).min(1.0)
    } else {
        0.0
    };
    depth.signum() * depth * depth * AUTOSCROLL_SPEED_ROWS_PER_SEC * row * dt
}

/// 본문을 그린 뒤 한 번 부른다. `drawn` 은 이번 프레임에 그린 항목이다.
pub(crate) fn frame(ui: &egui::Ui, theme: &Theme, view: &mut ExplorerView, drawn: &[Drawn]) {
    let ctx = ui.ctx().clone();
    let (Some(list), Some(layout)) = (view.list_rect, view.list_layout) else {
        view.marquee = MarqueeState::default();
        return;
    };
    let (pressed, down, origin, pos, mods, dt) = ctx.input(|i| {
        (
            i.pointer.primary_pressed(),
            i.pointer.primary_down(),
            i.pointer.press_origin(),
            i.pointer.latest_pos(),
            i.modifiers,
            i.stable_dt,
        )
    });
    if pressed {
        view.marquee.press = origin
            .filter(|&o| list.contains(o) && free_spot(&ctx, list, drawn, o))
            .map(|o| Press {
                at: (o - layout.origin).to_pos2(),
                base: if mods.command || mods.shift {
                    view.selected.clone()
                } else {
                    HashSet::new()
                },
                active: false,
                last: Vec::new(),
            });
    }
    if !down {
        view.marquee = MarqueeState::default();
        return;
    }
    let (Some(mut press), Some(pos)) = (view.marquee.press.take(), pos) else {
        return;
    };
    if !press.active {
        let dragging = ctx.input(|i| i.pointer.is_decidedly_dragging());
        let taken = egui::DragAndDrop::has_any_payload(&ctx) || ctx.dragged_id().is_some();
        if !dragging || taken {
            // 다른 위젯이 끌기를 가져갔으면 이번 누름에서는 영역 선택을 시작하지 않는다.
            view.marquee.press = (!taken).then_some(press);
            return;
        }
        press.active = true;
        view.cursor.hide();
    }
    let area = egui::Rect::from_two_pos(press.at, (pos - layout.origin).to_pos2());
    let hits = layout.hits(area);
    if hits != press.last {
        let mut selected = press.base.clone();
        for range in &hits {
            selected.extend(view.shown_range(range.clone()).into_iter().map(|e| e.path));
        }
        let anchor = hits
            .first()
            .and_then(|r| view.shown_range(r.start..r.start + 1).pop());
        press.last = hits;
        view.replace_selection(selected, anchor.map(|e| e.path));
    }
    view.marquee.press = Some(press);
    let zone = theme.explorer_autoscroll_zone().value();
    view.marquee.scroll = autoscroll(list.y_range(), pos.y, zone, layout.step.y, dt);
    ctx.request_repaint();
    paint(ui, theme, area.translate(layout.origin.to_vec2()), list);
}

/// 누른 자리가 빈 곳인가. 항목 칸이 아니고, 누를 수 있는 다른 위젯(스크롤 막대·이름 입력·`..` 칸)도
/// 아니어야 한다. 목록 전체를 덮는 배경 위젯은 빈 곳으로 본다.
fn free_spot(ctx: &egui::Context, list: egui::Rect, drawn: &[Drawn], at: egui::Pos2) -> bool {
    if drawn.iter().any(|d| d.shown.contains(at)) {
        return false;
    }
    let covers = |w: &egui::WidgetRect| w.interact_rect.contains_rect(list);
    ctx.viewport(|v| {
        let hits = &v.hits;
        hits.click.as_ref().is_none_or(covers) && hits.drag.as_ref().is_none_or(covers)
    })
}

fn paint(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, list: egui::Rect) {
    let painter = ui.ctx().layer_painter(ui.layer_id()).with_clip_rect(list);
    tasty_ui_widgets::paint_marquee(&painter, theme, rect);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoscroll_speeds_up_by_the_square_and_keeps_full_speed_past_the_edge() {
        let list = egui::Rangef::new(100.0, 500.0);
        let (zone, row, dt) = (24.0, 22.0, 0.1);
        let full = AUTOSCROLL_SPEED_ROWS_PER_SEC * row * dt;
        assert_eq!(autoscroll(list, 300.0, zone, row, dt), 0.0);
        assert_eq!(autoscroll(list, 124.0, zone, row, dt), 0.0);
        // 띠 가운데(t = 0.5)는 최고 속도의 1/4 이다.
        assert!((autoscroll(list, 112.0, zone, row, dt) + full / 4.0).abs() < 1e-4);
        assert!((autoscroll(list, 488.0, zone, row, dt) - full / 4.0).abs() < 1e-4);
        assert_eq!(autoscroll(list, 100.0, zone, row, dt), -full);
        assert_eq!(autoscroll(list, 500.0, zone, row, dt), full);
        // 목록 밖에서는 거리와 관계없이 최고 속도를 유지한다.
        assert_eq!(autoscroll(list, 40.0, zone, row, dt), -full);
        assert_eq!(autoscroll(list, 900.0, zone, row, dt), full);
        // 줄 단위라 줄이 높은 보기(Grid)는 같은 시간에 같은 수의 줄을 지난다.
        assert_eq!(autoscroll(list, 500.0, zone, 2.0 * row, dt), 2.0 * full);
        // 띠 둘이 겹칠 만큼 낮은 목록은 스크롤하지 않는다.
        assert_eq!(
            autoscroll(egui::Rangef::new(0.0, 40.0), 1.0, zone, row, dt),
            0.0
        );
    }
}
