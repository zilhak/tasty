//! 설정 › 일반 › 권한 화면의 specimen. macOS 전용 L2 서브탭이다.
//!
//! 본체는 `src/view/settings/ui/tabs/macos_permissions.rs`의
//! `draw_macos_permissions_tab`이다. 이 화면은 디자인 시안 없이 본체와 같은 토큰·위젯으로
//! 조립했다. 권한 요청 버튼과 진행 표시는 [ADR-0052]로 새로 생긴 요소라 어느 시안에도
//! 없다. 나중에 시안이 오면 이 specimen이 대조 기준이 된다.
//!
//! 콘텐츠 컬럼은 라벨과 상태 두 열로 된 grid(Full Disk Access · 화면 기록 · 파일 폴더,
//! debug 빌드에서는 손쉬운 사용 한 행 추가), 그 아래 muted 주석, 버튼 행
//! (primary "Request all permissions", secondary "Open Full Disk Access settings"),
//! muted 설명 줄로 이루어진다. 갤러리는 본체에 의존하지 않으므로 TCC와 플랫폼에 의존하는
//! `draw_macos_permissions_tab`을 직접 호출하지 않고 같은 위젯과 토큰으로 같은 화면을
//! 다시 만든다. settings_remote_transfer도 같은 방식이다.
//!
//! 이 specimen의 쓸모는 상태 조합을 한자리에서 볼 수 있다는 점이다. 본체에서는 실제
//! 장비의 TCC 상태 하나만 보인다. 특히 Full Disk Access의 "Unknown"은 추정에 쓰는 경로가
//! 하나도 없는 macOS에서만 나와 재현하기 어렵고, 요청 진행 중 화면도 실제 프롬프트를
//! 띄우지 않고서는 볼 수 없다.
//!
//! [ADR-0052]: ../../../../docs/adr/0052-permission-prompts-are-raised-on-request-not-at-boot.md

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 디자인의 설정 콘텐츠 컬럼 폭에 맞춘 프레임 크기다. settings_remote_transfer와 같은
/// 값을 쓴다.
const WIDTH: LogicalPx = LogicalPx(560.0);

/// 각 행에 표시하는 상태. 본체의 `fda_status_text`, `status_text`, 파일 폴더 행에
/// 대응한다.
#[derive(Clone, Copy)]
enum Status {
    /// 허용됨. accent-success 색을 쓴다.
    Granted,
    /// 허용 안 됨. 확실히 허용되지 않은 상태이며 muted 색을 쓴다.
    Missing,
    /// 확인 불가. Full Disk Access를 추정할 근거가 없을 때만 나오며 muted 색을 쓴다.
    Unknown,
    /// 조회 수단 없음. 파일 폴더에만 쓴다. 상태를 물어보는 행위 자체가 프롬프트라서 잴
    /// 방법이 없다. 비워 두면 허용된 것으로 읽히므로 이 문구를 적는다.
    NotObservable,
}

impl Status {
    fn text(self, theme: &Theme) -> egui::RichText {
        let (label, color) = match self {
            Status::Granted => ("Granted", theme.accent_success().to_egui()),
            Status::Missing => ("Not granted", theme.text_muted().to_egui()),
            Status::Unknown => ("Unknown", theme.text_muted().to_egui()),
            Status::NotObservable => ("Cannot be observed", theme.text_muted().to_egui()),
        };
        egui::RichText::new(label).color(color)
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "Nothing granted yet", |ui| {
            panel(ui, theme, Status::Missing, Status::Missing, false);
        });
        spec::cluster(ui, theme, "All granted", |ui| {
            panel(ui, theme, Status::Granted, Status::Granted, false);
        });
        spec::cluster(ui, theme, "Full Disk Access estimate has no basis", |ui| {
            panel(ui, theme, Status::Unknown, Status::Granted, false);
        });
        spec::cluster(ui, theme, "Requesting", |ui| {
            panel(ui, theme, Status::Missing, Status::Missing, true);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("L2 position", "last — macOS only, after Display"),
            (
                "rows",
                "Full Disk Access · Screen recording · File folders (+ Accessibility in debug)",
            ),
            ("row grid", "label · status · gap 12 / row gap 8"),
            (
                "buttons",
                "Request all (primary) · Open FDA settings (secondary)",
            ),
            (
                "while requesting",
                "primary disabled · hint line swaps to progress",
            ),
            (
                "status colors",
                "granted = accent-success · everything else = text-muted",
            ),
        ],
        &[
            TokenChip::new(
                "accent-success",
                "granted status",
                theme.accent_success().to_egui(),
            ),
            TokenChip::new(
                "text-muted",
                "other statuses + notes",
                theme.text_muted().to_egui(),
            ),
            TokenChip::new(
                "spacing-md",
                "grid column gap",
                theme.surface_active().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "This is the only place a permission request starts — Tasty raises no prompts at \
         boot (ADR-0569). Full Disk Access is absent from the request button because no app \
         can ask for it; the secondary button sends the user to System Settings instead. The \
         file folder row can never show a real state: asking macOS for it is the prompt.",
    );
}

/// 화면 한 벌을 그린다. 본체와 같은 순서로 상태 grid, 주석, 버튼 행, 설명 줄을 쌓는다.
fn panel(ui: &mut egui::Ui, theme: &Theme, fda: Status, screen: Status, requesting: bool) {
    kit::frame_card_flat(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
        kit::region_sym(ui, theme.spacing_lg, theme.spacing_md, |ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();

            egui::Grid::new(ui.next_auto_id())
                .num_columns(2)
                .spacing([theme.spacing_md.value(), theme.spacing_sm.value()])
                .show(ui, |ui| {
                    for (label, status) in [
                        ("Full Disk Access", fda),
                        ("Screen recording", screen),
                        (
                            "File folders (Downloads · Documents · Desktop · volumes)",
                            Status::NotObservable,
                        ),
                    ] {
                        ui.label(label);
                        ui.label(status.text(theme));
                        ui.end_row();
                    }
                });

            muted(
                ui,
                theme,
                "macOS offers no API to ask whether an app has Full Disk Access, so this state \
                 is inferred and can be wrong.",
            );

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                // specimen이라 클릭에 반응할 필요가 없다. 실제 요청은 본체가 한다.
                let _ = Button::new("Request all permissions")
                    .variant(ButtonVariant::Primary)
                    .size(ControlSize::Md)
                    .enabled(!requesting)
                    .show(ui, theme);
                // specimen이라 시스템 설정을 열지 않는다. 그 동작은 본체가 한다.
                let _ = Button::new("Open Full Disk Access settings")
                    .variant(ButtonVariant::Secondary)
                    .size(ControlSize::Md)
                    .show(ui, theme);
            });

            muted(
                ui,
                theme,
                if requesting {
                    "Requesting permissions. Answer each prompt as it appears — the next one \
                     only shows once you answer the previous."
                } else {
                    "This requests the file folder and screen recording permissions one at a \
                     time. Items you already allowed or denied do not prompt again."
                },
            );
        });
    });
}

/// caption 크기에 text-muted 색을 쓰는 설명 줄. settings_remote_transfer의 `row_desc`와
/// 같은 자리다.
fn muted(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}
