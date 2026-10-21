//! Markdown 주소 띠 예제. 본체는 플러그인이 WebView HTML로 그리고, 여기서는 같은 토큰으로 egui 근사를 그린다.
//! 시안 `gallery/plugins-md-address.jsx`의 세 Spec(띠 상태 · 폴더 버튼 · 좁은 폭)을 옮긴다.
//!
//! 띠: [Folder] (원격은 [Folder · Reload]) · 필드(kind 글리프 + mono 경로) · [Go]. 모든 버튼은 IconButton
//! ghost sm 이다. 필드가 `md-address-go-hide-below` 보다 좁아지면 Go 를 먼저 숨기고 Enter 는 그대로다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    ControlSize, IconButton, IconButtonState, Input, autocomplete_dropdown, paint_icon_button_state,
};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 띠 상태. 시안 `MdAddressBar` 의 `state` 값과 같다.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BarState {
    Idle,
    Editing,
    Stale,
    Disconnected,
    Error,
}

/// 플러그인 문구(`crates/tasty-plugin-markdown/lang/*.toml` 의 `markdown.address.*`). 갤러리는 본체
/// 번역표만 읽으므로 시안 문구를 그대로 둔다.
struct Copy {
    open: &'static str,
    open_remote: &'static str,
    go: &'static str,
    reload: &'static str,
    stale: &'static str,
    disconnected: &'static str,
    /// `{path}` 자리에 경로가 들어간다.
    not_found: &'static str,
}

const EN: Copy = Copy {
    open: "Open a file…",
    open_remote: "Open a remote file…",
    go: "Go",
    reload: "Reload",
    stale: "The remote file changed — reload",
    disconnected: "Disconnected from the remote",
    not_found: "Can't open {path} on the remote: file not found.",
};
const KO: Copy = Copy {
    open: "파일 열기…",
    open_remote: "원격 파일 열기…",
    go: "이동",
    reload: "새로 고침",
    stale: "원격 파일이 바뀌었습니다 — 새로 고침",
    disconnected: "원격 연결이 끊겼습니다",
    not_found: "원격에서 {path} 파일을 열 수 없습니다: 파일이 없습니다.",
};
const JA: Copy = Copy {
    open: "ファイルを開く…",
    open_remote: "リモートのファイルを開く…",
    go: "移動",
    reload: "再読み込み",
    stale: "リモートのファイルが変更されました — 再読み込み",
    disconnected: "リモートから切断されました",
    not_found: "リモートで {path} を開けません: ファイルが見つかりません。",
};

const LOCAL_PATH: &str = "~/work/tasty/docs/architecture.md";
const REMOTE_PATH: &str = "/home/deploy/agents-prod/RUNBOOK.md";
const LONG_PATH: &str =
    "~/work/tasty/docs/design/systems/very-long-folder-name-for-width-checks/theme-and-tokens.md";
const RECENT: &[&str] = &[
    "~/work/tasty/README.md",
    "~/work/tasty/docs/architecture.md",
    "~/work/tasty/docs/design/systems/theme.md",
    "~/work/tasty/CHANGELOG.md",
];

/// 상태 견본 한 칸의 폭(시안 `MaPair` 칸의 기준 폭 size-440). 두 테마는 세로로 쌓는다.
const CASE_W: LogicalPx = LogicalPx(440.0);
/// 최근 목록 견본의 목록 높이 상한(시안 `maxDropdownHeight` 160).
const RECENT_LIST_H: LogicalPx = LogicalPx(160.0);

/// 한 띠의 내용.
pub(crate) struct Bar<'a> {
    pub(crate) remote: bool,
    pub(crate) state: BarState,
    pub(crate) path: &'a str,
    /// 좁은 폭에서 Go 를 숨긴 모양.
    pub(crate) no_go: bool,
}

fn copy_for(lang: &str) -> &'static Copy {
    match lang {
        "ko" => &KO,
        "ja" => &JA,
        _ => &EN,
    }
}

/// 주소 띠 하나를 `width` 폭으로 그린다. 시안의 고정 위치값은 모두 `md-address-*` 토큰에서 읽는다.
pub(crate) fn bar(ui: &mut egui::Ui, theme: &Theme, width: f32, lang: &str, bar: &Bar<'_>) {
    let copy = copy_for(lang);
    let off = bar.state == BarState::Disconnected;
    let pad = theme.md_address_bar_padding_x().value();
    let gap = theme.md_address_bar_gap().value();
    let nav_gap = theme.md_address_bar_nav_gap().value();
    let btn = theme.icon_button_size_sm().value();
    let left = if bar.remote { btn * 2.0 + nav_gap } else { btn };
    let go = if bar.no_go { 0.0 } else { gap + btn };
    let field_w =
        (width - pad * 2.0 - left - gap - go).max(theme.md_address_field_min_width().value());
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::Frame::new()
            .fill(theme.md_address_bar_bg().to_egui())
            .inner_margin(egui::Margin::symmetric(pad as i8, 0))
            .show(ui, |ui| {
                let size = egui::vec2(width - pad * 2.0, theme.md_address_bar_height().value());
                ui.allocate_ui_with_layout(
                    size,
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_size(size);
                        ui.spacing_mut().item_spacing.x = nav_gap;
                        let open = if bar.remote {
                            copy.open_remote
                        } else {
                            copy.open
                        };
                        icon_button(ui, theme, &icons::FOLDER_OPEN, !off).on_hover_text(if off {
                            copy.disconnected
                        } else {
                            open
                        });
                        if bar.remote {
                            if bar.state == BarState::Stale {
                                stale_reload(ui, theme).on_hover_text(copy.stale);
                            } else {
                                icon_button(ui, theme, &icons::REFRESH, !off).on_hover_text(
                                    if off { copy.disconnected } else { copy.reload },
                                );
                            }
                        }
                        ui.add_space(gap - nav_gap);
                        field(ui, theme, field_w, bar);
                        if !bar.no_go {
                            ui.add_space(gap - nav_gap);
                            icon_button(ui, theme, &icons::ARROW_RIGHT, !off)
                                .on_hover_text(if off { copy.disconnected } else { copy.go });
                        }
                    },
                );
            });
        if bar.state == BarState::Error {
            error_row(
                ui,
                theme,
                width,
                &copy.not_found.replace("{path}", bar.path),
            );
        }
        let (edge, _) = ui.allocate_exact_size(
            egui::vec2(width, theme.border_width.value()),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(
            edge,
            0.0,
            theme.md_address_bar_border().to_egui_premultiplied(),
        );
    });
}

fn icon_button(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: &icons::Icon,
    enabled: bool,
) -> egui::Response {
    IconButton::new()
        .size(ControlSize::Sm)
        .enabled(enabled)
        .show(ui, theme, &|ui, rect, c| {
            icon.image(rect.height(), c).paint_at(ui, rect)
        })
}

/// stale Reload — IconButton sm 과 같은 상자를 `md-address-stale-bg` 로 채운다. 띠에서 유일하게 채운 요소다.
fn stale_reload(ui: &mut egui::Ui, theme: &Theme) -> egui::Response {
    let side = theme.icon_button_size_sm().value();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
    ui.painter().rect_filled(
        rect,
        theme.icon_button_radius().value(),
        theme.md_address_stale_bg().to_egui(),
    );
    let glyph = theme.icon_glyph_size_md.value();
    let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
    icons::REFRESH
        .image(glyph, theme.md_address_stale_fg().to_egui())
        .paint_at(ui, inner);
    resp
}

/// 필드 — kind 글리프(file | remote) + mono 경로. editing 은 focus 테두리, error 는 invalid 테두리,
/// disconnected 는 읽기 전용 상자에 `state-disabled-fg` 경로다.
fn field(ui: &mut egui::Ui, theme: &Theme, width: f32, bar: &Bar<'_>) {
    let mut buf = bar.path.to_string();
    let glyph = if bar.remote {
        &icons::REMOTE
    } else {
        &icons::FILE
    };
    let paint = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
        glyph.image(rect.height(), c).paint_at(ui, rect);
    };
    let off = bar.state == BarState::Disconnected;
    let editing = matches!(bar.state, BarState::Editing | BarState::Error);
    let mut input = Input::new()
        .id(ui.id().with(("md_address_field", bar.path, bar.remote)))
        .mono(true)
        .width(width)
        .icon(&paint)
        .invalid(bar.state == BarState::Error)
        .read_only(off)
        .focus_look(editing);
    input = if off {
        input.text_color(theme.state_disabled_fg().to_egui())
    } else if editing {
        input
    } else {
        input.text_color(theme.text_secondary().to_egui())
    };
    input.show(ui, theme, &mut buf);
}

/// 이동 실패 행 — 띠 아래, 높이 `md-address-error-height`, alertCircle(`md-address-error-glyph`) + caption.
fn error_row(ui: &mut egui::Ui, theme: &Theme, width: f32, text: &str) {
    let pad = theme.md_address_bar_padding_x().value();
    egui::Frame::new()
        .fill(theme.md_address_bar_bg().to_egui())
        .inner_margin(egui::Margin::symmetric(pad as i8, 0))
        .show(ui, |ui| {
            let size = egui::vec2(width - pad * 2.0, theme.md_address_error_height().value());
            ui.allocate_ui_with_layout(
                size,
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_size(size);
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    let side = theme.icon_glyph_size_sm.value();
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                    icons::ALERT_CIRCLE
                        .image(side, theme.md_address_error_glyph().to_egui())
                        .paint_at(ui, rect);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(text)
                                .size(theme.font_size_caption.value())
                                .color(theme.text_primary().to_egui()),
                        )
                        .truncate(),
                    )
                    .on_hover_text(text);
                },
            );
        });
}

/// 로컬 편집 중 최근 목록이 열린 모양. 최근 목록은 로컬에만 있다.
fn local_with_recent(ui: &mut egui::Ui, theme: &Theme, width: f32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        bar(
            ui,
            theme,
            width,
            "en",
            &Bar {
                remote: false,
                state: BarState::Editing,
                path: "~/work/tasty/docs/",
                no_go: false,
            },
        );
        let pad = theme.md_address_bar_padding_x().value();
        let btn = theme.icon_button_size_sm().value();
        let gap = theme.md_address_bar_gap().value();
        let list_w = width - pad * 2.0 - btn * 2.0 - gap * 2.0;
        ui.horizontal(|ui| {
            ui.add_space(pad + btn + gap);
            ui.set_max_width(pad + btn + gap + list_w);
            let paint = |ui: &mut egui::Ui, rect: egui::Rect, c: egui::Color32| {
                icons::FILE.image(rect.height(), c).paint_at(ui, rect);
            };
            autocomplete_dropdown(
                ui,
                theme,
                "md_address_recent",
                RECENT,
                "No matching path",
                true,
                Some(&paint),
                Some(1),
                "~/work/tasty/docs/",
                true,
                RECENT_LIST_H.value(),
            );
        });
    });
}

fn case(ui: &mut egui::Ui, theme: &Theme, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        ui.label(
            egui::RichText::new(label)
                .size(theme.font_size_micro.value())
                .color(theme.text_muted().to_egui()),
        );
        ui.push_id(label, add);
    });
}

/// 띠 상태 일곱 장을 Mocha · Latte 두 테마로 그린다.
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let latte = crate::host_shell::latte_theme();
    let w = CASE_W.value();
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        for (label, th) in [("Mocha", theme), ("Latte", &latte)] {
            spec::cluster(ui, th, label, |ui| {
                ui.push_id(label, |ui| {
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = th.spacing_md.value();
                        let states = [
                            ("local · idle", false, BarState::Idle, LOCAL_PATH),
                            ("remote · idle", true, BarState::Idle, REMOTE_PATH),
                            (
                                "remote · editing",
                                true,
                                BarState::Editing,
                                "/home/deploy/agents-prod/docs/ONCALL",
                            ),
                            (
                                "remote · stale (Reload filled)",
                                true,
                                BarState::Stale,
                                REMOTE_PATH,
                            ),
                            (
                                "remote · disconnected",
                                true,
                                BarState::Disconnected,
                                REMOTE_PATH,
                            ),
                            (
                                "remote · navigate failed",
                                true,
                                BarState::Error,
                                "/home/deploy/agents-prod/RUNBOK.md",
                            ),
                        ];
                        case(ui, th, "local · editing, recent files open", |ui| {
                            local_with_recent(ui, th, w)
                        });
                        for (name, remote, state, path) in states {
                            case(ui, th, name, |ui| {
                                bar(
                                    ui,
                                    th,
                                    w,
                                    "en",
                                    &Bar {
                                        remote,
                                        state,
                                        path,
                                        no_go: false,
                                    },
                                )
                            });
                        }
                    })
                });
            });
        }
    });
    spec::meta(
        ui,
        theme,
        &[
            (
                "band",
                "md-address-bar-height 40 · md-address-bar-bg (bg-sidebar) · 1px md-address-bar-border below · padding-x 8 · group gap 8",
            ),
            (
                "left cluster",
                "Folder (local) · Folder + Reload (remote), gap md-address-bar-nav-gap 4",
            ),
            (
                "buttons",
                "IconButton ghost sm — 24 box, icon-size-md 16 glyph; disabled = state-disabled-fg, no overlay",
            ),
            (
                "field",
                "PathField box — input-height 28 · leading kind glyph (file | remote) · mono · front ellipsis · floor md-address-field-min-width 96",
            ),
            (
                "Go",
                "arrowRight, trailing, both variants; Enter is the same action",
            ),
            (
                "remote editing",
                "Enter / Go → same surface opens that remote path · Esc reverts · no recent list (local only)",
            ),
            (
                "stale",
                "Reload = filled md-address-stale-bg (accent-primary) + md-address-stale-fg (text-on-accent)",
            ),
            (
                "disconnected",
                "field = input-readonly box, path in state-disabled-fg · Folder · Reload · Go disabled · tooltip “Disconnected from the remote”",
            ),
            (
                "navigate failed",
                "typed path kept, invalid edge + ring · error row md-address-error-height 24, alertCircle in md-address-error-glyph · clears on next keystroke, Esc, or a successful open",
            ),
        ],
        &[
            TokenChip::new(
                "md-address-bar-bg",
                "→ html-address-bar-bg",
                theme.md_address_bar_bg().to_egui(),
            ),
            TokenChip::new(
                "md-address-stale-bg",
                "→ accent-primary",
                theme.md_address_stale_bg().to_egui(),
            ),
            TokenChip::new(
                "md-address-stale-fg",
                "→ text-on-accent",
                theme.md_address_stale_fg().to_egui(),
            ),
            TokenChip::new(
                "md-address-error-glyph",
                "→ html-address-error-glyph",
                theme.md_address_error_glyph().to_egui(),
            ),
            TokenChip::new(
                "input-readonly-bg",
                "disconnected field",
                theme.input_readonly_bg().to_egui(),
            ),
            TokenChip::new(
                "input-border-invalid",
                "failed navigate edge",
                theme.input_border_invalid().to_egui(),
            ),
        ],
    );
    spec::note(
        ui,
        theme,
        "Markdown mirror vs HTML mirror: same band box, same left-cluster Reload, same remote glyph, same error row. HTML mirror keeps a read-only field with no Go; the Markdown mirror field is editable with Go.",
    );
}

/// 폴더 버튼 상태 다섯 개와 세 언어 tooltip 문구.
pub fn draw_folder_button(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "states", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
                for (name, hovered, pressed, enabled) in [
                    ("normal", false, false, true),
                    ("hover", true, false, true),
                    ("active", false, true, true),
                    ("disabled", false, false, false),
                ] {
                    ui.vertical(|ui| {
                        pinned_button(ui, theme, hovered, pressed, enabled);
                        ui.label(
                            egui::RichText::new(name)
                                .size(theme.font_size_micro.value())
                                .color(theme.text_muted().to_egui()),
                        );
                    });
                }
            });
        });
        spec::cluster(ui, theme, "tooltips (en · ko · ja)", |ui| {
            ui.vertical(|ui| {
                for c in [&EN, &KO, &JA] {
                    ui.label(format!("{} · {} · {}", c.open, c.open_remote, c.stale));
                }
            });
        });
    });
    spec::note(
        ui,
        theme,
        "The shared IconButton; only the glyph and the tooltip are per surface. Hover / active use the Theme overlays; disabled (remote disconnected) has no overlay. Tooltip = aria-label without the ellipsis.",
    );
}

/// 상태를 고정한 폴더 버튼(호버·누름 오버레이는 IconButton 과 같은 도출 규칙).
fn pinned_button(ui: &mut egui::Ui, theme: &Theme, hovered: bool, pressed: bool, enabled: bool) {
    let side = theme.icon_button_size_sm().value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
    let state = IconButtonState {
        enabled,
        ..IconButtonState::ghost(hovered, pressed)
    };
    let fg = paint_icon_button_state(ui.painter(), theme, rect, state);
    let glyph = theme.icon_glyph_size_md.value();
    let inner = egui::Rect::from_center_size(rect.center(), egui::vec2(glyph, glyph));
    icons::FOLDER_OPEN.image(glyph, fg).paint_at(ui, inner);
}

/// 좁은 폭과 긴 경로. Go 를 먼저 숨기고 필드는 96 하한까지 줄어든다.
pub fn draw_narrow(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        let cases = [
            (
                "560 · en · local, long path",
                560.0,
                "en",
                false,
                BarState::Idle,
                LONG_PATH,
                false,
            ),
            (
                "288 · ko · local, Go hidden",
                288.0,
                "ko",
                false,
                BarState::Idle,
                LONG_PATH,
                true,
            ),
            (
                "240 · ja · remote stale, Go hidden",
                240.0,
                "ja",
                true,
                BarState::Stale,
                REMOTE_PATH,
                true,
            ),
            (
                "320 · ja · remote navigate failed",
                320.0,
                "ja",
                true,
                BarState::Error,
                "/home/deploy/agents-prod/RUNBOK.md",
                true,
            ),
        ];
        for (label, w, lang, remote, state, path, no_go) in cases {
            case(ui, theme, label, |ui| {
                bar(
                    ui,
                    theme,
                    w,
                    lang,
                    &Bar {
                        remote,
                        state,
                        path,
                        no_go,
                    },
                )
            });
        }
    });
    spec::meta(
        ui,
        theme,
        &[
            ("hide order", "Go (field < 160) → nothing else hides"),
            (
                "floor",
                "field 96 = md-address-field-min-width; below that the band clips at its end",
            ),
            ("local min band", "8 + 24 + 8 + 96 + 8 = 144 without Go"),
            (
                "remote min band",
                "8 + 24 + 4 + 24 + 8 + 96 + 8 = 172 without Go",
            ),
        ],
        &[
            TokenChip::without_color("md-address-go-hide-below", "→ field-width-md 160"),
            TokenChip::without_color(
                "md-address-field-min-width",
                "→ html-address-field-min-width 96",
            ),
        ],
    );
}
