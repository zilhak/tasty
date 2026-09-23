//! Settings › General › Permissions — macOS 전용 L2 서브탭 specimen.
//!
//! 본체: `src/view/settings/ui/tabs/macos_permissions.rs::draw_macos_permissions_tab`.
//! 전사 원본이 없다 — 이 화면은 claude design 시안 없이 본체와 같은 토큰·위젯으로
//! 조립했다(권한 요청 버튼·진행 표시는 [ADR-0569] 가 만든 새 요소다). 시안이 나중에
//! 오면 이 specimen 이 그 대조 자리가 된다.
//!
//! 콘텐츠 컬럼 = 라벨/상태 2 열 grid(FDA · 화면 기록 · 파일 폴더, debug 본체는 손쉬운
//! 사용 1 행 추가) + muted 주석 + 버튼 행(primary "Request all permissions" ·
//! secondary "Open Full Disk Access settings") + muted 설명 줄. 갤러리는 본체 미의존이라
//! host `draw_macos_permissions_tab`(TCC · 플랫폼 의존)을 직접 못 부르고 같은 위젯 ·
//! 토큰으로 미러한다(settings_remote_transfer 전례).
//!
//! **상태 조합을 전부 보여주는 것이 이 specimen 의 값이다.** 본체에서는 실기의 TCC 상태
//! 하나만 보이는데, 특히 FDA "Unknown" 은 프로브 경로가 하나도 없는 macOS 에서만 나와
//! 실기 재현이 어렵다. 요청 진행 중(버튼 비활성 + 진행 문구)도 프롬프트를 띄우지 않고는
//! 못 보는 상태다.
//!
//! [ADR-0569]: ../../../../docs/adr/0569-permission-prompts-are-raised-on-request-not-at-boot.md

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// 디자인 settings 콘텐츠 컬럼 근사 프레임 폭(settings_remote_transfer 와 동일).
const WIDTH: LogicalPx = LogicalPx(560.0);

/// 한 행의 상태 칸. 본체의 `fda_status_text` / `status_text` / 파일 폴더 행에 대응한다.
#[derive(Clone, Copy)]
enum Status {
    /// 허용됨 — accent-success.
    Granted,
    /// 허용 안 됨 — muted. 판정이 확실한 미승인이다.
    Missing,
    /// 확인 불가 — muted. FDA 추정의 근거가 없을 때만 나온다.
    Unknown,
    /// 조회 수단 없음 — muted. 파일 폴더 전용이다: 물어보는 행위가 곧 프롬프트라
    /// 상태를 잴 방법이 없다. 빈칸으로 두면 승인된 것으로 읽힌다.
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

/// 한 벌의 화면. 본체 draw 와 같은 순서 — 상태 grid → 주석 → 버튼 행 → 설명 줄.
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
                // specimen — 클릭 응답 불필요, 그리기만(요청 시퀀스는 host 소유).
                let _ = Button::new("Request all permissions")
                    .variant(ButtonVariant::Primary)
                    .size(ControlSize::Md)
                    .enabled(!requesting)
                    .show(ui, theme);
                // specimen — 시스템 설정 열기는 host 소유다.
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

/// caption · text-muted 설명 줄 (settings_remote_transfer 의 `row_desc` 와 같은 자리).
fn muted(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}
