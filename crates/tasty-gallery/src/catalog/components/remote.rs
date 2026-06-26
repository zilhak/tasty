//! Remote connections — 디자인(4) Overlays `remote` Spec (신규).
//!
//! 520×460 모달. 헤더(remote icon + title + close) · 2 탭(Remote profiles /
//! Passkeys, bg-sidebar) · Add profile 버튼행 · ProfileRow 리스트(name + (label)
//! + type Tag + target mono + passkey caption/detecting Spinner + 우측 IconButton ×3).

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    Button, ButtonVariant, IconButton, IconButtonVariant, Spinner, TagVariant, tag,
};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: f32 = 520.0;

struct Profile {
    name: &'static str,
    label: &'static str,
    tag: &'static str,
    target: &'static str,
    passkey: &'static str,
    detecting: bool,
}

const PROFILES: &[Profile] = &[
    Profile {
        name: "prod-web",
        label: "us-east",
        tag: "ssh",
        target: "deploy@10.0.4.12",
        passkey: "ed25519-main",
        detecting: false,
    },
    Profile {
        name: "db-primary",
        label: "",
        tag: "ssh",
        target: "postgres@db.internal:2222",
        passkey: "",
        detecting: false,
    },
    Profile {
        name: "edge-cache",
        label: "staging",
        tag: "ssh",
        target: "root@edge.example.com",
        passkey: "edge-pem",
        detecting: true,
    },
    Profile {
        name: "media-nas",
        label: "lab",
        tag: "smb",
        target: "host=nas.local  share=media",
        passkey: "nas-cred",
        detecting: false,
    },
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            // 헤더.
            kit::region_sym(
                ui,
                theme.spacing_md.value(),
                theme.spacing_sm.value(),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                        kit::icon(
                            ui,
                            icons::REMOTE,
                            theme.icon_glyph_size_md.value(),
                            theme.text_secondary().to_egui(),
                        );
                        kit::title(ui, theme, "Remote connections");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            IconButton::new().variant(IconButtonVariant::Ghost).show(
                                ui,
                                theme,
                                &|ui, rect, c| {
                                    icons::CLOSE.image(rect.height(), c).paint_at(ui, rect)
                                },
                            );
                        });
                    });
                },
            );
            // 2 탭 (bg-sidebar).
            egui::Frame::new()
                .fill(theme.bg_sidebar().to_egui())
                .inner_margin(egui::Margin::symmetric(theme.spacing_md.value() as i8, 0))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                        tab_btn(ui, theme, "Remote profiles", true);
                        tab_btn(ui, theme, "Passkeys", false);
                    });
                });
            kit::hsep(ui, theme);

            // Add profile 버튼행.
            kit::region_sym(
                ui,
                theme.spacing_md.value(),
                theme.spacing_sm.value(),
                |ui| {
                    ui.horizontal(|ui| {
                        Button::new("Add profile")
                            .variant(ButtonVariant::Secondary)
                            .leading_icon(&|ui, rect, c| {
                                icons::PLUS.image(rect.height(), c).paint_at(ui, rect)
                            })
                            .show(ui, theme);
                    });
                },
            );

            // ProfileRow 리스트.
            kit::region_sym(ui, theme.spacing_md.value(), 0.0, |ui| {
                for (i, p) in PROFILES.iter().enumerate() {
                    if i > 0 {
                        kit::hsep(ui, theme);
                    }
                    profile_row(ui, theme, p);
                }
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "520×460 · bg-panel"),
            ("tabs", "Remote profiles / Passkeys · bg-sidebar"),
            ("row", "name · status Tag · target mono · passkey/detecting"),
            ("detecting", "Spinner 12"),
            ("actions", "IconButton sm ×3 (right)"),
        ],
        &[
            TokenChip::new("bg-sidebar", "tab strip", theme.bg_sidebar().to_egui()),
            TokenChip::new("accent-success", "online", theme.accent_success().to_egui()),
            TokenChip::new("accent-agent", "passkey", theme.accent_agent().to_egui()),
            TokenChip::new("text-muted", "target mono", theme.text_muted().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "Tasty has no remote security model of its own — every profile is an SSH \
         target, and identity is delegated to passkeys at that boundary.",
    );
}

fn tab_btn(ui: &mut egui::Ui, theme: &Theme, label: &str, active: bool) {
    let h = theme.titlebar_height.value();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme.font_size_body.value()),
        egui::Color32::PLACEHOLDER,
    );
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(galley.rect.width(), h), egui::Sense::hover());
    let fg = if active {
        theme.text_primary()
    } else {
        theme.text_muted()
    };
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - galley.rect.height() * 0.5),
        galley,
        fg.to_egui(),
    );
    if active {
        let bar = egui::Rect::from_min_size(
            egui::pos2(
                rect.left(),
                rect.bottom() - theme.tab_indicator_width.value(),
            ),
            egui::vec2(rect.width(), theme.tab_indicator_width.value()),
        );
        ui.painter()
            .rect_filled(bar, 0.0, theme.accent_primary().to_egui());
    }
}

fn profile_row(ui: &mut egui::Ui, theme: &Theme, p: &Profile) {
    kit::region_sym(ui, 0.0, theme.spacing_sm.value(), |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    ui.label(
                        egui::RichText::new(p.name)
                            .size(theme.font_size_body.value())
                            .strong()
                            .color(theme.text_primary().to_egui()),
                    );
                    if !p.label.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("({})", p.label))
                                .size(theme.font_size_body.value())
                                .color(theme.text_muted().to_egui()),
                        );
                    }
                    tag(ui, theme, p.tag, TagVariant::Default, false);
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    ui.label(
                        egui::RichText::new(p.target)
                            .monospace()
                            .size(theme.font_size_caption.value())
                            .color(theme.text_muted().to_egui()),
                    );
                });
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    let passkey = if p.passkey.is_empty() {
                        "—"
                    } else {
                        p.passkey
                    };
                    kit::caption(ui, theme, &format!("passkey: {passkey}"), true);
                    if p.detecting {
                        Spinner::new()
                            .size(theme.font_size_term_sm.value())
                            .show(ui, theme);
                        kit::caption(ui, theme, "detecting…", false);
                    }
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for glyph in [icons::TRASH, icons::PLUG, icons::EDIT] {
                    IconButton::new()
                        .variant(IconButtonVariant::Ghost)
                        .size(tasty_ui_widgets::ControlSize::Sm)
                        .show(ui, theme, &|ui, rect, c| {
                            glyph.image(rect.height(), c).paint_at(ui, rect)
                        });
                }
            });
        });
    });
}
