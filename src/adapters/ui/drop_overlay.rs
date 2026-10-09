//! 외부 drag&drop hover 중 표시되는 시각 피드백.
//!
//! `MainViewState.drop_hover` 가 활성인 동안 terminal_rect 안쪽에 반투명 채움 + 1px 보더와
//! download 글리프 + "Drop to open" 라벨을 그린다(`tasty_ui_widgets::file_drop_overlay`).
//! `HoveredFileCancelled` / `DroppedFile` 직후 사라진다.

use crate::model::PhysicalRect;
use crate::state::MainViewState;

/// 여러 파일을 끌어올 때 라벨 뒤에 붙는 개수 문구. 번역문은 이름 붙은 `{n}` 자리로 개수를 받는다.
fn multi_files_label(count: usize) -> String {
    crate::i18n::t("file_drop.multi_files").replace("{n}", &count.to_string())
}

/// drop hover overlay 를 egui 프레임 마지막에 그린다 (popup 위, plugin popup 아래).
pub fn draw_drop_overlay(
    ctx: &egui::Context,
    state: &MainViewState,
    _engine: &crate::runtime::engine_read::EngineRead<'_>,
    terminal_rect: PhysicalRect,
    scale_factor: f32,
) {
    let Some(hover) = state.drop_hover.as_ref() else {
        return;
    };
    if hover.paths.is_empty() {
        return;
    }
    // explorer 칸 위에서는 칸이 자기 링과 칩으로 복사해 넣을 곳을 보인다.
    if crate::explorer_ui::view::drag::os_hover_claimed(&state.explorer_views) {
        return;
    }

    let theme = crate::theme::theme();

    let rect = crate::adapters::ui::to_egui_rect(terminal_rect, scale_factor);

    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("drop_overlay"));
    let ui = egui::Ui::new(
        ctx.clone(),
        egui::Id::new("drop_overlay"),
        egui::UiBuilder::new().layer_id(layer).max_rect(rect),
    );

    let label = if hover.paths.len() > 1 {
        format!(
            "{}  ({})",
            crate::i18n::t("file_drop.hover_label"),
            multi_files_label(hover.paths.len()),
        )
    } else {
        crate::i18n::t("file_drop.hover_label").to_string()
    };
    tasty_ui_widgets::file_drop_overlay(&ui, &theme, rect, &label);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_count_replaces_the_named_placeholder() {
        crate::i18n::init("en");
        let label = multi_files_label(3);
        assert_eq!(label, "3 files");
        assert!(!label.contains('{'), "{label}");
    }
}
