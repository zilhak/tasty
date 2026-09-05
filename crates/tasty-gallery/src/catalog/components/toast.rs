//! Toast 카드 스택 데모 (Tier 3 재분류).
//!
//! 본체 `src/adapters/ui/toast.rs::draw_toast_view` 의 *우측 하단 스택 시각* 을
//! mock props 로 재현. ToastManager 의 *상태 관리* (push / coalesce / lifetime /
//! fade) 는 그대로 유지된다 — 갤러리는 미리 계산된 alpha 만 주입.
//!
//! 갤러리가 본체 binary 에 의존할 수 없어 ToastKind / Entry / Scope 구조를 로컬
//! 미러. props 분리 패턴(`docs/dev-guide/gallery-first.md`).
//!
//! 대표 상태 (6 가지):
//! 1. Single Info (정상)
//! 2. Single Success
//! 3. Single Warning
//! 4. Single Error
//! 5. 긴 메시지 (줄바꿈 wrap)
//! 6. 스택 4 개 (Info → Success → Warning → Error, fade alpha 그라데이션)
//!
//! Note: Tooltip 레이어 위치 결정은 본체에서만 의미가 있으므로 데모는 카드 그룹을
//! 한 frame area 안에 우측 하단 앵커로 그려 *상대 위치* 만 시각화한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::vspace;

use tasty_ui_widgets::tokens::{
    STRUCT_GAP_2, TOAST_GAP, TOAST_MIN_INNER_WIDTH as MIN_TOAST_INNER_WIDTH, TOAST_MIN_MAX_WIDTH,
    TOAST_SCOPE_MARGIN as SCOPE_MARGIN,
};

use crate::catalog::toast_card::{
    self, ACCENT_BAR_WIDTH, PADDING_X, PADDING_Y, ToastKind, accent_color,
};

// ── specimen 무대 치수 ────────────────────────────────────────────────────────
//
// 토스트가 뜨는 "scope" 를 흉내 내는 데모 캔버스 크기다. 디자인 토큰이 아니라
// **무대 크기**라 Theme 에서 오지 않는다 — 케이스마다 다른 것은 그 케이스가 무엇을
// 보여야 하는지(1줄 · wrap · 스택)에 달려 있기 때문이다.

/// 모든 케이스 공통 가로. wrap 케이스가 80% 폭 클램프를 실제로 넘도록 정한 값.
const SPECIMEN_W: LogicalPx = LogicalPx(480.0);
/// 토스트 1 개 케이스의 세로.
const SPECIMEN_H_SINGLE: LogicalPx = LogicalPx(120.0);
/// 여러 줄 wrap 케이스의 세로 — 한 장이 세로로 자란다.
const SPECIMEN_H_WRAP: LogicalPx = LogicalPx(180.0);
/// 4 개 스택 케이스의 세로.
const SPECIMEN_H_STACK: LogicalPx = LogicalPx(280.0);

/// 데모 프레임 좌상단 "scope (frame)" 라벨의 세로 인셋. 4px 그리드 밖(6px)이라
/// spacing 토큰에 대응이 없다 — 프레임 border 와 캡 높이 사이를 눈으로 맞춘 값이다.
const SCOPE_LABEL_INSET_Y: LogicalPx = LogicalPx(6.0);

#[derive(Clone, Debug)]
struct ToastEntryView {
    kind: ToastKind,
    message: String,
    alpha: f32,
}

#[derive(Clone, Debug)]
struct ToastScopeView {
    /// 데모용 — 카드를 그릴 영역 (frame 의 local rect).
    scope_rect: egui::Rect,
    /// id 오름차순 (= 발사 순서). view 가 reverse 해서 위로 쌓는다.
    entries: Vec<ToastEntryView>,
}

struct ToastViewProps<'a> {
    theme: &'a Theme,
    scopes: &'a [ToastScopeView],
}

/// 본체 `draw_toast_view` 의 카드 스택 시각 미러. Tooltip 레이어 대신 painter_at
/// 으로 frame 내부에 직접 그린다.
fn draw_toast_view_mock(ui: &mut egui::Ui, props: &ToastViewProps<'_>) {
    let th = props.theme;
    for scope in props.scopes {
        let scope_rect = scope.scope_rect;
        let painter = ui.painter_at(scope_rect);
        let mut cursor_y = scope_rect.max.y - SCOPE_MARGIN;

        for entry in scope.entries.iter().rev() {
            let alpha = entry.alpha;
            if alpha <= 0.0 {
                continue;
            }

            // 본체 toast.rs 와 동일: 좁은 surface 에서 좌측 누출 방지 클램프
            // (정상 폭에서는 0.8 폭 그대로 = 시각 무변경) + wrap_width 음수 가드.
            let inner_limit = (scope_rect.width() - SCOPE_MARGIN * 2.0).max(MIN_TOAST_INNER_WIDTH);
            let max_width = (scope_rect.width() * 0.8)
                .max(TOAST_MIN_MAX_WIDTH)
                .min(inner_limit);
            let font = egui::FontId::proportional(th.font_size_body.value());
            let text_color = th.text_primary().gamma_multiply(alpha);
            let wrap_width = (max_width - PADDING_X * 2.0 - ACCENT_BAR_WIDTH).max(1.0);

            let galley = ui.ctx().fonts(|f| {
                f.layout(
                    entry.message.clone(),
                    font.clone(),
                    text_color.into(),
                    wrap_width,
                )
            });

            let toast_w = (galley.size().x + PADDING_X * 2.0 + ACCENT_BAR_WIDTH).min(max_width);
            let toast_h = galley.size().y + PADDING_Y * 2.0;

            let max_x = scope_rect.max.x - SCOPE_MARGIN;
            let bottom_y = cursor_y;
            let top_y = bottom_y - toast_h;
            // 본체 toast.rs 와 동일: scope 상단 초과 시 옛 토스트 생략(클립 보완).
            if top_y < scope_rect.min.y {
                break;
            }
            let left_x = max_x - toast_w;

            let rect =
                egui::Rect::from_min_max(egui::pos2(left_x, top_y), egui::pos2(max_x, bottom_y));

            let bg = th.surface_raised().gamma_multiply(alpha);
            // divergence: toast 보더 코드=surface1 이지만 toast_border()=surface0 → 값-보존 border_strong().
            let border = th.border_strong().gamma_multiply(alpha);
            let accent = accent_color(entry.kind, th).gamma_multiply(alpha);

            toast_card::draw_card(
                &painter,
                th,
                rect,
                toast_card::CardColors {
                    bg: bg.into(),
                    border: border.into(),
                    accent,
                    text: text_color.into(),
                },
                galley,
            );

            cursor_y = top_y - TOAST_GAP;
        }
    }
}

/// 카드 그룹을 보여주기 위해 surface1 보더의 영역을 할당하고 그 안에 toast view 호출.
fn frame_case(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    height: f32,
    entries: Vec<ToastEntryView>,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Frame 배경 — bg_panel()(=base) 색으로 *어디에 떠 있는지* 가시화.
    painter.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.bg_panel()),
    );
    painter.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_strong()),
        ),
        egui::StrokeKind::Inside,
    );

    // 좌상단에 "scope" 라벨 — 데모임을 알림.
    painter.text(
        egui::pos2(
            rect.min.x + theme.spacing_sm.value(),
            rect.min.y + SCOPE_LABEL_INSET_Y.value(),
        ),
        egui::Align2::LEFT_TOP,
        "scope (frame)",
        egui::FontId::proportional(theme.font_size_micro.value()),
        // dim 라벨 — 값-동일 text_placeholder()(=placeholder=overlay0 값).
        egui::Color32::from(theme.text_placeholder()),
    );

    let scopes = vec![ToastScopeView {
        scope_rect: rect,
        entries,
    }];
    let props = ToastViewProps {
        theme,
        scopes: &scopes,
    };
    draw_toast_view_mock(ui, &props);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "ToastViewProps + draw_toast_view — AppState/CoreState 비의존 view 함수.",
        )
        .small()
        .color(egui::Color32::from(theme.text_muted())),
    );
    vspace(ui, theme.spacing_xs);
    ui.label(
        egui::RichText::new(
            "Wrapper: src/adapters/ui/toast.rs::ToastManager::draw (상태 관리 + view 호출)",
        )
        .small()
        .color(egui::Color32::from(theme.text_muted())),
    );
    vspace(ui, theme.spacing_md);

    egui::ScrollArea::vertical()
        .id_salt("toast_demo_scroll")
        .show(ui, |ui| {
            // Case 1 — Info
            ui.label(
                egui::RichText::new("Case 1 — Info (blue accent, alpha=1.0)")
                    .strong()
                    .color(egui::Color32::from(theme.text_primary())),
            );
            vspace(ui, STRUCT_GAP_2);
            frame_case(
                ui,
                theme,
                SPECIMEN_W.value(),
                SPECIMEN_H_SINGLE.value(),
                vec![ToastEntryView {
                    kind: ToastKind::Info,
                    message: "Reloaded settings.json".into(),
                    alpha: 1.0,
                }],
            );
            vspace(ui, theme.spacing_lg);

            // Case 2 — Success
            ui.label(
                egui::RichText::new("Case 2 — Success (green accent)")
                    .strong()
                    .color(egui::Color32::from(theme.text_primary())),
            );
            vspace(ui, STRUCT_GAP_2);
            frame_case(
                ui,
                theme,
                SPECIMEN_W.value(),
                SPECIMEN_H_SINGLE.value(),
                vec![ToastEntryView {
                    kind: ToastKind::Success,
                    message: "Workspace saved.".into(),
                    alpha: 1.0,
                }],
            );
            vspace(ui, theme.spacing_lg);

            // Case 3 — Warning
            ui.label(
                egui::RichText::new("Case 3 — Warning (yellow accent)")
                    .strong()
                    .color(egui::Color32::from(theme.text_primary())),
            );
            vspace(ui, STRUCT_GAP_2);
            frame_case(
                ui,
                theme,
                SPECIMEN_W.value(),
                SPECIMEN_H_SINGLE.value(),
                vec![ToastEntryView {
                    kind: ToastKind::Warning,
                    message: "Low disk space — clean up downloads.".into(),
                    alpha: 1.0,
                }],
            );
            vspace(ui, theme.spacing_lg);

            // Case 4 — Error
            ui.label(
                egui::RichText::new("Case 4 — Error (red accent)")
                    .strong()
                    .color(egui::Color32::from(theme.text_primary())),
            );
            vspace(ui, STRUCT_GAP_2);
            frame_case(
                ui,
                theme,
                SPECIMEN_W.value(),
                SPECIMEN_H_SINGLE.value(),
                vec![ToastEntryView {
                    kind: ToastKind::Error,
                    message: "Plugin crashed: tasty-plugin-foo. See logs.".into(),
                    alpha: 1.0,
                }],
            );
            vspace(ui, theme.spacing_lg);

            // Case 5 — Long body (wrap)
            ui.label(
                egui::RichText::new("Case 5 — 긴 본문 (max_width 80% 내 줄바꿈 wrap)")
                    .strong()
                    .color(egui::Color32::from(theme.text_primary())),
            );
            vspace(ui, STRUCT_GAP_2);
            frame_case(
                ui,
                theme,
                SPECIMEN_W.value(),
                SPECIMEN_H_WRAP.value(),
                vec![ToastEntryView {
                    kind: ToastKind::Warning,
                    message:
                        "이것은 매우 긴 toast 메시지로, scope 의 가로 80% 폭을 초과하면 여러 줄에 \
                         걸쳐 wrap 된다. 본체 view 가 ctx.fonts(|f| f.layout(...)) 로 측정하고 \
                         toast 카드 크기를 동적으로 늘린다. 여기서는 mock 으로 같은 알고리즘을 \
                         시연한다."
                            .into(),
                    alpha: 1.0,
                }],
            );
            vspace(ui, theme.spacing_lg);

            // Case 6 — 스택 4 개 (fade 그라데이션)
            ui.label(
                egui::RichText::new(
                    "Case 6 — 4 toast 스택 (id 오름차순: Info → Success → Warning → Error). \
                     alpha 그라데이션으로 fade-in/out 단계 시각화.",
                )
                .strong()
                .color(egui::Color32::from(theme.text_primary())),
            );
            vspace(ui, STRUCT_GAP_2);
            frame_case(
                ui,
                theme,
                SPECIMEN_W.value(),
                SPECIMEN_H_STACK.value(),
                vec![
                    ToastEntryView {
                        kind: ToastKind::Info,
                        message: "Connected to plugin host.".into(),
                        alpha: 0.4, // 가장 오래된 — fade-out 진행
                    },
                    ToastEntryView {
                        kind: ToastKind::Success,
                        message: "Loaded 3 plugins.".into(),
                        alpha: 0.7,
                    },
                    ToastEntryView {
                        kind: ToastKind::Warning,
                        message: "Plugin 'foo' missing signature.".into(),
                        alpha: 1.0,
                    },
                    ToastEntryView {
                        kind: ToastKind::Error,
                        message: "Failed to start 'bar': missing entrypoint.".into(),
                        alpha: 1.0, // 가장 최근 — full opacity
                    },
                ],
            );

            vspace(ui, theme.spacing_md);
            ui.label(
                egui::RichText::new(
                    "⚠ 본체는 Tooltip 레이어에 그려 모든 UI 위에 표시. lifetime (2s) + \
                     fade-in (80ms) / fade-out (160ms) 은 ToastManager 가 매 프레임 \
                     alpha 로 계산해 view 에 전달 — view 는 시간 의존 없음.",
                )
                .small()
                .color(egui::Color32::from(theme.text_muted())),
            );
        });
}
