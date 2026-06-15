//! tastyrc 편집 섹션 (Windows 전용).
//!
//! 구 "Misc" 탭은 General L1 의 L2 섹션으로 분해됨 — Accessibility/Performance 는
//! 각자 `accessibility.rs`/`performance.rs` 가, tastyrc 편집만 여기 남는다.
//! 비-Windows 빌드에서는 공개 항목이 없어 빈 모듈이 된다.

#[cfg(windows)]
use crate::i18n::t;

/// tastyrc 섹션: Tasty 모드에서 적용되는 bashrc 사용자 영역 편집.
#[cfg(windows)]
pub fn draw_tastyrc_subtab(ui: &mut egui::Ui, bashrc_user_draft: &mut Option<String>) {
    let th = crate::theme::theme();

    ui.heading(t("settings.misc.bashrc.heading"));
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(t("settings.misc.bashrc.description"))
            .small()
            .color(th.subtext0),
    );
    ui.add_space(8.0);

    // draft는 mod.rs 진입부에서 lazy 로드되므로 이 시점엔 항상 Some.
    let draft = bashrc_user_draft.get_or_insert_with(crate::settings::general::load_user_bashrc);

    ui.horizontal(|ui| {
        if ui.button(t("settings.misc.bashrc.reset_button")).clicked() {
            *draft = crate::settings::general::INITIAL_USER_BASHRC.to_string();
        }
    });
    ui.add_space(4.0);

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(draft)
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(20)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
        });
}
