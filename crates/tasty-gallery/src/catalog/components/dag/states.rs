//! 비-정상 상태 — 디자인 `DagEmpty` 두 변형 + `CycleBanner`.
//!
//! 사이클은 **숨기지 않는다**: 배너를 캔버스 상단에 고정하고 그래프는 뒤에서
//! 그대로 그린다. 이 서피스는 관찰용이라 상태를 가리는 쪽이 더 나쁘다.

use tasty_type_appearance::theme::Theme;

use super::{canvas, chrome};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 빈 상태 무대 한 칸의 최소 높이 — 시안 160.
fn empty_height(theme: &Theme) -> f32 {
    theme.dag_detail_log_max_height().value()
}

fn empty_box(ui: &mut egui::Ui, theme: &Theme, query: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), empty_height(theme)),
        egui::Sense::hover(),
    );
    let radius = theme.corner_radius.value();
    ui.painter()
        .rect_filled(rect, radius, theme.bg_panel().to_egui());
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    chrome::paint_empty(ui, theme, rect, query);
}

/// `states` 섹션 Spec — 빈 상태 2 종 + 사이클 경고.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let cycle = super::cycle_dag();
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        empty_box(ui, theme, None);
        empty_box(ui, theme, Some("deploy"));
        let ids = cycle.cycle.clone().unwrap_or_default();
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), theme.dag_cycle_height().value()),
            egui::Sense::hover(),
        );
        chrome::paint_cycle_banner(ui, theme, rect, &ids);
    });
    canvas::cycle_stage(ui, theme);
    spec::meta(
        ui,
        theme,
        &[
            ("banner", "28px · pinned to canvas top"),
            ("banner copy", "names the cycle path"),
            ("empty A", "surface — how a DAG appears"),
            ("empty B", "search — echoes the query"),
        ],
        &[
            TokenChip::new(
                "--tasty-dag-cycle-bg",
                "banner wash",
                theme.dag_cycle_bg().to_egui(),
            ),
            TokenChip::new(
                "--tasty-dag-cycle-fg",
                "banner text",
                theme.dag_cycle_fg().to_egui(),
            ),
            TokenChip::new(
                "--tasty-text-disabled",
                "empty glyph",
                theme.text_disabled().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "The cyclic graph keeps rendering behind the banner — this surface observes, it never \
         hides state. Layering is longest-path and capped by node count, so a cyclic graph still \
         terminates and still draws.",
    );
}
