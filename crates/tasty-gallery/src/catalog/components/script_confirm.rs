//! `script-confirm` specimen — Lua 스크립트 TOFU 변경 확인 팝업 (Overlays).
//!
//! 본체 `src/adapters/ui/popup/script_confirm.rs::draw_script_confirm_view` 의
//! 구조 전사. 그 view 는 이미 props 분리(`ScriptConfirmProps { theme, name }`)가
//! 끝나 있고 `AppState`/`CoreState` 를 모르지만, 본체 binary 안에 있고 라벨을
//! `t()` 로 직접 조립하므로 갤러리가 호출할 수는 없다 — 같은 순서·같은 토큰으로
//! 복제한다(`docs/dev-guide/gallery-first.md` "이미 본체에만 있는 view").
//!
//! 수직 스택 4단 (`item_spacing.y = spacing_sm`):
//! 1. 제목 — `font_size_body` semibold `text_primary`.
//! 2. 스크립트 이름 — `font_size_caption` **mono** `text_muted`, 넘치면 truncate.
//! 3. 경고 줄 — `tag`(Warning) + `font_size_caption` `text_secondary` 안내문,
//!    `item_spacing.x = spacing_sm`.
//! 4. `spacing_xs` 여백 뒤 푸터 — 우측정렬 `Run anyway`(Primary) / `Cancel`(Ghost).
//!
//! 팝업 기본 크기는 `popup/defs.rs` 의 360×150 — 폭 360 을 그대로 쓴다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, TagVariant, tag};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// `popup/defs.rs` 의 `script_changed_confirm` 기본 폭.
const POPUP_WIDTH: LogicalPx = LogicalPx(360.0);

fn card(ui: &mut egui::Ui, theme: &Theme, name: &str) {
    kit::frame_card(ui, theme, POPUP_WIDTH, kit::panel_fill(theme), |ui| {
        kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();

            // ① 제목 (본체는 font_size_body — kit::title 의 font_size_max 가 아니다).
            ui.label(
                egui::RichText::new("Script changed since registration")
                    .size(theme.font_size_body.value())
                    .strong()
                    .color(theme.text_primary().to_egui()),
            );

            // ② 스크립트 이름 — mono, muted, truncate.
            ui.add(
                egui::Label::new(
                    egui::RichText::new(name)
                        .size(theme.font_size_caption.value())
                        .family(egui::FontFamily::Monospace)
                        .color(theme.text_muted().to_egui()),
                )
                .truncate(),
            );

            // ③ 경고 태그 + 안내문.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                tag(ui, theme, "changed", TagVariant::Warning, false);
                ui.label(
                    egui::RichText::new(
                        "Review it, then run the new version. Its recorded hash will be updated.",
                    )
                    .size(theme.font_size_caption.value())
                    .color(theme.text_secondary().to_egui()),
                );
            });

            ui.add_space(theme.spacing_xs.value());

            // ④ 푸터 — 우측정렬, Run 이 가장 오른쪽.
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    Button::new("Run anyway")
                        .variant(ButtonVariant::Primary)
                        .show(ui, theme);
                    Button::new("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .show(ui, theme);
                });
            });
        });
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
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "360px · bg-panel · popup 기본 크기 360×150"),
            ("name", "font-size-caption mono text-muted · truncate"),
            ("warning", "tag(Warning) + caption text-secondary"),
            ("footer", "Run anyway(Primary) / Cancel(Ghost) · 우측정렬"),
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
