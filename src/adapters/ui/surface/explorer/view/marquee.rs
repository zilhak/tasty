//! 목록 빈 곳에서 끌어 사각형에 걸친 항목을 고르는 영역 선택. 사각형의 시작점은 목록 좌표로 두어
//! 스크롤해도 처음 누른 항목 자리에 남고, 화면 밖 항목도 칸 배치(`ListLayout`)로 계산해 고른다.

use std::collections::HashSet;
use std::ops::Range;
use std::path::PathBuf;

use tasty_type_appearance::theme::Theme;

use super::ExplorerView;
use super::cursor::Drawn;

/// 가장자리 띠 가장 바깥에서의 자동 스크롤 속도(논리 px/초). 띠 안쪽 경계에서 0 이고 가장자리로 갈수록
/// 선형으로 빨라진다.
// TODO(design-request): 디자인 회신은 속도를 정하지 않았다. 값을 받으면 토큰 접근자로 바꾼다.
const AUTOSCROLL_MAX_SPEED: f32 = 800.0;

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

    /// 목록 ScrollArea 를 그리기 전에 부른다. 띠 안에서 끌고 있으면 휠 입력처럼 스크롤 양을 넣는다.
    pub(crate) fn feed_scroll(&mut self, ui: &egui::Ui) {
        let dy = std::mem::take(&mut self.scroll);
        if dy != 0.0 && self.active() {
            // egui 는 휠 아래 방향을 음수로 받는다.
            ui.ctx().input_mut(|i| i.smooth_scroll_delta.y -= dy);
        }
    }
}

/// 띠 안 깊이에 비례한 이번 프레임 스크롤 양. 위 띠면 음수, 아래 띠면 양수, 그 밖이면 0 이다.
/// 목록 밖도 0 이다. egui ScrollArea 는 포인터가 자기 안에 있을 때만 스크롤 입력을 받는다.
pub(crate) fn autoscroll(list: egui::Rangef, y: f32, zone: f32, dt: f32) -> f32 {
    if zone <= 0.0 || list.span() <= zone * 2.0 || !list.contains(y) {
        return 0.0;
    }
    let depth = if y < list.min + zone {
        -(list.min + zone - y) / zone
    } else if y > list.max - zone {
        (y - (list.max - zone)) / zone
    } else {
        0.0
    };
    depth * AUTOSCROLL_MAX_SPEED * dt
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
    // TODO(design-tokens): `explorer_autoscroll_zone` 접근자로 바꾼다. 지금은 같은 값(size-24)의 spacing_xl 이다.
    let zone = theme.spacing_xl.value();
    view.marquee.scroll = autoscroll(list.y_range(), pos.y, zone, dt);
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
    // TODO(design-tokens): `explorer_marquee_bg`·`explorer_marquee_border` 접근자로 바꾼다. 지금은 같은
    // 값(accent-primary × tint-fill·tint-border)의 drop target 채움과 파일 드롭 테두리를 쓴다.
    let fill = theme.explorer_drop_target_bg().to_egui();
    let border = theme.file_drop_overlay_border().to_egui();
    let painter = ui.ctx().layer_painter(ui.layer_id()).with_clip_rect(list);
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(theme.border_width.value(), border),
        egui::StrokeKind::Inside,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn autoscroll_runs_only_inside_the_edge_band_and_grows_toward_the_edge() {
        let list = egui::Rangef::new(100.0, 500.0);
        let dt = 0.1;
        assert_eq!(autoscroll(list, 300.0, 24.0, dt), 0.0);
        assert_eq!(autoscroll(list, 124.0, 24.0, dt), 0.0);
        let half = autoscroll(list, 112.0, 24.0, dt);
        let edge = autoscroll(list, 100.0, 24.0, dt);
        assert!(half < 0.0 && edge < half, "{half} {edge}");
        assert_eq!(edge, -AUTOSCROLL_MAX_SPEED * dt);
        assert!(autoscroll(list, 490.0, 24.0, dt) > 0.0);
        assert_eq!(autoscroll(list, 500.0, 24.0, dt), AUTOSCROLL_MAX_SPEED * dt);
        // 목록 밖에서는 스크롤하지 않는다.
        assert_eq!(autoscroll(list, 900.0, 24.0, dt), 0.0);
        // 띠 둘이 겹칠 만큼 낮은 목록은 스크롤하지 않는다.
        assert_eq!(autoscroll(egui::Rangef::new(0.0, 40.0), 1.0, 24.0, dt), 0.0);
    }
}
