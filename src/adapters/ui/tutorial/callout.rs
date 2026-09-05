//! 안내 말풍선(Callout) — 244px 고정폭. 제목·본문·`step/total`·dot rail·
//! Skip/Back/Next + 4방 tail. **edge-avoidance layout pass**(선호순서 below→
//! above→right→left, 뷰포트 오버플로 시 flip, 8px 안전영역 clamp, clamp 후에도
//! tail 은 마커 모서리를 계속 조준)는 순수 함수 [`place_callout`] 로 분리해 단위
//! 테스트한다.
//!
//! 디자인 SoT `gallery/overlays-tutorial.jsx::Callout` 의 host 대응. 버튼은 DS
//! `Button` 재사용, 색·간격·반경은 `Theme` 토큰.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, vspace};

use crate::i18n::t;

/// 244px 고정폭(i18n 가변폭은 세로로 흡수).
pub const CALLOUT_W: LogicalPx = LogicalPx(244.0);
/// tail 삼각 크기(12px diamond 전사).
const TAIL: LogicalPx = LogicalPx(12.0);
/// up/down tail 의 좌측 기준 앵커 offset(디자인 left:28).
const TAIL_OFF_H: LogicalPx = LogicalPx(28.0);
/// left/right tail 의 상단 기준 앵커 offset(디자인 top:24).
const TAIL_OFF_V: LogicalPx = LogicalPx(24.0);

/// 스텝 레일 점의 지름. 스케일 밖(5) — 점 치수 토큰은 `status-dot-size`(8) 하나뿐이고
/// 그 토큰은 `zoomed()` 를 타 배율 0.85 / 1.0 / 1.2 에서 7 / 8 / 10 이 된다. 여기를
/// 8 로 보내면 배율 1 에서 픽셀이 바뀐다 — 스냅이 아니라 값 변경이라
/// `docs/adr/0126-off-scale-font-values-are-not-snapped-to-tokens.md` 대로 이름만 붙인다.
/// **같은 5 를 `src/view/settings/ui/tabs/appearance.rs` 의 `COLOR_OVERRIDE_DOT_SIZE`
/// 도 쓴다** — 무관한 두 화면이 독립적으로 고른 값이라 드리프트가 아니라 역할일
/// 가능성이 높고, 그 판단이 서면 둘이 한 토큰으로 모인다.
///
/// 이것은 상태 점이 아니라 **진행 표시(pagination)** 점이다 — 위 두 자리가 한 토큰으로
/// 모이더라도 이 자리가 거기 속하는지는 별개 물음이다.
const STEP_RAIL_DOT_SIZE: LogicalPx = LogicalPx(5.0);

/// 마커가 말풍선의 어느 쪽에 있는지 = tail 방향.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tail {
    Up,
    Down,
    Left,
    Right,
}

/// 배치 결과 — 말풍선 좌상단 pos + tail 방향 + tail 앵커 offset(clamp 반영).
#[derive(Clone, Copy, Debug)]
pub struct Placement {
    pub pos: egui::Pos2,
    pub tail: Tail,
    /// up/down 이면 좌측에서의 x offset, left/right 이면 상단에서의 y offset.
    pub tail_offset: LogicalPx,
}

fn fits(rect: egui::Rect, safe: egui::Rect) -> bool {
    rect.min.x >= safe.min.x
        && rect.min.y >= safe.min.y
        && rect.max.x <= safe.max.x
        && rect.max.y <= safe.max.y
}

/// edge-avoidance 배치 — 선호순서 below→above→right→left 중 안전영역에 들어가는
/// 첫 후보를 쓰고, 아무것도 안 맞으면 below 를 안전영역으로 clamp 한다. tail 은
/// clamp 후에도 마커 중심을 조준하도록 offset 을 재계산한다.
pub fn place_callout(
    marker: egui::Rect,
    size: egui::Vec2,
    screen: egui::Rect,
    gap: f32,
    safe: f32,
) -> Placement {
    let safe_rect = screen.shrink(safe);
    // 후보 pos + tail (선호순서).
    let below = (
        egui::pos2(
            marker.center().x - TAIL_OFF_H.value(),
            marker.bottom() + gap,
        ),
        Tail::Up,
    );
    let above = (
        egui::pos2(
            marker.center().x - TAIL_OFF_H.value(),
            marker.top() - gap - size.y,
        ),
        Tail::Down,
    );
    let right = (
        egui::pos2(marker.right() + gap, marker.center().y - TAIL_OFF_V.value()),
        Tail::Left,
    );
    let left = (
        egui::pos2(
            marker.left() - gap - size.x,
            marker.center().y - TAIL_OFF_V.value(),
        ),
        Tail::Right,
    );

    let (mut pos, tail) = [below, above, right, left]
        .into_iter()
        .find(|(pos, _)| fits(egui::Rect::from_min_size(*pos, size), safe_rect))
        .unwrap_or(below);

    // 안전영역 clamp.
    pos.x = pos.x.clamp(
        safe_rect.min.x,
        (safe_rect.max.x - size.x).max(safe_rect.min.x),
    );
    pos.y = pos.y.clamp(
        safe_rect.min.y,
        (safe_rect.max.y - size.y).max(safe_rect.min.y),
    );

    // tail 은 마커 중심을 계속 조준 — offset 재계산 + tail 범위로 clamp.
    let tail_offset = match tail {
        Tail::Up | Tail::Down => {
            LogicalPx(marker.center().x - pos.x).clamp(TAIL, LogicalPx(size.x) - TAIL)
        }
        Tail::Left | Tail::Right => {
            LogicalPx(marker.center().y - pos.y).clamp(TAIL, LogicalPx(size.y) - TAIL)
        }
    };

    Placement {
        pos,
        tail,
        tail_offset,
    }
}

/// 말풍선 높이 추정(배치용) — 본문 galley 높이 + 고정 라인/패딩. egui Area 는
/// 세로 auto-size 지만 배치(above/clamp)에는 사전 높이가 필요하다.
pub fn callout_height(ctx: &egui::Context, theme: &Theme, body: &str) -> f32 {
    let content_w = (CALLOUT_W - theme.spacing_lg.scaled(2.0)).value();
    let body_h = ctx.fonts(|f| {
        f.layout(
            body.to_string(),
            egui::FontId::proportional(theme.font_size_caption.value()),
            theme.text_secondary().to_egui(),
            content_w,
        )
        .size()
        .y
    });
    // padding(md*2) + step line + gap + title line + gap + body + button row.
    theme.spacing_md.value() * 2.0
        + theme.font_size_micro.value()
        + 4.0
        + theme.font_size_body.value()
        + 6.0
        + body_h
        + theme.spacing_md.value()
        + theme.item_height_interactive.value()
}

/// 버튼 클릭 결과.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CalloutClick {
    None,
    Next,
    Back,
    Skip,
}

/// 말풍선을 `placement.pos` 에 최상위(Order::Tooltip) Area 로 그린다. 마커/scrim 과
/// 달리 이 Area 는 자기 영역의 마우스를 소비한다(버튼 상호작용). tail 은 Frame
/// 렌더 후 painter 로 얹는다.
#[allow(clippy::too_many_arguments)]
pub fn draw_callout(
    ctx: &egui::Context,
    theme: &Theme,
    placement: Placement,
    step: usize,
    total: usize,
    title: &str,
    body: &str,
    first: bool,
    last: bool,
) -> CalloutClick {
    let mut click = CalloutClick::None;
    let area = egui::Area::new(egui::Id::new("tutorial_callout"))
        .order(egui::Order::Tooltip)
        .fixed_pos(placement.pos)
        .show(ctx, |ui| {
            let resp = egui::Frame::new()
                .fill(theme.surface_raised().to_egui())
                .stroke(egui::Stroke::new(
                    theme.border_width.value(),
                    theme.border_strong().to_egui(),
                ))
                .corner_radius(theme.corner_radius_lg.value())
                .shadow(theme.shadow_popover().to_egui())
                .inner_margin(egui::Margin {
                    left: theme.spacing_lg.value() as i8,
                    right: theme.spacing_lg.value() as i8,
                    top: theme.spacing_md.value() as i8,
                    bottom: theme.spacing_md.value() as i8,
                })
                .show(ui, |ui| {
                    ui.set_width((CALLOUT_W - theme.spacing_lg.scaled(2.0)).value());
                    ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                    // step / total.
                    ui.label(
                        egui::RichText::new(format!("{step} / {total}"))
                            .monospace()
                            .size(theme.font_size_micro.value())
                            .strong()
                            .color(theme.accent_primary().to_egui()),
                    );
                    vspace(ui, theme.spacing_xs);
                    ui.label(
                        egui::RichText::new(title)
                            .size(theme.font_size_body.value())
                            .strong()
                            .color(theme.text_primary().to_egui()),
                    );
                    // 디자인 전사값 6px — 토큰 산술(4×1.5)로 표현.
                    vspace(ui, theme.spacing_xs * 1.5);
                    ui.label(
                        egui::RichText::new(body)
                            .size(theme.font_size_caption.value())
                            .color(theme.text_secondary().to_egui()),
                    );
                    ui.add_space(theme.spacing_md.value());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                        // dot rail (좌).
                        for i in 0..total {
                            let c = if i == step - 1 {
                                theme.accent_primary().to_egui()
                            } else {
                                theme.surface_active().to_egui()
                            };
                            let (r, _) = ui.allocate_exact_size(
                                egui::vec2(STEP_RAIL_DOT_SIZE.value(), STEP_RAIL_DOT_SIZE.value()),
                                egui::Sense::hover(),
                            );
                            ui.painter().circle_filled(
                                r.center(),
                                STEP_RAIL_DOT_SIZE.value() * 0.5,
                                c,
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let next_label = if last {
                                t("tutorial.btn_done")
                            } else {
                                t("tutorial.btn_next")
                            };
                            if Button::new(next_label)
                                .variant(ButtonVariant::Primary)
                                .size(ControlSize::Sm)
                                .show(ui, theme)
                                .clicked()
                            {
                                click = CalloutClick::Next;
                            }
                            if !first
                                && Button::new(t("tutorial.btn_back"))
                                    .variant(ButtonVariant::Secondary)
                                    .size(ControlSize::Sm)
                                    .show(ui, theme)
                                    .clicked()
                            {
                                click = CalloutClick::Back;
                            }
                            let skip = ui.add(
                                egui::Label::new(
                                    egui::RichText::new(t("tutorial.btn_skip"))
                                        .size(theme.font_size_caption.value())
                                        .color(theme.text_muted().to_egui()),
                                )
                                .sense(egui::Sense::click()),
                            );
                            if skip.clicked() {
                                click = CalloutClick::Skip;
                            }
                        });
                    });
                });
            resp.response.rect
        });

    paint_tail(
        &ctx.layer_painter(area.response.layer_id),
        area.inner,
        theme,
        placement,
    );
    click
}

/// tail 삼각형 — bubble 모서리에서 마커 방향으로 튀어나온다. 외곽 2변만 stroke.
fn paint_tail(p: &egui::Painter, bubble: egui::Rect, theme: &Theme, pl: Placement) {
    let fill = theme.surface_raised().to_egui();
    let stroke = egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui());
    let h = TAIL / 2.0;
    let off = pl.tail_offset;
    let (a, b, apex) = match pl.tail {
        Tail::Up => {
            let cx = LogicalPx(bubble.min.x) + off;
            (
                egui::pos2((cx - h).value(), bubble.min.y),
                egui::pos2((cx + h).value(), bubble.min.y),
                egui::pos2(cx.value(), bubble.min.y - h.value()),
            )
        }
        Tail::Down => {
            let cx = LogicalPx(bubble.min.x) + off;
            (
                egui::pos2((cx - h).value(), bubble.max.y),
                egui::pos2((cx + h).value(), bubble.max.y),
                egui::pos2(cx.value(), bubble.max.y + h.value()),
            )
        }
        Tail::Left => {
            let cy = LogicalPx(bubble.min.y) + off;
            (
                egui::pos2(bubble.min.x, (cy - h).value()),
                egui::pos2(bubble.min.x, (cy + h).value()),
                egui::pos2(bubble.min.x - h.value(), cy.value()),
            )
        }
        Tail::Right => {
            let cy = LogicalPx(bubble.min.y) + off;
            (
                egui::pos2(bubble.max.x, (cy - h).value()),
                egui::pos2(bubble.max.x, (cy + h).value()),
                egui::pos2(bubble.max.x + h.value(), cy.value()),
            )
        }
    };
    p.add(egui::Shape::convex_polygon(
        vec![a, apex, b],
        fill,
        egui::Stroke::NONE,
    ));
    p.line_segment([a, apex], stroke);
    p.line_segment([apex, b], stroke);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0, 800.0))
    }
    const SIZE: egui::Vec2 = egui::Vec2 { x: 244.0, y: 150.0 };

    #[test]
    fn prefers_below_when_room() {
        // 마커가 화면 상단 → 아래에 공간 충분 → below(tail Up).
        let marker = egui::Rect::from_min_size(egui::pos2(400.0, 40.0), egui::vec2(120.0, 60.0));
        let p = place_callout(marker, SIZE, screen(), 12.0, 8.0);
        assert_eq!(p.tail, Tail::Up);
        assert!(p.pos.y > marker.bottom(), "callout below marker");
    }

    #[test]
    fn flips_above_when_no_room_below() {
        // 마커가 화면 하단 → 아래 공간 없음 → above(tail Down).
        let marker = egui::Rect::from_min_size(egui::pos2(400.0, 720.0), egui::vec2(120.0, 60.0));
        let p = place_callout(marker, SIZE, screen(), 12.0, 8.0);
        assert_eq!(p.tail, Tail::Down);
        assert!(p.pos.y + SIZE.y <= marker.top(), "callout above marker");
    }

    #[test]
    fn clamps_into_safe_area() {
        // 마커가 좌측 끝 → below pos.x 가 음수여도 안전영역으로 clamp.
        let marker = egui::Rect::from_min_size(egui::pos2(0.0, 40.0), egui::vec2(40.0, 40.0));
        let p = place_callout(marker, SIZE, screen(), 12.0, 8.0);
        assert!(p.pos.x >= 8.0, "clamped to left safe margin: {}", p.pos.x);
    }

    #[test]
    fn tail_keeps_aiming_after_clamp() {
        // clamp 후에도 tail offset 은 [TAIL, w-TAIL] 범위 내에서 마커 중심을 향한다.
        let marker = egui::Rect::from_min_size(egui::pos2(0.0, 40.0), egui::vec2(40.0, 40.0));
        let p = place_callout(marker, SIZE, screen(), 12.0, 8.0);
        assert!(p.tail_offset >= TAIL && p.tail_offset <= LogicalPx(SIZE.x) - TAIL);
        // 마커 중심 x 는 pos.x + tail_offset 근처.
        let aim_x = LogicalPx(p.pos.x) + p.tail_offset;
        assert!(
            (aim_x - LogicalPx(marker.center().x)).abs() <= LogicalPx(SIZE.x),
            "tail aims near marker center"
        );
    }
}
