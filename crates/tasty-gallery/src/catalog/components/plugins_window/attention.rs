//! 서명·권한·실행 오류로 확인이 필요한 플러그인 예제.
//! 본체는 선택한 하나를 표시하지만 갤러리는 네 사유를 나란히 보여준다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::{PLUGIN_LIST_ROW_HEIGHT, STRUCT_GAP_2};
use tasty_ui_widgets::{
    Button, ButtonVariant, PluginAvatarSize, TagVariant, margin_all, paint_plugin_avatar,
    plugin_avatar, tag,
};

/// 본체 ATTN_PRIMITIVE_12와 같은 12px 글꼴. 대응 semantic 토큰이 없다.
const ATTN_PRIMITIVE_12: LogicalPx = LogicalPx(12.0);

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    UnknownKey,
    SignatureInvalid,
    PermissionsChanged,
    HealthError,
}

impl Kind {
    /// 본체 `AttentionKind::is_danger` — 서명 계열만 danger, 나머지는 warning.
    fn is_danger(self) -> bool {
        matches!(self, Self::UnknownKey | Self::SignatureInvalid)
    }

    fn label(self) -> &'static str {
        match self {
            Self::UnknownKey => "Signature not trusted",
            Self::SignatureInvalid => "Signature invalid",
            Self::PermissionsChanged => "Permissions changed",
            Self::HealthError => "Runtime error",
        }
    }

    fn blurb(self) -> &'static str {
        match self {
            Self::UnknownKey => {
                "Signed by a key that isn't in your trust store — registration rejected."
            }
            Self::SignatureInvalid => {
                "Signature missing or failed verification — registration rejected."
            }
            Self::PermissionsChanged => {
                "Manifest permissions changed since you trusted it — re-approval required."
            }
            Self::HealthError => "Enabled, but failing at runtime. See the log for details.",
        }
    }

    /// 액션 바 우측 버튼 — 본체 `draw_action_bar` 의 사유별 분기.
    fn action(self) -> &'static str {
        match self {
            Self::PermissionsChanged => "Re-approve",
            Self::HealthError => "Configure",
            Self::UnknownKey | Self::SignatureInvalid => "Copy fingerprint",
        }
    }

    fn status(self) -> &'static str {
        if self.is_danger() {
            "Not registered"
        } else {
            "Needs review"
        }
    }
}

pub(super) struct Entry {
    pub name: &'static str,
    pub version: &'static str,
    pub id: &'static str,
    pub builtin: bool,
    pub kind: Kind,
}

pub(super) const ENTRIES: &[Entry] = &[
    Entry {
        name: "Port scanner",
        version: "0.2.0",
        id: "com.example.port-scanner",
        builtin: false,
        kind: Kind::UnknownKey,
    },
    Entry {
        name: "Log tailer",
        version: "0.1.4",
        id: "com.example.log-tailer",
        builtin: false,
        kind: Kind::SignatureInvalid,
    },
    Entry {
        name: "Git viewer",
        version: "0.3.1",
        id: "com.tasty.git-viewer",
        builtin: true,
        kind: Kind::PermissionsChanged,
    },
    Entry {
        name: "Markdown",
        version: "0.9.0",
        id: "com.tasty.markdown",
        builtin: true,
        kind: Kind::HealthError,
    },
];

/// 상세에 펼쳐 보이는 항목 — 본체는 선택 행 하나를 그린다.
const SELECTED: usize = 2;

fn sev_color(theme: &Theme, kind: Kind) -> egui::Color32 {
    if kind.is_danger() {
        theme.accent_danger().to_egui()
    } else {
        theme.accent_warning().to_egui()
    }
}

/// 좌측 목록 — 본체 `SidePanel::left("plugins_attention_list")`. 행 높이·패딩은
/// Installed 목록과 같은 토큰 조립이고, 두 번째 줄이 사유 라벨(severity 색)이다.
pub(super) fn list_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());

    let row_h = PLUGIN_LIST_ROW_HEIGHT.value();
    let pad = egui::vec2(theme.spacing_sm.value(), theme.spacing_sm.value());
    let avatar = PluginAvatarSize::Row.side().value();
    let mut y = rect.min.y + theme.spacing_sm.value();

    for (i, entry) in ENTRIES.iter().enumerate() {
        let r =
            egui::Rect::from_min_size(egui::pos2(rect.min.x, y), egui::vec2(rect.width(), row_h));
        if i == SELECTED {
            p.rect(
                r,
                theme.corner_radius.value(),
                theme.surface_active().to_egui(),
                egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
                egui::StrokeKind::Inside,
            );
        }
        paint_plugin_avatar(
            &p,
            theme,
            egui::pos2(r.min.x + pad.x + avatar * 0.5, r.center().y),
            entry.name,
            PluginAvatarSize::Row,
        );

        let name_pos = r.min + pad + egui::vec2(avatar + theme.spacing_sm.value(), 0.0);
        p.text(
            name_pos,
            egui::Align2::LEFT_TOP,
            entry.name,
            egui::FontId::proportional(theme.font_size_body.value()),
            theme.text_primary().to_egui(),
        );
        p.text(
            name_pos + egui::vec2(0.0, theme.spacing_lg.value() + STRUCT_GAP_2.value()),
            egui::Align2::LEFT_TOP,
            entry.kind.label(),
            egui::FontId::proportional(theme.font_size_micro.value()),
            sev_color(theme, entry.kind),
        );
        p.circle_filled(
            egui::pos2(r.max.x - theme.spacing_md.value(), r.center().y),
            theme.status_dot_size.value() * 0.5,
            sev_color(theme, entry.kind),
        );
        y += row_h + STRUCT_GAP_2.value();
    }
}

/// 사유 배너 — 본체 `draw_detail` 의 severity 프레임.
fn banner(ui: &mut egui::Ui, theme: &Theme, kind: Kind) {
    let color = sev_color(theme, kind);
    egui::Frame::new()
        .fill(color.gamma_multiply(theme.tint_fill_alpha()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            color.gamma_multiply(theme.tint_border_alpha()),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(margin_all(theme.spacing_md))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(kind.label())
                    .strong()
                    .size(theme.font_size_body.value())
                    .color(color),
            );
            ui.label(
                egui::RichText::new(kind.blurb())
                    .size(ATTN_PRIMITIVE_12.value())
                    .color(theme.text_secondary().to_egui()),
            );
        });
}

/// 사유별 추가 정보 — 본체 `draw_reason_detail` 의 세 분기.
fn reason_detail(ui: &mut egui::Ui, theme: &Theme, kind: Kind) {
    let mono_header = |ui: &mut egui::Ui, text: &str| {
        ui.label(
            egui::RichText::new(text)
                .size(theme.font_size_micro.value())
                .color(theme.text_muted().to_egui()),
        );
    };
    match kind {
        Kind::PermissionsChanged => {
            mono_header(ui, "Permission changes");
            for (sign, token, note, color) in [
                (
                    "+",
                    "fs:write",
                    "newly requested",
                    theme.accent_success().to_egui(),
                ),
                (
                    "−",
                    "clipboard",
                    "no longer used",
                    theme.text_muted().to_egui(),
                ),
            ] {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(sign).monospace().strong().color(color));
                    ui.label(
                        egui::RichText::new(token)
                            .monospace()
                            .size(ATTN_PRIMITIVE_12.value()),
                    );
                    ui.label(
                        egui::RichText::new(note)
                            .size(theme.font_size_caption.value())
                            .color(theme.text_muted().to_egui()),
                    );
                });
            }
        }
        Kind::UnknownKey | Kind::SignatureInvalid => {
            mono_header(ui, "Signature");
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("fingerprint")
                        .size(theme.font_size_caption.value())
                        .color(theme.text_secondary().to_egui()),
                );
                ui.label(
                    egui::RichText::new("SHA256:9f2c…a17e")
                        .monospace()
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
            });
        }
        Kind::HealthError => {
            mono_header(ui, "Log");
            egui::Frame::new()
                .fill(theme.bg_panel().to_egui())
                .stroke(egui::Stroke::new(
                    theme.border_width.value(),
                    theme.separator.to_egui(),
                ))
                .corner_radius(theme.corner_radius.value())
                .inner_margin(margin_all(theme.spacing_sm))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("exited with status 101 (panicked at doc.rs:88)")
                            .monospace()
                            .size(ATTN_PRIMITIVE_12.value())
                            .color(theme.accent_danger().to_egui()),
                    );
                });
        }
    }
}

/// 상태 점 + 상태 텍스트 + 우측 조치 버튼 — 본체 `draw_action_bar`.
fn action_bar(ui: &mut egui::Ui, theme: &Theme, kind: Kind) {
    let color = sev_color(theme, kind);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(
            egui::Vec2::splat(theme.status_dot_size.value()),
            egui::Sense::hover(),
        );
        ui.painter()
            .circle_filled(rect.center(), theme.status_dot_size.value() * 0.5, color);
        ui.label(
            egui::RichText::new(kind.status())
                .size(ATTN_PRIMITIVE_12.value())
                .color(color),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            Button::new(kind.action())
                .variant(ButtonVariant::Secondary)
                .show(ui, theme);
        });
    });
}

/// 우측 상세 — identity → 배너 → 사유 detail → 구분선 → 액션 바.
pub(super) fn detail_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let entry = &ENTRIES[SELECTED];
    let inner = rect.shrink(theme.spacing_md.value());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.spacing_mut().item_spacing.y = theme.spacing_sm.value();

    child.horizontal_top(|ui| {
        plugin_avatar(ui, theme, entry.name, PluginAvatarSize::Detail);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(entry.name)
                        .size(theme.font_size_max.value())
                        .strong()
                        .color(theme.text_primary().to_egui()),
                );
                tag(
                    ui,
                    theme,
                    &format!("v{}", entry.version),
                    TagVariant::Default,
                    false,
                );
                if entry.builtin {
                    ui.label(
                        egui::RichText::new("built-in")
                            .size(theme.font_size_caption.value())
                            .color(theme.accent_agent().to_egui()),
                    );
                }
            });
            ui.label(
                egui::RichText::new(entry.id)
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
        });
    });
    banner(&mut child, theme, entry.kind);
    reason_detail(&mut child, theme, entry.kind);
    child.separator();
    action_bar(&mut child, theme, entry.kind);
}

/// 상세 영역의 빈 상태. 목록 배경과 너비는 유지한다.
pub(super) fn empty_detail_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let inner = rect.shrink(theme.spacing_md.value());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.add_space(theme.spacing_xl.value() * 2.0);
    child.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        ui.label(
            egui::RichText::new("No plugins need attention")
                .size(theme.font_size_body.value())
                .color(theme.text_secondary().to_egui()),
        );
        ui.label(
            egui::RichText::new(
                "Rejected or failing plugins show up here with the reason and what to do next.",
            )
            .size(ATTN_PRIMITIVE_12.value())
            .color(theme.text_muted().to_egui()),
        );
    });
}

/// 빈 상태의 목록 패널 — 행이 없다. 배경과 폭은 그대로다.
pub(super) fn empty_list_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());
}

/// 비교 카드마다 세로 배치와 위쪽 정렬을 적용해 내용이 가로로 넘치지 않게 한다.
pub(super) fn reason_cards(ui: &mut egui::Ui, theme: &Theme) {
    let card_w = theme.measure_sm.value() * 0.5;
    ui.with_layout(
        egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
        |ui| cards_row(ui, theme, card_w),
    );
}

fn cards_row(ui: &mut egui::Ui, theme: &Theme, card_w: f32) {
    for kind in [
        Kind::UnknownKey,
        Kind::SignatureInvalid,
        Kind::PermissionsChanged,
        Kind::HealthError,
    ] {
        ui.allocate_ui_with_layout(
            egui::vec2(card_w, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui: &mut egui::Ui| {
                ui.set_max_width(card_w);
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                banner(ui, theme, kind);
                reason_detail(ui, theme, kind);
            },
        );
    }
}
