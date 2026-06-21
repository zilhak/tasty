//! Update-available popup.
//!
//! Shows the current version, the latest detected version, the release notes
//! (plain text), and a button to open the GitHub Releases page. A "Check now"
//! button forces an immediate poll. Phase 1 — no in-app download.
//!
//! ## Tier 3 분리
//!
//! - `UpdateProps` / `UpdateStatusView` / `UpdateAction` — AppState 와 무관한
//!   순수 데이터/액션. 갤러리에서 mock 으로 모든 상태를 단독 검증 가능.
//! - `draw_update_view` — side-effect 없는 순수 view 함수. props 만 입력으로
//!   받아 사용자 의도를 `UpdateAction` 으로 반환.
//! - `draw_update_popup` — 본체 wrapper. AppState 에서 snapshot → props 빌드 →
//!   view 호출 → action 처리 (브라우저 열기 / 체크 트리거).

use tasty_type_appearance::theme::Theme;

use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;

pub const UPDATE_POPUP_ID: &str = "update_check";

/// View 전용 update 상태 스냅샷.
///
/// `AppState::update_status` 의 `Arc<Mutex<UpdateStatus>>` 와 분리된 owned 데이터.
#[derive(Debug, Clone)]
pub enum UpdateStatusView {
    /// 아직 체크된 적 없음 (`last_checked.is_none()` && `!in_flight`).
    NeverChecked,
    /// 체크 중 (`in_flight`).
    Checking,
    /// 최신 버전 (체크 완료, latest=None, 에러 없음).
    UpToDate,
    /// 새 버전 발견.
    Available {
        version: String,
        body: String,
        html_url: String,
    },
    /// 마지막 체크가 실패 (latest=None, last_error=Some).
    Failed { reason: String },
}

/// `draw_update_view` 의 입력. AppState/CoreState 와 무관.
#[derive(Debug, Clone)]
pub struct UpdateProps {
    pub current_version: String,
    pub status: UpdateStatusView,
}

/// View 함수가 호출처에 보고하는 사용자 의도. side-effect 없음.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateAction {
    /// 아무 일도 없음.
    None,
    /// popup 닫기 요청 (Escape 등).
    Close,
    /// 사용자가 "release page 열기" 버튼 누름.
    OpenReleasePage(String),
    /// 사용자가 "지금 체크" 버튼 누름.
    CheckNow,
}

/// AppState 의 `update_status` snapshot 으로부터 view props 를 빌드한다.
///
/// `version` 은 호출처가 `env!("CARGO_PKG_VERSION")` 등으로 주입.
pub fn build_update_props(
    current_version: &str,
    snapshot: &crate::state::update_check::UpdateStatus,
) -> UpdateProps {
    let status = match &snapshot.latest {
        Some(info) => UpdateStatusView::Available {
            version: info.version.clone(),
            body: info.body.clone(),
            html_url: info.html_url.clone(),
        },
        None => {
            if let Some(reason) = snapshot.localized_error() {
                UpdateStatusView::Failed { reason }
            } else if snapshot.in_flight {
                UpdateStatusView::Checking
            } else if snapshot.last_checked.is_none() {
                UpdateStatusView::NeverChecked
            } else {
                UpdateStatusView::UpToDate
            }
        }
    };

    UpdateProps {
        current_version: current_version.to_string(),
        status,
    }
}

/// 순수 view 함수.
///
/// AppState/CoreState 의존 0, side-effect 0 — 사용자 의도만 `UpdateAction` 으로
/// 반환한다. 가장 우선순위 높은 action 하나를 돌려준다.
pub fn draw_update_view(ui: &mut egui::Ui, th: &Theme, props: &UpdateProps) -> UpdateAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return UpdateAction::Close;
    }

    let mut action = UpdateAction::None;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;

        ui.label(
            egui::RichText::new(t("update.heading"))
                .color(th.text)
                .size(13.0),
        );
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t("update.current_label")).color(th.subtext0));
            ui.label(
                egui::RichText::new(&props.current_version)
                    .color(th.text)
                    .strong(),
            );
        });

        match &props.status {
            UpdateStatusView::Available {
                version,
                body,
                html_url,
            } => {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(t("update.latest_label")).color(th.subtext0));
                    ui.label(egui::RichText::new(version).color(th.accent_success()).strong());
                });
                ui.separator();
                ui.label(
                    egui::RichText::new(t("update.notes_label"))
                        .color(th.subtext0)
                        .size(12.0),
                );
                egui::ScrollArea::vertical()
                    .max_height(180.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(egui::RichText::new(body).color(th.text).size(12.0))
                                .wrap(),
                        );
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t("update.open_release")).clicked() {
                        action = UpdateAction::OpenReleasePage(html_url.clone());
                    }
                    if ui.button(t("update.check_now")).clicked() {
                        action = UpdateAction::CheckNow;
                    }
                });
            }
            other => {
                match other {
                    UpdateStatusView::Failed { reason } => {
                        ui.label(
                            egui::RichText::new(format!("{}: {reason}", t("update.error_label")))
                                .color(th.accent_danger())
                                .size(12.0),
                        );
                    }
                    UpdateStatusView::NeverChecked => {
                        ui.label(egui::RichText::new(t("update.never_checked")).color(th.subtext0));
                    }
                    UpdateStatusView::UpToDate => {
                        ui.label(egui::RichText::new(t("update.up_to_date")).color(th.accent_success()));
                    }
                    UpdateStatusView::Checking => {
                        // 메시지는 아래 in_flight 라벨이 담당.
                    }
                    UpdateStatusView::Available { .. } => unreachable!(),
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(t("update.check_now")).clicked() {
                        action = UpdateAction::CheckNow;
                    }
                    if matches!(other, UpdateStatusView::Checking) {
                        ui.label(
                            egui::RichText::new(t("update.checking"))
                                .color(th.subtext0)
                                .italics(),
                        );
                    }
                });
            }
        }
    });

    action
}

/// 본체 wrapper. PopupDef::draw_fn 시그니처에 맞춰 AppState/CoreState 를 받는다.
pub fn draw_update_popup(
    ui: &mut egui::Ui,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) -> PopupAction {
    let th = theme::theme();
    let snapshot = state.update_status.lock().unwrap().clone();
    let props = build_update_props(env!("CARGO_PKG_VERSION"), &snapshot);

    let action = draw_update_view(ui, &th, &props);

    match action {
        UpdateAction::None => PopupAction::None,
        UpdateAction::Close => PopupAction::Close,
        UpdateAction::OpenReleasePage(url) => {
            if let Err(e) = webbrowser::open(&url) {
                tracing::warn!("update popup: open browser failed: {e}");
            }
            PopupAction::None
        }
        UpdateAction::CheckNow => {
            trigger_check(state);
            PopupAction::None
        }
    }
}

fn trigger_check(state: &AppState) {
    crate::state::update_check::trigger_check(
        state.update_status.clone(),
        "zilhak",
        "tasty",
        env!("CARGO_PKG_VERSION"),
    );
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use tasty_update::ReleaseInfo;

    use super::*;
    use crate::state::update_check::UpdateStatus;

    fn release(version: &str) -> ReleaseInfo {
        ReleaseInfo {
            version: version.to_string(),
            parsed: semver::Version::parse(version).unwrap(),
            html_url: format!("https://example.com/{version}"),
            body: format!("notes for {version}"),
            assets: vec![],
        }
    }

    #[test]
    fn props_never_checked() {
        let snap = UpdateStatus::default();
        let p = build_update_props("0.1.0", &snap);
        assert!(matches!(p.status, UpdateStatusView::NeverChecked));
        assert_eq!(p.current_version, "0.1.0");
    }

    #[test]
    fn props_checking_when_in_flight() {
        let snap = UpdateStatus {
            in_flight: true,
            ..Default::default()
        };
        let p = build_update_props("0.1.0", &snap);
        assert!(matches!(p.status, UpdateStatusView::Checking));
    }

    #[test]
    fn props_up_to_date_after_check_with_no_result() {
        let snap = UpdateStatus {
            last_checked: Some(Instant::now()),
            ..Default::default()
        };
        let p = build_update_props("0.1.0", &snap);
        assert!(matches!(p.status, UpdateStatusView::UpToDate));
    }

    #[test]
    fn props_failed_when_last_error_set() {
        let snap = UpdateStatus {
            last_error: Some("network".into()),
            last_checked: Some(Instant::now()),
            ..Default::default()
        };
        let p = build_update_props("0.1.0", &snap);
        let UpdateStatusView::Failed { reason } = p.status else {
            panic!("expected Failed");
        };
        assert_eq!(reason, "network");
    }

    #[test]
    fn props_available_carries_release_fields() {
        let snap = UpdateStatus {
            latest: Some(release("0.2.0")),
            last_checked: Some(Instant::now()),
            ..Default::default()
        };
        let p = build_update_props("0.1.0", &snap);
        let UpdateStatusView::Available {
            version,
            body,
            html_url,
        } = p.status
        else {
            panic!("expected Available");
        };
        assert_eq!(version, "0.2.0");
        assert_eq!(body, "notes for 0.2.0");
        assert_eq!(html_url, "https://example.com/0.2.0");
    }
}
