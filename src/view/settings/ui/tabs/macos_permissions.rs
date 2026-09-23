//! macOS 권한 상태와 시스템 설정 바로가기. Full Disk Access는 사용자가 직접 설정해야 한다.
//! 손쉬운 사용은 surface.raw_key를 사용하는 debug 빌드에서만 표시한다(ADR-0012).
//!
//! Full Disk Access는 보호된 경로의 접근 결과로 추정하며 기능 차단에 사용하지 않는다.
//! 경로가 없어 판단할 수 없으면 미승인이 아닌 확인 불가로 표시한다.
//! 부팅 안내에는 별도의 끄기 설정이 없다.
//! 상태는 이 화면이 재지 않는다. 부팅, 화면 진입, 설정 창 포커스 복귀에서 잰
//! 스냅샷을 읽는다.

use crate::i18n::t;
use tasty_ui_widgets::vspace;

pub fn draw_macos_permissions_tab(ui: &mut egui::Ui) {
    let th = crate::theme::theme();
    // 측정은 파일 열기 syscall 과 TCC 데몬 IPC 라 draw 경로에 두면 repaint 마다 반복되고
    // tccd 가 늦는 만큼 창이 멈춘다. 갱신 시점은
    // `crates/tasty-platform/src/macos_permissions.rs` 를 따른다.
    let snapshot = crate::macos_permissions::permission_snapshot();
    vspace(ui, th.spacing_sm);

    egui::Grid::new("macos_permissions_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.macos_permissions.full_disk_access_label"));
            ui.label(fda_status_text(snapshot.full_disk_access));
            ui.end_row();

            ui.label(t("settings.macos_permissions.screen_recording_label"));
            ui.label(status_text(snapshot.screen_recording));
            ui.end_row();

            #[cfg(debug_assertions)]
            {
                ui.label(t("settings.macos_permissions.accessibility_label"));
                ui.label(status_text(snapshot.accessibility));
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
}

/// Full Disk Access 전용 — 3 상태라 `status_text` 의 bool 로는 못 적는다.
fn fda_status_text(access: crate::macos_permissions::FullDiskAccess) -> egui::RichText {
    use crate::macos_permissions::FullDiskAccess;
    let th = crate::theme::theme();
    match access {
        FullDiskAccess::Granted => {
            egui::RichText::new(t("settings.macos_permissions.status_granted"))
                .color(th.accent_success().to_egui())
        }
        FullDiskAccess::Denied => {
            egui::RichText::new(t("settings.macos_permissions.status_missing"))
                .color(th.text_muted().to_egui())
        }
        FullDiskAccess::Unknown => {
            egui::RichText::new(t("settings.macos_permissions.status_unknown"))
                .color(th.text_muted().to_egui())
        }
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
