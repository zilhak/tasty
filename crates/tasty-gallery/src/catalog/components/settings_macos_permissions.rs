//! 설정 › 일반 › 권한 화면의 specimen. macOS 전용 L2 서브탭이다.
//!
//! 본체 `src/view/settings/ui/tabs/macos_permissions.rs`와 같은
//! `tasty_ui_widgets::mac_permissions`를 호출한다. 본체는 실제 장비의 TCC 상태 하나만
//! 보여주지만, 여기서는 시안의 A~E 조합을 나란히 둔다. 특히 Full Disk Access의 "Unknown"은
//! 추정에 쓰는 경로가 하나도 없는 macOS에서만 나오고, 요청 진행 중 화면은 실제 프롬프트를
//! 띄우지 않고서는 볼 수 없다. 손쉬운 사용 행은 debug 빌드에만 있다(ADR-0012).

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{MacPermissionsView, PermRow, PermState, mac_permissions};

use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 시안 specimen 카드의 폭(primitive `size-560`). 설정 콘텐츠 컬럼 폭에 대응하는 Theme
/// 값이 없다. settings_remote_transfer와 같은 값을 쓴다.
const WIDTH: LogicalPx = LogicalPx(560.0);

const FDA_HINT: &str =
    "macOS has no API to report Full Disk Access, so this state is inferred and can be wrong.";
const FOLDER_HINT: &str =
    "Checking a folder would itself open a permission prompt, so Tasty does not check.";
const DETECTION_NOTE: &str = "These states are a snapshot from startup, from opening this page, \
     and from this window regaining focus. Nothing in Tasty is blocked by them; they only decide \
     whether the startup notice appears.";
const REQUEST_NOTE: &str = "Asks for folder access, then screen recording, one prompt at a time. \
     Items you already allowed or denied are not asked again, and a denied item can only be \
     restored in System Settings. Full Disk Access cannot be requested by an app: use Open \
     System Settings in its row and add Tasty yourself.";
const REQUESTING_NOTE: &str = "Requesting permissions. Answer each prompt as it appears; the next \
     one shows after you answer.";

/// 시안의 시나리오. (Full Disk Access, 화면 기록, 손쉬운 사용)
#[derive(Clone, Copy)]
enum Scenario {
    None,
    All,
    FdaUnknown,
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
        }
    }
}

fn word(state: PermState) -> &'static str {
    match state {
        PermState::Granted => "Granted",
        PermState::Missing => "Not granted",
        PermState::Unknown => "Unknown",
        PermState::NotObservable => "Cannot check automatically",
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    let cases: [(&str, &str, &Theme, Scenario, bool); 7] = [
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
    // 카드 폭 560 두 장이 문서 컬럼에 들어가므로 두 장씩 한 줄에 놓는다.
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        for pair in cases.chunks(2) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
                for &(key, label, th, scenario, debug) in pair {
                    spec::cluster(ui, th, label, |ui| pane(ui, th, key, scenario, debug));
                }
            });
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
            hint: Some(FDA_HINT),
            detail: None,
            tag: None,
            state: fda,
            state_label: word(fda),
            action: Some("Open System Settings"),
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
            hint: Some(FOLDER_HINT),
            detail: Some("Downloads · Documents · Desktop · volumes"),
            tag: None,
            state: PermState::NotObservable,
            state_label: word(PermState::NotObservable),
            action: None,
        },
    ];
    if debug {
        rows.push(PermRow {
            label: "Accessibility (key injection)",
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
                    detection_note: DETECTION_NOTE,
                    request_label: "Request all permissions",
                    request_note: REQUEST_NOTE,
                    requesting_note: REQUESTING_NOTE,
                    requesting: matches!(scenario, Scenario::Requesting),
                },
            );
        });
}
