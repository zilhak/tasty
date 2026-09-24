//! 설치된 플러그인의 목록·상세·제거 확인 예제.
//! 본체 상세는 스크롤하지만 갤러리는 전체 내용을 비교할 수 있게 예제 높이를 늘린다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::tokens::{PLUGIN_LIST_ROW_HEIGHT, STRUCT_GAP_2};
use tasty_ui_widgets::{
    Button, ButtonVariant, PluginAvatarSize, TagVariant, checkbox, margin_sym, paint_plugin_avatar,
    plugin_avatar, tag,
};

/// 상세 컬럼이 그릴 것 — 본체는 선택 상태와 uninstall 확인 상태로 갈린다.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Detail {
    /// `ui_state.selected_id` 가 비었을 때. 본체 `plugins.none_selected`.
    None,
    /// 선택된 행의 전체 메타.
    Selected(usize),
    /// `Uninstall` 을 누른 뒤 — 경고 문구 + 확인/취소 두 버튼.
    ConfirmUninstall(usize),
}

impl Detail {
    /// 목록에서 강조할 행. 빈 상태에는 강조가 없다.
    pub(super) fn selected_row(self) -> Option<usize> {
        match self {
            Detail::None => None,
            Detail::Selected(i) | Detail::ConfirmUninstall(i) => Some(i),
        }
    }
}

/// 목록 행 하나의 표시 데이터 — 본체 `PluginsSnapshot.plugins` 항목과 동형.
pub(super) struct Row {
    pub(super) name: &'static str,
    version: &'static str,
    builtin: bool,
    enabled: bool,
    running: bool,
    health_error: bool,
    id: &'static str,
    description: &'static str,
    authors: &'static str,
    homepage: &'static str,
    surface_kinds: &'static [&'static str],
    permissions: &'static [&'static str],
    commands: &'static [(&'static str, &'static str)],
    install_dir: &'static str,
    log_path: &'static str,
}

pub(super) const ROWS: &[Row] = &[
    Row {
        name: "Clipboard viewer",
        version: "0.4.2",
        builtin: true,
        enabled: true,
        running: true,
        health_error: false,
        id: "com.tasty.clipboard-viewer",
        description: "Shows the clipboard history in a popup, grouped by content type.",
        authors: "tasty",
        homepage: "https://github.com/zilhak/tasty",
        surface_kinds: &["clipboard-viewer"],
        permissions: &["clipboard", "surface:read"],
        commands: &[("Open clipboard viewer", "Ctrl+Shift+V")],
        install_dir: "~/.tasty/plugins/com.tasty.clipboard-viewer",
        log_path: "~/.tasty/logs/com.tasty.clipboard-viewer.log",
    },
    Row {
        name: "Git viewer",
        version: "0.3.1",
        builtin: true,
        enabled: true,
        running: false,
        health_error: true,
        id: "com.tasty.git-viewer",
        description: "Shows the working tree and staged diff for the surface's directory.",
        authors: "tasty",
        homepage: "",
        surface_kinds: &[],
        permissions: &["fs:read", "process:read"],
        commands: &[],
        install_dir: "~/.tasty/plugins/com.tasty.git-viewer",
        log_path: "~/.tasty/logs/com.tasty.git-viewer.log",
    },
    Row {
        name: "Markdown",
        version: "0.9.0",
        builtin: false,
        enabled: false,
        running: false,
        health_error: false,
        id: "com.tasty.markdown",
        description: "Renders markdown files in a native WebView surface.",
        authors: "tasty",
        homepage: "",
        surface_kinds: &["markdown"],
        permissions: &["fs:read"],
        commands: &[],
        install_dir: "~/.tasty/plugins/com.tasty.markdown",
        log_path: "~/.tasty/logs/com.tasty.markdown.log",
    },
];

/// 목록 행의 높이는 아바타와 위아래 패딩으로 정한다.
pub(super) fn list_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, detail: Detail) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());

    let row_h = PLUGIN_LIST_ROW_HEIGHT.value();
    let pad = egui::vec2(theme.spacing_sm.value(), theme.spacing_sm.value());
    let avatar = PluginAvatarSize::Row.side().value();
    let mut y = rect.min.y + theme.spacing_sm.value();

    for (i, row) in ROWS.iter().enumerate() {
        let r =
            egui::Rect::from_min_size(egui::pos2(rect.min.x, y), egui::vec2(rect.width(), row_h));
        if detail.selected_row() == Some(i) {
            p.rect(
                r,
                theme.corner_radius.value(),
                theme.surface_active().to_egui(),
                egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
                egui::StrokeKind::Inside,
            );
        }

        let name = if row.builtin {
            format!("{}  •", row.name)
        } else {
            row.name.to_string()
        };
        paint_plugin_avatar(
            &p,
            theme,
            egui::pos2(r.min.x + pad.x + avatar * 0.5, r.center().y),
            row.name,
            PluginAvatarSize::Row,
        );

        let name_pos = r.min + pad + egui::vec2(avatar + theme.spacing_sm.value(), 0.0);
        p.text(
            name_pos,
            egui::Align2::LEFT_TOP,
            &name,
            egui::FontId::proportional(theme.font_size_body.value()),
            theme.text_primary().to_egui(),
        );

        let mut sub = format!("v{}", row.version);
        if !row.enabled {
            sub.push_str("  ·  Disabled");
        } else if row.running {
            sub.push_str("  ·  Running");
        }
        p.text(
            name_pos + egui::vec2(0.0, theme.spacing_lg.value() + STRUCT_GAP_2.value()),
            egui::Align2::LEFT_TOP,
            &sub,
            egui::FontId::proportional(theme.font_size_micro.value()),
            theme.text_muted().to_egui(),
        );

        if row.health_error && row.enabled {
            p.circle_filled(
                egui::pos2(r.max.x - theme.spacing_md.value(), r.center().y),
                theme.status_dot_size.value() * 0.5,
                theme.accent_danger().to_egui(),
            );
        }
        y += row_h + STRUCT_GAP_2.value();
    }
}

fn caption(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_body.value())
            .color(theme.text_primary().to_egui()),
    );
}

fn muted(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// 활성 플러그인의 실행 오류만 강조한다. 사용자가 비활성화한 상태는 오류로 표시하지 않는다.
fn health_box(ui: &mut egui::Ui, theme: &Theme) {
    let danger = theme.accent_danger().to_egui();
    egui::Frame::new()
        .fill(danger.gamma_multiply(theme.tint_fill_alpha()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            danger.gamma_multiply(theme.tint_border_alpha()),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(margin_sym(theme.spacing_md, theme.spacing_sm))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(
                    "Failed to connect. Check the plugin's configuration in Settings.",
                )
                .size(theme.font_size_body.value())
                .color(danger),
            );
        });
}

/// `Status:` 행 — 체크박스 + `Configure`. 본체는 egui `ui.checkbox` 를 직접 쓰지만
/// 갤러리는 공용 위젯(`docs/architecture/ui-widgets-crate.md` 의 "무엇을 공용 위젯으로")을 부른다.
fn status_row(ui: &mut egui::Ui, theme: &Theme, row: &Row) {
    ui.horizontal(|ui| {
        caption(ui, theme, "Status:");
        let mut enabled = row.enabled;
        checkbox(ui, theme, &mut enabled, "Enabled", true);
        Button::new("Configure")
            .variant(ButtonVariant::Secondary)
            .show(ui, theme);
    });
}

/// 목록 값 한 묶음 — 비면 본체처럼 `(none)` 을 그린다.
fn list_or_none(ui: &mut egui::Ui, theme: &Theme, label: &str, values: &[&str], as_tags: bool) {
    caption(ui, theme, label);
    if values.is_empty() {
        caption(ui, theme, "(none)");
    } else if as_tags {
        ui.horizontal_wrapped(|ui| {
            for v in values {
                tag(ui, theme, v, TagVariant::Default, false);
            }
        });
    } else {
        caption(ui, theme, &values.join(", "));
    }
}

/// `Commands:` — 제목 좌, 단축키 tag 우. 명령이 없으면 절 자체가 안 나온다.
fn commands(ui: &mut egui::Ui, theme: &Theme, row: &Row) {
    if row.commands.is_empty() {
        return;
    }
    ui.separator();
    caption(ui, theme, "Commands:");
    for (title, kb) in row.commands {
        ui.horizontal(|ui| {
            caption(ui, theme, title);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                tag(ui, theme, kb, TagVariant::Default, false);
            });
        });
    }
}

/// 설치 경로 + 로그 경로. 경로는 길어서 본체도 muted small 로 흘린다.
fn paths(ui: &mut egui::Ui, theme: &Theme, row: &Row) {
    ui.separator();
    caption(ui, theme, "Install path:");
    ui.horizontal(|ui| {
        muted(ui, theme, row.install_dir);
        Button::new("Open folder")
            .variant(ButtonVariant::Secondary)
            .show(ui, theme);
    });
    muted(ui, theme, &format!("Log: {}", row.log_path));
}

/// 본체와 같은 일반 버튼으로 제거 동작과 확인·취소를 표시한다.
fn uninstall(ui: &mut egui::Ui, theme: &Theme, row: &Row, confirming: bool) {
    if !confirming {
        Button::new("Uninstall")
            .variant(ButtonVariant::Secondary)
            .show(ui, theme);
        return;
    }
    let warning = if row.builtin {
        "This is a built-in plugin. Once removed, it will not be auto-reinstalled on next launch."
    } else {
        "All files of this plugin will be deleted."
    };
    ui.label(
        egui::RichText::new(warning)
            .size(theme.font_size_body.value())
            .color(theme.accent_attention().to_egui()),
    );
    ui.horizontal(|ui| {
        Button::new("Confirm uninstall")
            .variant(ButtonVariant::Secondary)
            .show(ui, theme);
        Button::new("Cancel")
            .variant(ButtonVariant::Secondary)
            .show(ui, theme);
    });
}

/// 우측 상세 — 본체 `CentralPanel` 블록 전량.
pub(super) fn detail_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, detail: Detail) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let inner = rect.shrink(theme.spacing_md.value());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.spacing_mut().item_spacing.y = theme.spacing_sm.value();

    let Some(i) = detail.selected_row() else {
        child.add_space(theme.spacing_xl.value());
        caption(&mut child, theme, "Select a plugin from the left.");
        return;
    };
    let row = &ROWS[i];

    child.horizontal_top(|ui| {
        plugin_avatar(ui, theme, row.name, PluginAvatarSize::Detail);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(row.name)
                        .size(theme.font_size_max.value())
                        .strong()
                        .color(theme.text_primary().to_egui()),
                );
                tag(
                    ui,
                    theme,
                    &format!("v{}", row.version),
                    TagVariant::Default,
                    false,
                );
                if row.builtin {
                    ui.label(
                        egui::RichText::new("built-in")
                            .size(theme.font_size_caption.value())
                            .color(theme.accent_agent().to_egui()),
                    );
                }
            });
            muted(ui, theme, row.id);
        });
    });
    caption(&mut child, theme, row.description);

    if row.health_error && row.enabled {
        health_box(&mut child, theme);
    }

    caption(&mut child, theme, &format!("Authors: {}", row.authors));
    if !row.homepage.is_empty() {
        caption(&mut child, theme, &format!("Homepage: {}", row.homepage));
    }

    child.separator();
    status_row(&mut child, theme, row);
    list_or_none(
        &mut child,
        theme,
        "Surface kinds:",
        row.surface_kinds,
        false,
    );

    child.separator();
    list_or_none(&mut child, theme, "Permissions:", row.permissions, true);

    commands(&mut child, theme, row);
    paths(&mut child, theme, row);

    child.add_space(theme.spacing_sm.value());
    uninstall(
        &mut child,
        theme,
        row,
        matches!(detail, Detail::ConfirmUninstall(_)),
    );
}
