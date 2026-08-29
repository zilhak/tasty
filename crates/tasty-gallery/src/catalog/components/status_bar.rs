//! `statusbar` specimen — 작업 컬럼 하단 StatusBar (디자인 `ui_kits/terminal/work.jsx`
//! `StatusBar`).
//!
//! **복제가 아니다** — `tasty_ui_widgets::draw_status_bar_view` 를 그대로 호출한다.
//! 본체 `src/adapters/ui/status_bar.rs` 의 wrapper 가 부르는 것과 **같은 함수**라
//! 레이아웃·색·치수를 이 파일이 재선언하는 곳이 없다(본체와 시각이 자동 동기화).
//! 여기서 주는 것은 표시 데이터(`StatusBarData`)뿐이다 — i18n 문자열도 본체 wrapper 가
//! 주입하는 자리라, specimen 은 영문 리터럴을 넣는다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{StatusBarData, draw_status_bar_view};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};

/// specimen 공통 기본값 — 변형마다 필요한 필드만 덮어쓴다.
fn base() -> StatusBarData {
    StatusBarData {
        branch: Some("main".into()),
        surface_id: Some(3),
        shell: Some("zsh".into()),
        grid: Some((120, 32)),
        theme_id: "mocha".into(),
        theme_is_light: false,
        palette_label: "Ctrl+K palette".into(),
        palette_tooltip: "Open the command palette".into(),
        theme_tooltip: "Toggle light / dark theme".into(),
    }
}

/// 한 변형을 갤러리 폭에 맞춰 그린다. 본체에서는 작업 컬럼 폭이 들어오는 자리.
fn bar(ui: &mut egui::Ui, theme: &Theme, data: &StatusBarData) {
    let w = LogicalPx(ui.available_width().min(theme.measure_xl.value()));
    draw_status_bar_view(ui, theme, w, data);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Tight, |ui| {
        cluster(ui, theme, "in a repo — terminal surface", |ui| {
            bar(ui, theme, &base());
        });
        cluster(ui, theme, "outside a repo — branch cluster hidden", |ui| {
            bar(
                ui,
                theme,
                &StatusBarData {
                    branch: None,
                    ..base()
                },
            );
        });
        cluster(ui, theme, "non-terminal surface — no shell / grid", |ui| {
            bar(
                ui,
                theme,
                &StatusBarData {
                    shell: None,
                    grid: None,
                    ..base()
                },
            );
        });
        cluster(
            ui,
            theme,
            "no palette binding — word only, light theme",
            |ui| {
                bar(
                    ui,
                    theme,
                    &StatusBarData {
                        theme_id: "latte".into(),
                        theme_is_light: true,
                        palette_label: "palette".into(),
                        ..base()
                    },
                );
            },
        );
    });

    note(
        ui,
        theme,
        "본체는 이 view 를 `egui::Area`(Order::Foreground) 안에서 호출한다 — Area 와 \
         z-order 는 본체 정책이라 view 가 소유하지 않는다. 라벨/tooltip 도 본체 wrapper 가 \
         i18n 에서 주입한다(위젯 crate 는 i18n 비의존).",
    );

    meta(
        ui,
        theme,
        &[
            ("height", "status-bar-height (24)"),
            ("cell padding", "0 10px · gap 6"),
            ("dot", "7×7"),
            ("border-top", "border-width separator"),
            ("font", "mono font-size-caption"),
        ],
        &[
            TokenChip::new("bg-app", "bar", egui::Color32::from(theme.bg_app())),
            TokenChip::new(
                "separator",
                "border-top",
                egui::Color32::from(theme.separator),
            ),
            TokenChip::new(
                "accent-success",
                "branch dot",
                egui::Color32::from(theme.accent_success()),
            ),
            TokenChip::new(
                "text-muted",
                "cell text",
                egui::Color32::from(theme.text_muted()),
            ),
            TokenChip::new(
                "accent-agent",
                "dark theme dot",
                egui::Color32::from(theme.accent_agent()),
            ),
        ],
    );
}
