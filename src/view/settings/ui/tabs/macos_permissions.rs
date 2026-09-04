//! macOS 전용 — 권한(TCC) 상태 표시와 시스템 설정 바로가기.
//!
//! macOS 권한은 앱이 켤 수 없다. 파일 접근 · 화면 기록은 부팅 직후 프롬프트를 미리
//! 띄워 두지만(`src/platform/macos_permissions.rs`), Full Disk Access 는 **요청 API
//! 자체가 없어** 사용자가 시스템 설정에서 직접 추가해야 한다. 이 탭은 그 상태를
//! 보여주고 해당 패널로 바로 보내는 자리다.
//!
//! **손쉬운 사용 행은 debug 빌드에만 있다.** 이 권한을 쓰는 표면(`surface.raw_key`)이
//! debug 로 격리돼 release 에는 소비자가 없고, 프롬프트도 띄우지 않는다
//! ([ADR-0115](../../../../../docs/adr/0115-input-reproduction-ipc-debug-isolation.md)).
//! 소비자가 없는 권한을 release 화면에 남기면 사용자에게 영구히 "미승인" 으로만 보이는,
//! 켤 이유도 끌 이유도 없는 행이 된다. debug 빌드에서는 자기검증 시 승인 상태를 볼
//! 자리가 필요하므로 그대로 둔다.
//!
//! **표시되는 상태는 추정이다** — Full Disk Access 보유 여부를 묻는 공개 API 가 없어
//! "그 권한으로만 읽히는 것으로 알려진 경로가 열리는가" 로 대신한다. 그래서 이 값으로
//! 어떤 기능도 막지 않고, 화면에도 단정하지 않는 문구로 표시한다.

use crate::i18n::t;
use crate::settings::Settings;
use tasty_ui_widgets::vspace;

pub fn draw_macos_permissions_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    egui::Grid::new("macos_permissions_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.macos_permissions.full_disk_access_label"));
            ui.label(status_text(
                crate::macos_permissions::full_disk_access_likely(),
            ));
            ui.end_row();

            ui.label(t("settings.macos_permissions.screen_recording_label"));
            ui.label(status_text(
                crate::macos_permissions::screen_recording_authorized(),
            ));
            ui.end_row();

            #[cfg(debug_assertions)]
            {
                ui.label(t("settings.macos_permissions.accessibility_label"));
                ui.label(status_text(
                    crate::macos_permissions::accessibility_trusted(),
                ));
                ui.end_row();
            }
        });

    vspace(ui, th.spacing_sm);
    ui.label(
        egui::RichText::new(t("settings.macos_permissions.detection_note"))
            .color(th.text_muted().to_egui()),
    );

    vspace(ui, th.spacing_sm);
    if ui
        .button(t("settings.macos_permissions.open_full_disk_access"))
        .clicked()
    {
        crate::macos_permissions::open_full_disk_access_settings();
    }

    vspace(ui, th.spacing_sm);
    // 저장값은 "이미 안내했다" 라 화면 문구와 방향이 반대다 — 체크박스는 "띄운다"
    // 쪽으로 두는 편이 읽기 쉬워서 여기서 뒤집는다.
    let mut show_notice = !settings.general.macos_fda_notice_shown;
    if ui
        .checkbox(
            &mut show_notice,
            t("settings.macos_permissions.show_boot_notice"),
        )
        .changed()
    {
        settings.general.macos_fda_notice_shown = !show_notice;
    }
}

fn status_text(granted: bool) -> egui::RichText {
    let th = crate::theme::theme();
    if granted {
        egui::RichText::new(t("settings.macos_permissions.status_granted"))
            .color(th.accent_success().to_egui())
    } else {
        egui::RichText::new(t("settings.macos_permissions.status_missing"))
            .color(th.text_muted().to_egui())
    }
}
