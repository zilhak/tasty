//! `statusbar` specimen — 작업 컬럼 하단 StatusBar (디자인 `gallery/layouts.jsx`
//! 의 **Workspace status bar** 섹션).
//!
//! **복제가 아니다** — `tasty_ui_widgets::draw_status_bar_view` 를 그대로 호출한다.
//! 본체 `src/adapters/ui/status_bar.rs` 의 wrapper 가 부르는 것과 **같은 함수**라
//! 레이아웃·색·치수를 이 파일이 재선언하는 곳이 없다(본체와 시각이 자동 동기화).
//! 여기서 주는 것은 표시 데이터(`StatusBarData`)와 **바 폭**뿐이다 — i18n 문자열도
//! 본체 wrapper 가 주입하는 자리라, specimen 은 영문 리터럴을 넣는다.
//!
//! 축소 변종은 단계를 **지정하지 않는다.** 폭만 넘기고 어느 항목이 접히는지는 view 가
//! 정한다 — 단계를 여기서 박으면 specimen 이 본체와 다른 답을 들 수 있고, 그것이 이
//! 파일이 피하려는 바로 그 상태다.
//!
//! 그래서 **폭은 디자인 jsx 의 수를 그대로 못 쓴다.** 디자인 specimen 은 각 행에
//! `drop` 단계를 손으로 박고 폭은 그림용으로 골랐지만, 여기서는 폭이 곧 입력이고
//! 단계는 결과다. 같은 내용을 재는 글꼴이 달라(브라우저 11px ↔ egui caption) 디자인의
//! 470·340·210 은 이 view 에서 전부 level 0~4 안쪽에 들어와, 그대로 쓰면 행 라벨이
//! 약속한 단계가 **화면에 안 나온다.** 아래 폭은 그 단계를 실제로 띄우는 값이고,
//! 디자인 값을 고친 것이 아니라 **다른 축의 값**이다(단계가 정본이고 폭은 그 단계를
//! 부르는 수단이다). 디자인이 그대로 쓸 수 있는 것은 넉넉한 쪽 둘(720·470)뿐이다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{StatusBarData, draw_status_bar_view};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, note, stage};

/// specimen 공통 기본값 — 변형마다 필요한 필드만 덮어쓴다.
fn base() -> StatusBarData {
    StatusBarData {
        branch: Some("feat/dag-detail".into()),
        surface_id: Some(3),
        pane_id: Some(1),
        shell: Some("zsh".into()),
        grid: Some((120, 32)),
        theme_is_light: false,
        palette_keys: "Ctrl+K".into(),
        palette_tooltip: "Open the command palette".into(),
        theme_tooltip: "Toggle light / dark theme".into(),
    }
}

/// 각 변종의 바 폭. 본체에서는 작업 컬럼 폭이 들어오는 자리다.
/// 앞의 둘은 디자인 jsx 의 수 그대로이고, 뒤의 넷은 **단계를 띄우는 폭**이다(모듈 doc).
const WIDE: LogicalPx = LogicalPx(720.0);
const MID: LogicalPx = LogicalPx(470.0);
const DROPS_SHELL: LogicalPx = LogicalPx(330.0);
const DROPS_SURFACE_ID: LogicalPx = LogicalPx(270.0);
const DROPS_PALETTE: LogicalPx = LogicalPx(210.0);
const FLOOR: LogicalPx = LogicalPx(60.0);

/// 말줄임을 띄우려면 [`BRANCH_MAX_W`](tasty_ui_widgets) 를 넘는 이름이 필요하다 —
/// 디자인 specimen 의 긴 브랜치 그대로다.
const LONG_BRANCH: &str = "feat/dag-detail-and-runner-badge";

/// 긴 브랜치를 단 기본값 — 축소 사다리를 한 내용으로 이어 보이려고 세 행이 공유한다.
fn long_branch() -> StatusBarData {
    StatusBarData {
        branch: Some(LONG_BRANCH.into()),
        ..base()
    }
}

/// 한 변형을 주어진 폭으로 그린다. 카드가 더 좁으면 카드 폭이 이긴다.
fn bar(ui: &mut egui::Ui, theme: &Theme, w: LogicalPx, data: &StatusBarData) {
    let w = LogicalPx(ui.available_width().min(w.value()));
    draw_status_bar_view(ui, theme, w, data);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Tight, |ui| {
        cluster(ui, theme, "wide — everything", |ui| {
            bar(ui, theme, WIDE, &base());
        });
        cluster(ui, theme, "narrower — grid then shell drop", |ui| {
            bar(ui, theme, DROPS_SHELL, &long_branch());
        });
        cluster(
            ui,
            theme,
            "narrow — surface id drops, branch truncates",
            |ui| {
                bar(ui, theme, DROPS_SURFACE_ID, &long_branch());
            },
        );
        cluster(ui, theme, "narrower still — the palette cap drops", |ui| {
            bar(ui, theme, DROPS_PALETTE, &long_branch());
        });
        cluster(ui, theme, "floor — branch glyph + theme glyph", |ui| {
            bar(ui, theme, FLOOR, &base());
        });
        cluster(ui, theme, "not a repo — the branch item is absent", |ui| {
            bar(
                ui,
                theme,
                MID,
                &StatusBarData {
                    branch: None,
                    ..base()
                },
            );
        });
        cluster(
            ui,
            theme,
            "detached HEAD — short sha in the branch slot",
            |ui| {
                bar(
                    ui,
                    theme,
                    MID,
                    &StatusBarData {
                        branch: Some("@ 4f9c1ab".into()),
                        ..base()
                    },
                );
            },
        );
        cluster(
            ui,
            theme,
            "non-terminal surface / no palette binding — both items absent",
            |ui| {
                bar(
                    ui,
                    theme,
                    MID,
                    &StatusBarData {
                        shell: None,
                        grid: None,
                        palette_keys: String::new(),
                        theme_is_light: true,
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
         z-order 는 본체 정책이라 view 가 소유하지 않는다. tooltip 도 본체 wrapper 가 \
         i18n 에서 주입한다(위젯 crate 는 i18n 비의존). 바는 포커스 surface 의 읽기 전용 \
         요약이라 어느 항목도 포커스를 옮기지 않는다.",
    );

    meta(
        ui,
        theme,
        &[
            ("height", "status-bar-height (24) — UI 배율 밖"),
            ("layout", "padding 0 10 · item gap 10"),
            ("left", "branch · surface id · shell · grid"),
            ("right", "palette keycap · theme glyph"),
            ("mono", "surface id · grid 만"),
            (
                "drop order",
                "1 grid → 2 shell → 3 surface id → 4 palette cap → 5 branch text",
            ),
            ("never drops", "theme glyph"),
            ("no value", "항목이 자리째 없다 — dash 를 그리지 않는다"),
            (
                "glyphs",
                "git-branch / sun / theme · statusbar-glyph-size → icon-size-xs (12)",
            ),
            ("border-top", "border-width separator"),
        ],
        &[
            TokenChip::new("bg-app", "bar", egui::Color32::from(theme.bg_app())),
            TokenChip::new(
                "separator",
                "border-top",
                egui::Color32::from(theme.separator),
            ),
            TokenChip::new(
                "statusbar-glyph",
                "branch glyph",
                egui::Color32::from(theme.statusbar_glyph()),
            ),
            TokenChip::new(
                "text-muted",
                "cell text",
                egui::Color32::from(theme.text_muted()),
            ),
            TokenChip::new(
                "statusbar-theme-glyph",
                "theme glyph",
                egui::Color32::from(theme.statusbar_theme_glyph()),
            ),
        ],
    );
}
