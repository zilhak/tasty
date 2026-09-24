//! 공용 draw_toast_scopes로 토스트 종류·줄바꿈·스택을 비교한다.
//! 메시지와 불투명도는 예제 데이터이며 수명·중복 합치기·시간 경과는 실행하지 않는다.
//! 본체의 레이어 위치 대신 예제 영역 오른쪽 아래에 배치한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::vspace;

use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::{ToastEntryView, ToastScopeView, ToastViewProps, draw_toast_scopes};

use crate::catalog::toast_card::ToastKind;

// 한 줄·여러 줄·스택을 비교할 예제 영역의 크기.

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

    painter.text(
        egui::pos2(
            rect.min.x + theme.spacing_sm.value(),
            rect.min.y + SCOPE_LABEL_INSET_Y.value(),
        ),
        egui::Align2::LEFT_TOP,
        "scope (frame)",
        egui::FontId::proportional(theme.font_size_micro.value()),
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
    draw_toast_scopes(&painter, &props);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new("ToastViewProps + draw_toast_scopes — 본체와 공유하는 그리기 함수.")
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

    // 페이지 안에서 각 예제가 잘리지 않도록 모두 펼쳐 놓는다.
    ui.vertical(|ui| {
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
                "본체 ToastManager가 수명과 불투명도를 계산해 전달한다. 그리기 함수는 시간을 재지 않으며, 이 예제는 불투명도를 고정해 비교한다.",
            )
            .small()
            .color(egui::Color32::from(theme.text_muted())),
        );
    });
}
