//! macOS 전용 화면. 권한 상태를 보여주고, 한 번에 요청하고, 시스템 설정으로 보낸다.
//!
//! 권한 요청은 여기에서만 시작한다. 부팅 직후에 자동으로 요청하지는 않는다
//! ([ADR-0052](../../../../../docs/adr/0052-permission-prompts-are-raised-on-request-not-at-boot.md)).
//! [모든 권한 요청하기]를 누르면 파일 폴더와 화면 기록을 순서대로 하나씩 요청한다.
//! Full Disk Access는 요청하는 API가 없어 사용자가 시스템 설정에서 직접 추가해야 한다.
//! 손쉬운 사용은 `surface.raw_key`를 쓰는 debug 빌드에서만 표시한다(ADR-0012).
//!
//! Full Disk Access 상태는 보호된 경로에 접근해 본 결과로 추정하며, 이 값으로 기능을
//! 막지는 않는다. 경로가 없어 판단할 수 없으면 미승인이 아니라 확인 불가로 표시한다.
//! 폴더 접근은 확인하려는 동작 자체가 권한 요청을 띄워 "자동 확인 불가"로 적는다. 부팅
//! 안내를 끄는 설정은 따로 두지 않는다.
//!
//! 화면 배치는 갤러리와 같은 `tasty_ui_widgets::mac_permissions`가 그린다. 이 파일은 측정해
//! 둔 상태와 번역 문구를 넘기고 눌린 버튼을 처리한다.
//!
//! 상태는 이 화면에서 측정하지 않는다. 부팅, 화면 진입, 설정 창 포커스 복귀, 요청 완료
//! 시점에 측정해 둔 스냅샷을 읽기만 한다.

use crate::i18n::t;
use tasty_ui_widgets::{MacPermissionsView, PermRow, PermState, mac_permissions};

/// 표에서 Full Disk Access 행의 위치. 이 행의 행 액션이 시스템 설정을 연다.
const FDA_ROW: usize = 0;

pub fn draw_macos_permissions_tab(ui: &mut egui::Ui) {
    let th = crate::theme::theme();
    // 측정은 파일 열기 syscall과 TCC 데몬 IPC라서, 그리는 경로에 두면 repaint마다
    // 반복되고 응답이 늦는 만큼 창이 멈춘다. 갱신 시점은
    // `crates/tasty-platform/src/macos_permissions.rs`에 정리해 두었다.
    let snapshot = crate::macos_permissions::permission_snapshot();
    let requesting = crate::macos_permissions::permission_request_running();

    let fda = fda_state(snapshot.full_disk_access);
    let screen = bool_state(snapshot.screen_recording);
    let mut rows = vec![
        PermRow {
            label: t("settings.macos_permissions.full_disk_access_label"),
            hint: Some(t("settings.macos_permissions.full_disk_access_hint")),
            detail: fda_detail(fda, crate::macos_permissions::notice_inputs().0),
            tag: None,
            state: fda,
            state_label: state_label(fda),
            action: Some(t("settings.macos_permissions.open_full_disk_access")),
        },
        PermRow {
            label: t("settings.macos_permissions.screen_recording_label"),
            hint: None,
            detail: None,
            tag: None,
            state: screen,
            state_label: state_label(screen),
            action: None,
        },
        PermRow {
            label: t("settings.macos_permissions.file_access_label"),
            hint: Some(t("settings.macos_permissions.file_access_hint")),
            detail: Some(t("settings.macos_permissions.file_access_detail")),
            tag: None,
            state: PermState::NotObservable,
            state_label: state_label(PermState::NotObservable),
            action: None,
        },
    ];
    rows.extend(accessibility_row(&snapshot));

    let out = mac_permissions(
        ui,
        &th,
        &MacPermissionsView {
            id_salt: egui::Id::new("settings_macos_permissions"),
            rows: &rows,
            detection_note: t("settings.macos_permissions.detection_note"),
            request_label: t("settings.macos_permissions.request_all"),
            request_note: t("settings.macos_permissions.request_note"),
            requesting_note: t("settings.macos_permissions.requesting"),
            // 요청이 진행 중일 때는 버튼을 비활성으로 둔다. 요청이 겹치면 프롬프트를 하나씩
            // 띄운다는 전제가 깨져 여러 개가 포개진다. 플랫폼 쪽에서도 같은 조건을 막고
            // 있으므로, 이 비활성은 그 거부를 눈에 보이게 하는 역할이다.
            requesting,
        },
    );
    if out.request_clicked {
        // 요청이 끝나는 시점에는 사용자 입력이 없어 화면이 저절로 다시 그려지지
        // 않는다. 워커가 끝나면 이 컨텍스트를 깨워 갱신된 상태를 그리게 한다.
        let ctx = ui.ctx().clone();
        crate::macos_permissions::request_all_permissions(move || ctx.request_repaint());
    }
    if out.row_action == Some(FDA_ROW) {
        crate::macos_permissions::open_full_disk_access_settings();
    }
}

/// 손쉬운 사용 행. `surface.raw_key`를 쓰는 debug 빌드에만 있다(ADR-0012).
#[cfg(debug_assertions)]
fn accessibility_row(
    snapshot: &crate::macos_permissions::PermissionSnapshot,
) -> Option<PermRow<'static>> {
    let ax = bool_state(snapshot.accessibility);
    Some(PermRow {
        label: t("settings.macos_permissions.accessibility_label"),
        hint: None,
        detail: None,
        tag: Some(t("settings.macos_permissions.debug_tag")),
        state: ax,
        state_label: state_label(ax),
        action: None,
    })
}

#[cfg(not(debug_assertions))]
fn accessibility_row(
    _snapshot: &crate::macos_permissions::PermissionSnapshot,
) -> Option<PermRow<'static>> {
    None
}

/// Full Disk Access는 추정 실패가 있어 세 상태다.
fn fda_state(access: crate::macos_permissions::FullDiskAccess) -> PermState {
    use crate::macos_permissions::FullDiskAccess;
    match access {
        FullDiskAccess::Granted => PermState::Granted,
        FullDiskAccess::Denied => PermState::Missing,
        FullDiskAccess::Unknown => PermState::Unknown,
    }
}

/// Full Disk Access 행의 보조 줄. 보유했다가 앱이 바뀐 뒤 잃은 경우에만 처방을 적는다.
/// 상태는 그대로 "허용 안 됨"이고 다섯 번째 상태를 만들지 않는다.
fn fda_detail(
    fda: PermState,
    branch: crate::macos_permissions::FdaNoticeBranch,
) -> Option<&'static str> {
    (fda == PermState::Missing && branch == crate::macos_permissions::FdaNoticeBranch::Stale)
        .then(|| t("settings.macos_permissions.full_disk_access_stale_detail"))
}

fn bool_state(granted: bool) -> PermState {
    if granted {
        PermState::Granted
    } else {
        PermState::Missing
    }
}

fn state_label(state: PermState) -> &'static str {
    match state {
        PermState::Granted => t("settings.macos_permissions.status_granted"),
        PermState::Missing => t("settings.macos_permissions.status_missing"),
        PermState::Unknown => t("settings.macos_permissions.status_unknown"),
        PermState::NotObservable => t("settings.macos_permissions.status_not_observable"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::macos_permissions::FdaNoticeBranch;

    #[test]
    fn only_a_stale_grant_adds_the_remedy_line_to_a_missing_row() {
        assert!(fda_detail(PermState::Missing, FdaNoticeBranch::Stale).is_some());
        assert!(fda_detail(PermState::Missing, FdaNoticeBranch::Never).is_none());
        assert!(fda_detail(PermState::Missing, FdaNoticeBranch::Revoked).is_none());
        assert!(fda_detail(PermState::Granted, FdaNoticeBranch::Stale).is_none());
    }
}
