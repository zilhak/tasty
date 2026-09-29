//! 가로 탭 목록과 넘친 영역을 이동하는 화살표. id_salt는 호출자별로 달라야 한다.
//! SVG 로더가 미리 설치돼 있어야 한다. 화살표 칸은 탭 목록 양옆에 따로 두며 자체 채움이 없다.
//! 현재 갤러리 레이아웃 예제에서 사용하며 본체 설정 창은 제목을 포함한 별도 밴드를 그린다.

use tasty_type_appearance::theme::Theme;

/// 한 step 스크롤 거리 (평균 탭 너비 ~80px 기준).
const SCROLL_STEP: f32 = 80.0;

/// 스크롤 화살표 칸의 방향.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabScrollArrowSide {
    Left,
    Right,
}

/// 스크롤 화살표의 잉크. 도달한 끝은 `Disabled`, 이동 대기 대상이 그쪽에 가려져 있으면 `Move`다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabScrollArrowInk {
    Enabled,
    Disabled,
    Move,
}

/// 스크롤 화살표 칸 하나를 그린다. 칸은 채우지 않고 스트립 바탕을 그대로 쓴다.
/// hover 채움은 disabled가 아닐 때만 깐다.
pub fn paint_tab_scroll_arrow(
    ui: &egui::Ui,
    theme: &Theme,
    cell: egui::Rect,
    side: TabScrollArrowSide,
    ink: TabScrollArrowInk,
    hovered: bool,
) {
    if hovered && ink != TabScrollArrowInk::Disabled {
        ui.painter().rect_filled(
            cell,
            0.0,
            theme.tab_scroll_arrow_hover_bg().to_egui_premultiplied(),
        );
    }
    let color = match ink {
        TabScrollArrowInk::Enabled => theme.tab_scroll_arrow_fg(),
        TabScrollArrowInk::Disabled => theme.tab_scroll_arrow_fg_disabled(),
        TabScrollArrowInk::Move => theme.tab_scroll_arrow_move_fg(),
    };
    let glyph = match side {
        TabScrollArrowSide::Left => tasty_icons::CHEVRON_LEFT,
        TabScrollArrowSide::Right => tasty_icons::CHEVRON_RIGHT,
    };
    let size = theme.tab_scroll_arrow_glyph_size().value();
    glyph.image(size, color.to_egui()).paint_at(
        ui,
        egui::Rect::from_center_size(cell.center(), egui::vec2(size, size)),
    );
}

/// 탭을 선택하면 active를 갱신한다. 목록이 넘치면 양옆 화살표로 SCROLL_STEP만큼 이동한다.
/// 넘침 여부는 직전 프레임의 콘텐츠 폭으로 정하므로 넘친 첫 프레임 뒤에 화살표가 나타난다.
pub fn horizontal_tab_bar_with_arrows<T: Copy + PartialEq>(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    tabs: &[(T, &str)],
    active: &mut T,
) {
    let overflow_id = ui.id().with(id_salt).with("overflow");
    let full_w = ui.available_width();
    let overflowed = ui
        .data(|d| d.get_temp::<bool>(overflow_id))
        .unwrap_or(false);
    let arrow_w = theme.tab_scroll_arrow_width().value();

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let arrows = overflowed
            .then(|| ui.allocate_exact_size(egui::vec2(arrow_w, arrow_w), egui::Sense::click()));
        let viewport_max = if overflowed {
            (full_w - arrow_w * 2.0).max(0.0)
        } else {
            full_w
        };
        let output = egui::ScrollArea::horizontal()
            .id_salt(id_salt)
            .auto_shrink([false, true])
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .max_width(viewport_max)
            // 탭 클릭 중 드래그가 스크롤로 바뀌지 않도록 패닝을 끈다.
            .drag_to_scroll(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (tab, label) in tabs {
                        let selected = *active == *tab;
                        if ui.selectable_label(selected, *label).clicked() {
                            *active = *tab;
                        }
                    }
                });
            });

        let content_w = output.content_size.x;
        let needs_scroll = content_w > full_w + 0.5;
        if needs_scroll != overflowed {
            ui.data_mut(|d| d.insert_temp(overflow_id, needs_scroll));
            ui.ctx().request_repaint();
        }
        let Some((left_rect, left)) = arrows else {
            return;
        };
        let (right_rect, right) =
            ui.allocate_exact_size(egui::vec2(arrow_w, arrow_w), egui::Sense::click());

        let max_offset = (content_w - output.inner_rect.width()).max(0.0);
        let offset = output.state.offset.x;
        let can_left = offset > 0.0;
        let can_right = offset < max_offset;
        let ink = |enabled: bool| {
            if enabled {
                TabScrollArrowInk::Enabled
            } else {
                TabScrollArrowInk::Disabled
            }
        };
        paint_tab_scroll_arrow(
            ui,
            theme,
            left_rect,
            TabScrollArrowSide::Left,
            ink(can_left),
            left.hovered(),
        );
        paint_tab_scroll_arrow(
            ui,
            theme,
            right_rect,
            TabScrollArrowSide::Right,
            ink(can_right),
            right.hovered(),
        );

        let mut new_offset = offset;
        if left.clicked() && can_left {
            new_offset = (new_offset - SCROLL_STEP).max(0.0);
        }
        if right.clicked() && can_right {
            new_offset = (new_offset + SCROLL_STEP).min(max_offset);
        }
        if (new_offset - offset).abs() > f32::EPSILON {
            let mut s = output.state;
            s.offset.x = new_offset;
            s.store(ui.ctx(), output.id);
            ui.ctx().request_repaint();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
    }

    fn flatten(shape: &egui::Shape, out: &mut Vec<egui::Shape>) {
        match shape {
            egui::Shape::Vec(v) => v.iter().for_each(|s| flatten(s, out)),
            other => out.push(other.clone()),
        }
    }

    fn run(ctx: &egui::Context, width: f32, add: impl FnMut(&mut egui::Ui)) -> Vec<egui::Shape> {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 200.0),
            )),
            ..Default::default()
        };
        let mut add = add;
        let out = ctx.run(input, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| add(ui));
        });
        let mut shapes = Vec::new();
        out.shapes
            .iter()
            .for_each(|c| flatten(&c.shape, &mut shapes));
        shapes
    }

    /// 이미지는 텍스처 brush를 가진 사각형으로 칠해지고 fill이 tint다. 글리프 tint를 모두 모은다.
    fn glyph_tints(shapes: &[egui::Shape]) -> Vec<egui::Color32> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) if r.brush.is_some() => Some(r.fill),
                _ => None,
            })
            .collect()
    }

    /// 텍스처 없이 색을 칠한 사각형. 패널의 투명 바탕은 뺀다.
    fn filled_rects(shapes: &[egui::Shape]) -> Vec<(egui::Rect, egui::Color32)> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r)
                    if r.brush.is_none() && r.fill != egui::Color32::TRANSPARENT =>
                {
                    Some((r.rect, r.fill))
                }
                _ => None,
            })
            .collect()
    }

    fn one_arrow(ink: TabScrollArrowInk, hovered: bool) -> Vec<egui::Shape> {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let th = theme();
        let cell = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(24.0, 24.0));
        run(&ctx, 200.0, |ui| {
            paint_tab_scroll_arrow(ui, &th, cell, TabScrollArrowSide::Left, ink, hovered);
        })
    }

    #[test]
    fn each_ink_tints_the_chevron_with_its_role() {
        let th = theme();
        for (ink, want) in [
            (TabScrollArrowInk::Enabled, th.tab_scroll_arrow_fg()),
            (
                TabScrollArrowInk::Disabled,
                th.tab_scroll_arrow_fg_disabled(),
            ),
            (TabScrollArrowInk::Move, th.tab_scroll_arrow_move_fg()),
        ] {
            assert_eq!(
                glyph_tints(&one_arrow(ink, false)),
                vec![want.to_egui()],
                "{ink:?}"
            );
        }
    }

    #[test]
    fn only_a_side_that_can_scroll_takes_the_hover_fill() {
        let th = theme();
        let hover = th.tab_scroll_arrow_hover_bg().to_egui_premultiplied();
        for ink in [TabScrollArrowInk::Enabled, TabScrollArrowInk::Move] {
            let rects = filled_rects(&one_arrow(ink, true));
            assert!(rects.iter().any(|(_, f)| *f == hover), "{ink:?}: {rects:?}");
        }
        let rects = filled_rects(&one_arrow(TabScrollArrowInk::Disabled, true));
        assert!(rects.iter().all(|(_, f)| *f != hover), "{rects:?}");
        // 채우지 않은 칸은 hover가 없을 때 사각형을 하나도 칠하지 않는다.
        assert!(filled_rects(&one_arrow(TabScrollArrowInk::Enabled, false)).is_empty());
    }

    /// 넘친 목록을 처음 그리면 왼쪽은 끝에 닿아 disabled, 오른쪽은 enabled다. 0.4 tint는 없다.
    #[test]
    fn an_overflowing_bar_starts_with_the_left_arrow_disabled() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let th = theme();
        let tabs: Vec<(usize, String)> = (0..12).map(|i| (i, format!("tab number {i}"))).collect();
        let tabs: Vec<(usize, &str)> = tabs.iter().map(|(i, s)| (*i, s.as_str())).collect();
        let mut active = 0;
        let mut frame = || {
            run(&ctx, 300.0, |ui| {
                horizontal_tab_bar_with_arrows(ui, &th, "t", &tabs, &mut active);
            })
        };
        assert!(
            glyph_tints(&frame()).is_empty(),
            "첫 프레임은 넘침을 모른다"
        );
        let shapes = frame();
        assert_eq!(
            glyph_tints(&shapes),
            vec![
                th.tab_scroll_arrow_fg_disabled().to_egui(),
                th.tab_scroll_arrow_fg().to_egui()
            ]
        );
    }

    #[test]
    fn a_bar_that_fits_draws_no_arrows() {
        let ctx = egui::Context::default();
        egui_extras::install_image_loaders(&ctx);
        let th = theme();
        let tabs = [(0usize, "a"), (1, "b")];
        let mut active = 0;
        for _ in 0..2 {
            let shapes = run(&ctx, 600.0, |ui| {
                horizontal_tab_bar_with_arrows(ui, &th, "t", &tabs, &mut active);
            });
            assert!(glyph_tints(&shapes).is_empty());
        }
    }
}
