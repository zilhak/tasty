//! macOS 전용 — 권한(TCC) 상태 표시 · 일괄 요청 · 시스템 설정 바로가기.
//! 손쉬운 사용은 surface.raw_key를 사용하는 debug 빌드에서만 표시한다(ADR-0012).
//!
//! 권한 요청이 시작되는 유일한 자리다. 부팅 직후 자동 발화는 없앴다
//! ([ADR-0052](../../../../../docs/adr/0052-permission-prompts-are-raised-on-request-not-at-boot.md)).
//! [모든 권한 요청하기] 는 파일 폴더 → 화면 기록 순으로 하나씩 요청한다.
//! Full Disk Access 는 요청 API 가 없어 사용자가 시스템 설정에서 직접 추가해야 한다.
//!
//! Full Disk Access는 보호된 경로의 접근 결과로 추정하며 기능 차단에 사용하지 않는다.
//! 경로가 없어 판단할 수 없으면 미승인이 아닌 확인 불가로 표시한다.
//! 파일 폴더는 조회 수단이 없어 상태 대신 "조회 수단 없음" 을 적는다.
//! 부팅 안내에는 별도의 끄기 설정이 없다.
//! 상태는 이 화면이 재지 않는다. 부팅, 화면 진입, 설정 창 포커스 복귀, 요청 완료에서
//! 잰 스냅샷을 읽는다.

use crate::i18n::t;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, vspace};

pub fn draw_macos_permissions_tab(ui: &mut egui::Ui) {
    let th = crate::theme::theme();
    // 측정은 파일 열기 syscall 과 TCC 데몬 IPC 라 draw 경로에 두면 repaint 마다 반복되고
    // tccd 가 늦는 만큼 창이 멈춘다. 갱신 시점은
    // `crates/tasty-platform/src/macos_permissions.rs` 를 따른다.
    let snapshot = crate::macos_permissions::permission_snapshot();
    let requesting = crate::macos_permissions::permission_request_running();
    vspace(ui, th.spacing_sm);

    egui::Grid::new("macos_permissions_grid")
        .num_columns(2)
        .spacing([th.spacing_md.value(), th.spacing_sm.value()])
        .show(ui, |ui| {
            ui.label(t("settings.macos_permissions.full_disk_access_label"));
            ui.label(fda_status_text(snapshot.full_disk_access));
            ui.end_row();

            ui.label(t("settings.macos_permissions.screen_recording_label"));
            ui.label(status_text(snapshot.screen_recording));
            ui.end_row();

            ui.label(t("settings.macos_permissions.file_access_label"));
            ui.label(
                egui::RichText::new(t("settings.macos_permissions.status_not_observable"))
                    .color(th.text_muted().to_egui()),
            );
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

    vspace(ui, th.spacing_md);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        // 요청이 도는 동안에는 비활성이다 — 두 번째 시퀀스가 시작되면 "하나씩 순차로"
        // 전제가 깨져 프롬프트가 포개져 뜬다. 플랫폼 쪽에도 같은 판정이 있어(원자 교환)
        // 이 비활성은 그 거부를 눈에 보이게 하는 것이지 유일한 방어가 아니다.
        if Button::new(t("settings.macos_permissions.request_all"))
            .variant(ButtonVariant::Primary)
            .size(ControlSize::Md)
            .enabled(!requesting)
            .show(ui, &th)
            .clicked()
        {
            // 요청이 끝나는 시점에는 사용자 입력이 없어 repaint 가 저절로 안 난다.
            // 워커가 끝나면 이 컨텍스트를 깨워 갱신된 상태를 그리게 한다.
            let ctx = ui.ctx().clone();
            crate::macos_permissions::request_all_permissions(move || ctx.request_repaint());
        }
        if Button::new(t("settings.macos_permissions.open_full_disk_access"))
            .variant(ButtonVariant::Secondary)
            .size(ControlSize::Md)
            .show(ui, &th)
            .clicked()
        {
            crate::macos_permissions::open_full_disk_access_settings();
        }
    });

    vspace(ui, th.spacing_sm);
    let hint = if requesting {
        t("settings.macos_permissions.requesting")
    } else {
        t("settings.macos_permissions.request_note")
    };
    ui.label(egui::RichText::new(hint).color(th.text_muted().to_egui()));
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
