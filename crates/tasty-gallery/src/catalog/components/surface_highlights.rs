//! Surface highlight overlay 데모 (Tier 3 재분류).
//!
//! 본체 `src/adapters/ui/divider.rs::draw_surface_highlights_view` 와 동등한
//! 시각을 mock props 로 재현. AppState/CoreState 비의존이라는 props 분리의 성과를
//! 가시화한다.
//!
//! 본체 의존: 0. 본체 view 변경 시 시각 동기화는 수동 검증 (gallery 가 binary
//! crate `tasty` 에 의존 불가).
//!
//! 본체 view 는 `ctx.layer_painter(Order::Middle)` 로 전역 좌표에 그리므로 갤러리
//! 패널 안에 직접 mirror 할 수 없다. 대신 데모는 *동일한 stroke / 좌표 변환 식* 을
//! 로컬 ui-relative 좌표로 그려, "어떤 rect 가 highlight 됐을 때 어떻게 보이는가"
//! 를 단독 검증할 수 있게 한다.

use tasty_type_appearance::theme::Theme;

/// 본체 `SurfaceHighlightRegion` 와 동등한 로컬 mock.
#[derive(Debug, Clone, Copy)]
struct MockRegion {
    /// 데모 패널 내 로컬 좌표 (logical px).
    rect: egui::Rect,
    is_highlighted: bool,
}

fn draw_mock_highlights(ui: &mut egui::Ui, theme: &Theme, regions: &[MockRegion], frame_h: f32) {
    let (frame_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().min(560.0), frame_h),
        egui::Sense::hover(),
    );
    let painter = ui.painter_at(frame_rect);

    let bg: egui::Color32 = theme.crust.into();
    let pane_bg: egui::Color32 = theme.surface0.into();
    let stroke: egui::Color32 = theme.accent_primary().into();
    let label_color: egui::Color32 = theme.subtext0.into();

    painter.rect_filled(frame_rect, theme.corner_radius.value(), bg);

    for region in regions {
        let translated = region
            .rect
            .translate(frame_rect.min.to_vec2() + egui::vec2(8.0, 8.0));
        painter.rect_filled(translated, 2.0, pane_bg);
        if region.is_highlighted {
            painter.rect_stroke(
                translated,
                0.0,
                egui::Stroke::new(2.0, stroke),
                egui::StrokeKind::Inside,
            );
        }
        let tag = if region.is_highlighted {
            "highlighted"
        } else {
            "idle"
        };
        painter.text(
            egui::pos2(translated.min.x + 4.0, translated.min.y + 4.0),
            egui::Align2::LEFT_TOP,
            tag,
            egui::FontId::proportional(theme.font_size_micro.value()),
            label_color,
        );
    }
}

/// 대표 mock 상태 5 종:
/// 1. 단일 surface highlighted — 가장 기본 모양 (notification on)
/// 2. 2×1 split — 한 쪽만 highlight (인접 비교)
/// 3. 2×2 grid — 대각선 두 칸 highlight (다중 highlight)
/// 4. 좁고 긴 surface — 가로 grid, edge case
/// 5. 작은 surface 다수 (6 개 grid) — 일부만 highlight (밀집 시 가독성)
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "SurfaceHighlightsProps + draw_surface_highlights_view — AppState/CoreState 비의존.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(8.0);

    // ① 단일 surface highlight
    ui.label(
        egui::RichText::new("① 단일 surface — focused notification on:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    draw_mock_highlights(
        ui,
        theme,
        &[MockRegion {
            rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(520.0, 120.0)),
            is_highlighted: true,
        }],
        144.0,
    );
    ui.add_space(16.0);

    // ② 2×1 split, 한쪽만 highlight
    ui.label(
        egui::RichText::new("② 2×1 split — 왼쪽만 highlight:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    draw_mock_highlights(
        ui,
        theme,
        &[
            MockRegion {
                rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(254.0, 120.0)),
                is_highlighted: true,
            },
            MockRegion {
                rect: egui::Rect::from_min_size(egui::pos2(264.0, 0.0), egui::vec2(256.0, 120.0)),
                is_highlighted: false,
            },
        ],
        144.0,
    );
    ui.add_space(16.0);

    // ③ 2×2 grid — 대각선 highlight
    ui.label(
        egui::RichText::new("③ 2×2 grid — 대각선 highlight (다중):")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    draw_mock_highlights(
        ui,
        theme,
        &[
            MockRegion {
                rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(254.0, 90.0)),
                is_highlighted: true,
            },
            MockRegion {
                rect: egui::Rect::from_min_size(egui::pos2(264.0, 0.0), egui::vec2(256.0, 90.0)),
                is_highlighted: false,
            },
            MockRegion {
                rect: egui::Rect::from_min_size(egui::pos2(0.0, 100.0), egui::vec2(254.0, 90.0)),
                is_highlighted: false,
            },
            MockRegion {
                rect: egui::Rect::from_min_size(egui::pos2(264.0, 100.0), egui::vec2(256.0, 90.0)),
                is_highlighted: true,
            },
        ],
        214.0,
    );
    ui.add_space(16.0);

    // ④ 가로 split (좁고 긴 surface) — edge case
    ui.label(
        egui::RichText::new("④ 좁고 긴 가로 split — 가운데 highlight:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    draw_mock_highlights(
        ui,
        theme,
        &[
            MockRegion {
                rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(166.0, 120.0)),
                is_highlighted: false,
            },
            MockRegion {
                rect: egui::Rect::from_min_size(egui::pos2(176.0, 0.0), egui::vec2(166.0, 120.0)),
                is_highlighted: true,
            },
            MockRegion {
                rect: egui::Rect::from_min_size(egui::pos2(352.0, 0.0), egui::vec2(168.0, 120.0)),
                is_highlighted: false,
            },
        ],
        144.0,
    );
    ui.add_space(16.0);

    // ⑤ 6 surface grid — 일부 highlight
    ui.label(
        egui::RichText::new("⑤ 3×2 grid (6 surface) — 2 개 highlight, 밀집 시 가독성:")
            .color(egui::Color32::from(theme.text)),
    );
    ui.add_space(4.0);
    let cell_w = 166.0;
    let cell_h = 84.0;
    let gap = 10.0;
    let highlighted_indices = [1usize, 4];
    let regions: Vec<MockRegion> = (0..6)
        .map(|i| {
            let col = (i % 3) as f32;
            let row = (i / 3) as f32;
            MockRegion {
                rect: egui::Rect::from_min_size(
                    egui::pos2(col * (cell_w + gap), row * (cell_h + gap)),
                    egui::vec2(cell_w, cell_h),
                ),
                is_highlighted: highlighted_indices.contains(&i),
            }
        })
        .collect();
    draw_mock_highlights(ui, theme, &regions, 2.0 * cell_h + gap + 16.0);

    ui.add_space(12.0);
    ui.label(
        egui::RichText::new(
            "⚠ 본체 view 는 ctx.layer_painter(Order::Middle) 로 전역 좌표에 그림 — \
             gallery 는 패널 로컬 좌표로 재현 (시각 식별 목적).",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}
