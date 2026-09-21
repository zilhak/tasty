//! Toast 카드 스택 데모 (Tier 3 재분류).
//!
//! 본체 `src/adapters/ui/toast.rs::draw_toast_view` 의 *우측 하단 스택 시각* 을
//! mock props 로 재현. ToastManager 의 *상태 관리* (push / coalesce / lifetime /
//! fade) 는 그대로 유지된다 — 갤러리는 미리 계산된 alpha 만 주입.
//!
//! 그리기는 본체와 **같은 함수**(`tasty_ui_widgets::draw_toast_scopes`)를 부른다 —
//! 미러가 아니다. props 분리 패턴(`docs/dev-guide/gallery-first.md`).
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

use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::{ToastEntryView, ToastScopeView, ToastViewProps, draw_toast_scopes};

use crate::catalog::toast_card::ToastKind;

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
    // 본체가 부르는 바로 그 함수다. 본체는 Tooltip 레이어 painter 를, 여기는 무대
    // frame 의 painter 를 넘긴다 — 그리는 본문은 하나다.
    draw_toast_scopes(&painter, &props);
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

    // 여섯 케이스를 접지 않고 전부 세운다. 예전에는 `ScrollArea` 로 감쌌는데, 페이지
    // 스크롤 안에 놓인 그 영역이 케이스 1 frame 의 위쪽 일부만큼만 높이를 잡았고,
    // 카드는 frame **우하단**에 앵커되므로 전부 그 클립 밖에 그려져 한 장도 안 보였다.
    // 갤러리는 접으면 캡처에서 사라진다 — 무대는 펼쳐 둔다.
    ui.vertical(|ui| {
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
