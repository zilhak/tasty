//! 등록 후 내용이 바뀐 Lua 스크립트의 실행 확인 예제.
//! 콘텐츠는 본체 popup 과 같은 `tasty_ui_widgets::script_confirm` 이 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{ScriptConfirmView, script_confirm};

use crate::i18n::t;

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// `popup/defs.rs` 의 `script_changed_confirm` 기본 폭.
const POPUP_WIDTH: LogicalPx = LogicalPx(360.0);

fn card(ui: &mut egui::Ui, theme: &Theme, name: &str) {
    kit::frame_card(ui, theme, POPUP_WIDTH, kit::panel_fill(theme), |ui| {
        // 여백 12/14 는 공용 위젯이 넣는다.
        script_confirm(
            ui,
            theme,
            &ScriptConfirmView {
                title: t("script.confirm.title"),
                name,
                changed_tag: t("script.confirm.changed_tag"),
                body: t("script.confirm.body"),
                run: t("script.confirm.run"),
                cancel: t("button.cancel"),
            },
        );
    });
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "Changed script", |ui| {
            card(ui, theme, "~/.tasty/scripts/reload-panes.lua")
        });
        spec::cluster(ui, theme, "Long path (truncate)", |ui| {
            card(
                ui,
                theme,
                "~/.tasty/scripts/very/deeply/nested/path/that/overflows/the-card.lua",
            )
        });
        let latte = Theme::with_colors_and_zoom(
            crate::host_shell::latte_theme().to_colors(),
            true,
            theme.ui_zoom,
        );
        spec::cluster(ui, theme, "Latte", |ui| {
            card(ui, &latte, "~/.tasty/scripts/reload-panes.lua")
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "frame",
                "360px · bg-panel · 여백 위아래 space-md · 좌우 14 · 높이는 내용에 맞춘다(최소값 없음)",
            ),
            ("name", "font-size-caption mono text-muted · truncate"),
            (
                "warning",
                "tag(Warning) 단독 줄 · 아래 caption text-secondary 전폭 문단(line-height-ui) · 태그 → 문단 space-xs",
            ),
            ("gap", "행 space-sm · 본문 → 버튼 space-md"),
            (
                "footer",
                "Run anyway(Primary sm) / Cancel(Ghost sm) · 우측정렬",
            ),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new(
                "accent-warning",
                "changed tag",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new("text-muted", "script path", theme.text_muted().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "등록 시점 해시와 현재 파일 해시가 다를 때만 뜬다. Run anyway 로 확정해야 해시가 \
         갱신·영속되고 실행된다 — Escape·Cancel·X 는 모두 실행하지 않는다.",
    );
}
