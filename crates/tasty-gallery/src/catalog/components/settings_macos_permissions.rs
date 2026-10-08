//! 설정 › 일반 › 권한 화면의 specimen. macOS 전용 L2 서브탭이다.
//!
//! 본체 `src/view/settings/ui/tabs/macos_permissions.rs`와 같은
//! `tasty_ui_widgets::mac_permissions`를 호출한다. 본체는 실제 장비의 TCC 상태 하나만
//! 보여주지만, 여기서는 시안의 A~G 조합을 나란히 둔다. 특히 Full Disk Access의 "Unknown"은
//! 추정에 쓰는 경로가 하나도 없는 macOS에서만 나오고, 요청 진행 중 화면은 실제 프롬프트를
//! 띄우지 않고서는 볼 수 없다. 손쉬운 사용 행은 debug 빌드에만 있다(ADR-0012).

use tasty_platform::macos_permission_notice::{FdaNoticeBranch, fda_settings_detail_key};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{MacPermissionsView, PermRow, PermState, mac_permissions};

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 시안 specimen 카드의 폭(primitive `size-560`). 설정 콘텐츠 컬럼 폭에 대응하는 Theme
/// 값이 없다. settings_remote_transfer와 같은 값을 쓴다.
const WIDTH: LogicalPx = LogicalPx(560.0);

fn fda_hint() -> &'static str {
    crate::i18n::t("settings.macos_permissions.full_disk_access_hint")
}
fn folder_hint() -> &'static str {
    crate::i18n::t("settings.macos_permissions.file_access_hint")
}
fn detection_note() -> &'static str {
    crate::i18n::t("settings.macos_permissions.detection_note")
}
fn request_note() -> &'static str {
    crate::i18n::t("settings.macos_permissions.request_note")
}
fn requesting_note() -> &'static str {
    crate::i18n::t("settings.macos_permissions.requesting")
}

/// 시안의 시나리오. (Full Disk Access, 화면 기록, 손쉬운 사용)
#[derive(Clone, Copy)]
enum Scenario {
    None,
    All,
    FdaUnknown,
    /// 보유했다가 업데이트·재빌드로 잃었다. 상태는 Not granted, FDA 행에 처방 줄이 붙는다.
    FdaStale,
    /// 보유했고 앱은 그대로인데 Tasty 밖에서 꺼졌다. 상태는 Not granted, 다시 켜라는 처방과 재추가 문장이 한 줄에 붙는다.
    FdaRevoked,
    Requesting,
}

impl Scenario {
    fn states(self) -> (PermState, PermState, PermState) {
        match self {
            Scenario::None | Scenario::Requesting => {
                (PermState::Missing, PermState::Missing, PermState::Missing)
            }
            Scenario::All => (PermState::Granted, PermState::Granted, PermState::Granted),
            Scenario::FdaUnknown => (PermState::Unknown, PermState::Granted, PermState::Granted),
            Scenario::FdaStale | Scenario::FdaRevoked => {
                (PermState::Missing, PermState::Granted, PermState::Granted)
            }
        }
    }

    /// 본체가 보유 기록에서 고르는 FDA 갈래. 처방 줄은 이 갈래로 정한다.
    fn branch(self) -> FdaNoticeBranch {
        match self {
            Scenario::FdaStale => FdaNoticeBranch::Stale,
            Scenario::FdaRevoked => FdaNoticeBranch::Revoked,
            _ => FdaNoticeBranch::Never,
        }
    }
}

fn word(state: PermState) -> &'static str {
    match state {
        PermState::Granted => "Granted",
        PermState::Missing => "Not granted",
        PermState::Unknown => "Unknown",
        PermState::NotObservable => {
            crate::i18n::t("settings.macos_permissions.status_not_observable")
        }
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    let cases: [(&str, &str, &Theme, Scenario, bool); 10] = [
        (
            "a",
            "A · nothing granted — Mocha",
            theme,
            Scenario::None,
            false,
        ),
        (
            "a-latte",
            "A · nothing granted — Latte",
            &latte,
            Scenario::None,
            false,
        ),
        ("b", "B · all granted", theme, Scenario::All, false),
        (
            "c",
            "C · FDA unknown, screen granted",
            theme,
            Scenario::FdaUnknown,
            false,
        ),
        (
            "f",
            "F · FDA granted before an update — Mocha",
            theme,
            Scenario::FdaStale,
            false,
        ),
        (
            "f-latte",
            "F · FDA granted before an update — Latte",
            &latte,
            Scenario::FdaStale,
            false,
        ),
        (
            "g",
            "G · FDA turned off outside Tasty (revoked)",
            theme,
            Scenario::FdaRevoked,
            false,
        ),
        ("d", "D · requesting", theme, Scenario::Requesting, false),
        ("e", "E · debug build (4 rows)", theme, Scenario::None, true),
        (
            "e-latte",
            "E · debug build — Latte",
            &latte,
            Scenario::All,
            true,
        ),
    ];
    // 시안처럼 줄바꿈 무대에 놓는다. 카드 폭 560 두 장은 문서 칸에 들어가지 않아 한 줄에 한 장이다.
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for &(key, label, th, scenario, debug) in &cases {
            spec::cluster(ui, th, label, |ui| pane(ui, th, key, scenario, debug));
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            ("L2 position", "last — macOS only, after Display"),
            (
                "columns",
                "label (1fr) · status · row action · gap space-lg",
            ),
            (
                "row",
                "min perm-row-height 32 · pad space-xs · 1px border-default rule",
            ),
            ("label ↔ detail", "label-detail-gap 2"),
            (
                "status",
                "glyph icon-size-sm 14 + word · gap perm-status-gap 4",
            ),
            ("FDA action", "Secondary / Sm, in the FDA row"),
            ("primary", "Request all permissions · Primary / Md"),
            (
                "requesting",
                "button disabled · note line → spinner + text-secondary copy",
            ),
            (
                "notes",
                "caption 11 · line-height-ui · text-muted · wrap at measure-xl",
            ),
            (
                "narrow",
                "status + action wrap to the next line, right-aligned",
            ),
            ("debug row", "Tag \"debug\""),
            (
                "stale grant",
                "no 5th state — Not granted + a caption line under the label with the remedy (remove, then add again)",
            ),
            (
                "revoked",
                "same pattern — Not granted + one caption paragraph: turn it back on, then the re-add sentence (no longer in the list, e.g. after tccutil reset → add it again with +); wraps, no second element; no chip, no new status for either branch",
            ),
        ],
        &[
            TokenChip::new(
                "perm-granted-fg",
                "check",
                theme.perm_granted_fg().to_egui(),
            ),
            TokenChip::new(
                "perm-missing-fg",
                "alertCircle",
                theme.perm_missing_fg().to_egui(),
            ),
            TokenChip::new(
                "perm-unknown-fg",
                "helpCircle",
                theme.perm_unknown_fg().to_egui(),
            ),
            TokenChip::new(
                "perm-unobservable-fg",
                "eyeOff",
                theme.perm_unobservable_fg().to_egui(),
            ),
            TokenChip::without_color("perm-row-height", "→ settings row 32"),
            TokenChip::new(
                "border-default",
                "row rules",
                theme.border_default().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "This is the only place a permission request starts — Tasty raises no prompts at \
         boot (ADR-0052). Unknown (inference failed) and Cannot check (deliberately not looked \
         at) share the muted ink but never the glyph; an empty status cell would read as fine.",
    );
}

/// 시안 specimen 카드 한 장: bg-panel · border-frame · 안쪽 space-lg.
fn pane(ui: &mut egui::Ui, theme: &Theme, key: &str, scenario: Scenario, debug: bool) {
    let (fda, screen, ax) = scenario.states();
    let mut rows = vec![
        PermRow {
            label: "Full Disk Access",
            hint: Some(fda_hint()),
            // 처방 줄은 본체와 같은 함수·번역 키로 골라 사본을 두지 않는다.
            detail: fda_settings_detail_key(fda == PermState::Missing, scenario.branch())
                .map(crate::i18n::t),
            tag: None,
            state: fda,
            state_label: word(fda),
            action: Some(crate::i18n::t(
                "settings.macos_permissions.open_full_disk_access",
            )),
        },
        PermRow {
            label: "Screen recording",
            hint: None,
            detail: None,
            tag: None,
            state: screen,
            state_label: word(screen),
            action: None,
        },
        PermRow {
            label: "Folder access",
            hint: Some(folder_hint()),
            detail: Some(crate::i18n::t(
                "settings.macos_permissions.file_access_detail",
            )),
            tag: None,
            state: PermState::NotObservable,
            state_label: word(PermState::NotObservable),
            action: None,
        },
    ];
    if debug {
        rows.push(PermRow {
            label: crate::i18n::t("settings.macos_permissions.accessibility_label"),
            hint: None,
            detail: None,
            tag: Some("debug"),
            state: ax,
            state_label: word(ax),
            action: None,
        });
    }
    let pad = theme.spacing_lg;
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_frame().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(pad.value() as i8))
        .show(ui, |ui| {
            ui.set_width((WIDTH - pad * 2.0).value());
            // specimen이라 버튼을 눌러도 요청하거나 시스템 설정을 열지 않는다.
            mac_permissions(
                ui,
                theme,
                &MacPermissionsView {
                    id_salt: egui::Id::new("gallery_mac_permissions").with(key),
                    rows: &rows,
                    detection_note: detection_note(),
                    request_label: crate::i18n::t("settings.macos_permissions.request_all"),
                    request_note: request_note(),
                    requesting_note: requesting_note(),
                    requesting: matches!(scenario, Scenario::Requesting),
                },
            );
        });
}
