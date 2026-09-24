//! 원격 프로필, Attach 대상, 연결 설정 폼 예제.
//! 공용 탭·프로토콜 필터·SSH config 목록 뷰를 사용하며 실제 연결은 하지 않는다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    Button, ButtonVariant, IconButton, IconButtonVariant, LocalSshHost, LocalSshSectionData,
    ProtocolFilterItem, ProtocolFilterLabels, Spinner, TabStripData, TagVariant,
    draw_local_ssh_section, draw_protocol_filter_body, draw_protocol_filter_button, draw_tab_strip,
    select, tag,
};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: LogicalPx = LogicalPx(520.0);

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

/// 로컬 ssh config 항목 — 본체와 같은 타입으로 둔다(공용 view 의 입력).
///
/// tasty 레코드가 아니라 사용자의 `~/.ssh/config` 라 행 액션은 가져오기 하나뿐이고,
/// 이미 가져온 alias 는 액션 대신 Tag 로 상태만 보인다.
const LOCAL_HOSTS: &[LocalSshHost<'static>] = &[
    LocalSshHost {
        alias: "gb10",
        target: "maya@gb10.local:22",
        in_profiles: false,
    },
    LocalSshHost {
        alias: "prod-web",
        target: "deploy@10.0.4.12:22",
        in_profiles: true,
    },
    // 긴 별칭으로 오른쪽 동작 버튼을 유지하며 말줄임되는지 확인한다.
    LocalSshHost {
        alias: "eu-west-build-farm-bastion-01",
        target: "ci-runner@bastion-01.eu-west.build.example.com:2222",
        in_profiles: false,
    },
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    kit::icon(
                        ui,
                        icons::REMOTE,
                        theme.icon_glyph_size_md,
                        theme.text_secondary().to_egui(),
                    );
                    kit::title(ui, theme, "Remote connections");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        IconButton::new().variant(IconButtonVariant::Ghost).show(
                            ui,
                            theme,
                            &|ui, rect, c| icons::CLOSE.image(rect.height(), c).paint_at(ui, rect),
                        );
                    });
                });
            });
            tab_bar(ui, theme, 0);

            // 서로 다른 프로토콜이 둘 이상인 예제이므로 필터 버튼을 표시한다.
            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                ui.horizontal(|ui| {
                    Button::new("Add profile")
                        .variant(ButtonVariant::Secondary)
                        .leading_icon(&|ui, rect, c| {
                            icons::PLUS.image(rect.height(), c).paint_at(ui, rect)
                        })
                        .show(ui, theme);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        draw_protocol_filter_button(ui, theme, "Filter", false);
                    });
                });
            });

            // 프로필과 SSH config 목록을 같은 스크롤 영역에 둔다.
            kit::region_sym(ui, theme.spacing_md, LogicalPx(0.0), |ui| {
                for (i, p) in PROFILES.iter().enumerate() {
                    if i > 0 {
                        kit::hsep(ui, theme);
                    }
                    profile_row(ui, theme, p);
                }
                // 공용 목록 뷰가 구분선과 간격을 포함하므로 중복해서 그리지 않는다.
                draw_local_ssh_section(
                    ui,
                    theme,
                    &LocalSshSectionData {
                        heading: "From ssh config",
                        path: "~/.ssh/config",
                        in_profiles_tag: "in profiles",
                        add_label: "Add profile",
                        empty_message: "No hosts in ~/.ssh/config.",
                        hosts: LOCAL_HOSTS,
                    },
                );
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "520×460 · bg-panel"),
            ("tabs", "Remote profiles / Attach / Passkeys · bg-sidebar"),
            (
                "add-bar",
                "Add profile (left) · protocol filter (right) once two protocols are stored",
            ),
            ("row", "name · status Tag · target mono · passkey/detecting"),
            ("detecting", "Spinner 12"),
            ("actions", "IconButton sm ×3 (right)"),
            (
                "local ssh",
                "section header · 2-line row · ghost Add profile · Tag when already in profiles",
            ),
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
        "Remote profiles store connection details for their protocol. SSH profiles supply the connection and authentication settings; Attach records add the remote Tasty executable and port discovery options.",
    );
}

/// 공용 탭 바에 라벨과 활성 항목을 전달한다. 배경은 카드 전체 폭을 채운다.
fn tab_bar(ui: &mut egui::Ui, theme: &Theme, active: usize) {
    draw_tab_strip(
        ui,
        theme,
        &TabStripData {
            labels: &["Remote profiles", "Attach", "Passkeys"],
            active,
            x_range: ui.max_rect().x_range(),
        },
    );
}

struct Attach {
    name: &'static str,
    label: &'static str,
    mode: &'static str,
    target: &'static str,
    tasty: &'static str,
    port: &'static str,
    inactive: bool,
}

const ATTACHES: &[Attach] = &[
    Attach {
        name: "gb10",
        label: "us-east",
        mode: "profile",
        target: "→ prod-web",
        tasty: "tasty",
        port: "auto",
        inactive: false,
    },
    Attach {
        name: "edge-direct",
        label: "",
        mode: "inline",
        target: "root@edge.example.com",
        tasty: "/opt/tasty/bin/tasty",
        port: "file-unix",
        inactive: false,
    },
    Attach {
        name: "legacy-attach",
        label: "",
        mode: "profile",
        target: "→ legacy-box",
        tasty: "tasty",
        port: "subcommand",
        inactive: true,
    },
];

pub fn draw_attach(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            attach_header(ui, theme);
            tab_bar(ui, theme, 1);

            kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                ui.horizontal(|ui| {
                    Button::new("Add attach")
                        .variant(ButtonVariant::Secondary)
                        .leading_icon(&|ui, rect, c| {
                            icons::PLUS.image(rect.height(), c).paint_at(ui, rect)
                        })
                        .show(ui, theme);
                });
            });

            kit::region_sym(ui, theme.spacing_md, LogicalPx(0.0), |ui| {
                for (i, a) in ATTACHES.iter().enumerate() {
                    if i > 0 {
                        kit::hsep(ui, theme);
                    }
                    attach_row(ui, theme, a);
                }
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "520×460 · bg-panel · middle tab"),
            ("row1", "name · (label) · mode Tag · inactive badge"),
            ("row2", "target mono (→ profile | user@host[:port])"),
            ("row3", "tasty: + port: captions · gap 12"),
            ("add-bar", "Add attach only — no protocol filter"),
        ],
        &[
            TokenChip::new(
                "accent-warning",
                "inactive badge",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new(
                "text-disabled",
                "inactive name",
                theme.text_disabled().to_egui(),
            ),
            TokenChip::new("text-muted", "target mono", theme.text_muted().to_egui()),
            TokenChip::new("bg-sidebar", "tab strip", theme.bg_sidebar().to_egui()),
        ],
    );

    spec::do_(
        ui,
        theme,
        "Keep remote_tasty and port discovery on the Attach, not the ssh profile — an \
         ssh profile is reusable connection info; how to find the remote tasty binary \
         is attach-specific.",
    );
}

fn attach_header(ui: &mut egui::Ui, theme: &Theme) {
    kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            kit::icon(
                ui,
                icons::REMOTE,
                theme.icon_glyph_size_md,
                theme.text_secondary().to_egui(),
            );
            kit::title(ui, theme, "Remote connections");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                IconButton::new().variant(IconButtonVariant::Ghost).show(
                    ui,
                    theme,
                    &|ui, rect, c| icons::CLOSE.image(rect.height(), c).paint_at(ui, rect),
                );
            });
        });
    });
}

fn attach_row(ui: &mut egui::Ui, theme: &Theme, a: &Attach) {
    kit::region_sym(ui, LogicalPx(0.0), theme.spacing_sm, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    let name_color = if a.inactive {
                        theme.text_disabled()
                    } else {
                        theme.text_primary()
                    };
                    ui.label(
                        egui::RichText::new(a.name)
                            .size(theme.font_size_body.value())
                            .strong()
                            .color(name_color.to_egui()),
                    );
                    if !a.label.is_empty() {
                        ui.label(
                            egui::RichText::new(format!("({})", a.label))
                                .size(theme.font_size_body.value())
                                .color(theme.text_muted().to_egui()),
                        );
                    }
                    tag(ui, theme, a.mode, TagVariant::Default, false);
                    if a.inactive {
                        warn_pill(ui, theme, "inactive");
                    }
                });
                ui.label(
                    egui::RichText::new(a.target)
                        .monospace()
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                );
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                    kit::caption(ui, theme, &format!("tasty: {}", a.tasty), true);
                    kit::caption(ui, theme, &format!("port: {}", a.port), true);
                });
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                for glyph in [icons::CLOSE, icons::EDIT] {
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

/// accent-warning pill — 디자인 배지 (12% fill / 40% border / mono micro).
/// gallery 미러(`RemoteFrame` attach)의 inactive 배지는 아이콘 없는 텍스트 pill.
fn warn_pill(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    let warn = theme.accent_warning().to_egui();
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::monospace(theme.font_size_micro.value()),
        egui::Color32::PLACEHOLDER,
    );
    let pad_x = theme.spacing_sm.value() * 0.75; // 디자인 padding 0 6 (raw)
    let h = 16.0; // 디자인 배지 고정 높이 (size-16)
    let w = pad_x * 2.0 + galley.rect.width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let radius = theme.corner_radius_sm.value();
    // 채움은 `tint-fill-alpha`. 테두리 계수는 디자인이 "채움만" 으로 한정한
    // 부분 사용이라(docs/design/systems/theme.md#ui-코드의-색상-접근) 이 자리 고유 값으로 남는다.
    const BADGE_STROKE_OPACITY: f32 = 0.4;
    ui.painter()
        .rect_filled(rect, radius, warn.gamma_multiply(theme.tint_fill_alpha()));
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(
            theme.border_width.value(),
            warn.gamma_multiply(BADGE_STROKE_OPACITY),
        ),
        egui::StrokeKind::Inside,
    );
    let pos = egui::pos2(
        rect.left() + pad_x,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter().galley(pos, galley, warn);
}

/// 폼 라벨 컬럼 폭 — 디자인 `--tasty-remote-label-col`(size-112).
const LABEL_COL: LogicalPx = LogicalPx(112.0);
/// 폼 카드 폭 — 디자인 `RemoteFormFrame` maxWidth 460 (raw).
const FORM_WIDTH: LogicalPx = LogicalPx(460.0);

pub fn draw_attach_form(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for inline in [false, true] {
            // 같은 폼을 두 개 그리므로 위젯 ID의 범위를 나눈다.
            ui.push_id(inline, |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                    kit::caption(
                        ui,
                        theme,
                        if inline {
                            "inline ssh info"
                        } else {
                            "reference an ssh profile"
                        },
                        false,
                    );
                    attach_form_card(ui, theme, inline);
                });
            });
        }
    });

    spec::meta(
        ui,
        theme,
        &[
            ("toggle", "SSH profile ↔ Direct (inline)"),
            ("ref", "ssh_ref dropdown of ssh profiles"),
            ("inline", "host · user · port · shell · passkey"),
            ("remote tasty", "Executable (def. tasty)"),
            ("port", "auto / subcommand / file-unix / file-windows"),
            ("port file", "optional — overrides port mode"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "active segment / active tab / Save",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "text-on-accent",
                "active segment ink",
                theme.text_on_accent().to_egui(),
            ),
            TokenChip::new("text-muted", "labels / hints", theme.text_muted().to_egui()),
        ],
    );
}

fn attach_form_card(ui: &mut egui::Ui, theme: &Theme, inline: bool) {
    kit::frame_card(ui, theme, FORM_WIDTH, kit::panel_fill(theme), |ui| {
        attach_header(ui, theme);
        tab_bar(ui, theme, 1);

        kit::region_sym(ui, theme.spacing_lg, theme.spacing_md, |ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            ui.label(
                egui::RichText::new("New attach")
                    .size(theme.font_size_body.value())
                    .strong()
                    .color(theme.text_primary().to_egui()),
            );
            form_row(ui, theme, "Name", |ui| {
                kit::field(ui, theme, None, "gb10", false, false);
            });
            form_row(ui, theme, "Label", |ui| {
                if inline {
                    kit::field(ui, theme, None, "optional", true, false);
                } else {
                    kit::field(ui, theme, None, "us-east", false, false);
                }
            });
            form_row(ui, theme, "Connection", |ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value() * 0.75; // gap 6 (raw)
                seg_chip(ui, theme, "SSH profile", !inline);
                seg_chip(ui, theme, "Direct (inline)", inline);
            });
            if inline {
                form_row(ui, theme, "Host", |ui| {
                    kit::field(ui, theme, None, "edge.example.com", false, true);
                });
                form_row(ui, theme, "User", |ui| {
                    kit::field(ui, theme, None, "root", false, false);
                });
                form_row(ui, theme, "Port", |ui| {
                    kit::field(ui, theme, Some(LogicalPx(96.0)), "22", false, true);
                });
                form_row(ui, theme, "Shell", |ui| {
                    let mut sel = 0usize;
                    select(
                        ui,
                        theme,
                        "remote_attach_shell",
                        &mut sel,
                        &["auto", "bash", "zsh", "fish"],
                        ui.available_width(),
                        true,
                    );
                });
                form_row(ui, theme, "Passkey", |ui| {
                    let mut sel = 0usize;
                    select(
                        ui,
                        theme,
                        "remote_attach_passkey",
                        &mut sel,
                        &["(none)", "edge-pem"],
                        ui.available_width(),
                        true,
                    );
                });
            } else {
                form_row(ui, theme, "SSH profile", |ui| {
                    let mut sel = 0usize;
                    select(
                        ui,
                        theme,
                        "remote_attach_ssh_ref",
                        &mut sel,
                        &[
                            "(select a profile)",
                            "prod-web (us-east)",
                            "db-primary",
                            "legacy-box",
                        ],
                        ui.available_width(),
                        true,
                    );
                });
            }
            ui.add_space(theme.spacing_xs.value());
            ui.label(
                egui::RichText::new("REMOTE TASTY")
                    .monospace()
                    .size(theme.font_size_micro.value())
                    .color(theme.text_muted().to_egui()),
            );
            form_row(ui, theme, "Executable", |ui| {
                kit::field(
                    ui,
                    theme,
                    None,
                    if inline {
                        "/opt/tasty/bin/tasty"
                    } else {
                        "tasty"
                    },
                    false,
                    true,
                );
            });
            form_row(ui, theme, "Port mode", |ui| {
                let mut sel = 0usize;
                // 두 variant 카드가 같은 spec 에 그려지므로 salt 를 분리한다.
                let salt = if inline {
                    "remote_attach_port_mode_inline"
                } else {
                    "remote_attach_port_mode_ref"
                };
                select(
                    ui,
                    theme,
                    salt,
                    &mut sel,
                    &["auto", "subcommand", "file-unix", "file-windows"],
                    ui.available_width(),
                    true,
                );
            });
            form_row(ui, theme, "Port file", |ui| {
                if inline {
                    kit::field(ui, theme, None, "/run/user/1000/tasty/port", false, true);
                } else {
                    kit::field(ui, theme, None, "optional path", true, true);
                }
            });
            // hint — 입력 컬럼(112+12)에 맞춰 들여쓴 캡션.
            ui.horizontal(|ui| {
                ui.add_space((LABEL_COL + theme.spacing_md).value());
                ui.label(
                    egui::RichText::new(
                        "Optional — an explicit path takes precedence over the port mode.",
                    )
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
                );
            });
        });

        kit::hsep(ui, theme);
        kit::region_sym(ui, theme.spacing_lg, theme.spacing_md, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                Button::new("Save")
                    .variant(ButtonVariant::Primary)
                    .show(ui, theme);
                Button::new("Cancel")
                    .variant(ButtonVariant::Ghost)
                    .show(ui, theme);
            });
        });
    });
}

/// 폼 한 행 — 디자인 grid `[--tasty-remote-label-col 1fr]` columnGap 12 전사.
fn form_row(ui: &mut egui::Ui, theme: &Theme, label: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
        ui.allocate_ui_with_layout(
            egui::vec2(LABEL_COL.value(), theme.item_height_interactive.value()),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(
                    egui::RichText::new(label)
                        .size(theme.font_size_body.value())
                        .color(theme.text_muted().to_egui()),
                );
            },
        );
        add(ui);
    });
}

/// 연결 방식은 값을 고르는 항목이므로 밑줄 탭 대신 채운 세그먼트로 표시한다.
fn seg_chip(ui: &mut egui::Ui, theme: &Theme, label: &str, active: bool) {
    let h = theme.item_height_interactive.value();
    let font = egui::FontId::proportional(theme.font_size_body.value());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, egui::Color32::PLACEHOLDER);
    let w = galley.rect.width() + theme.spacing_md.value() * 2.0; // padding 0 12
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let (fill, border, fg) = if active {
        (
            theme.accent_primary(),
            theme.border_strong(),
            theme.text_on_accent(),
        )
    } else {
        (
            theme.surface_raised(),
            theme.border_default(),
            theme.text_secondary(),
        )
    };
    let radius = theme.corner_radius.value();
    ui.painter().rect_filled(rect, radius, fill.to_egui());
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.border_width.value(), border.to_egui()),
        egui::StrokeKind::Inside,
    );
    let pos = egui::pos2(
        rect.center().x - galley.rect.width() * 0.5,
        rect.center().y - galley.rect.height() * 0.5,
    );
    ui.painter().galley(pos, galley, fg.to_egui());
}

fn profile_row(ui: &mut egui::Ui, theme: &Theme, p: &Profile) {
    kit::region_sym(ui, LogicalPx(0.0), theme.spacing_sm, |ui| {
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

/// 필터 목록의 프로토콜 — 디자인 seed 와 같은 넷이고 마지막 하나가 미지 kind 다.
const FILTER_PROTOCOLS: &[(&str, bool)] = &[
    ("ssh", false),
    ("smb", false),
    ("http", false),
    ("snb", true),
];

/// 공용 프로토콜 필터의 닫힌 버튼과 열린 목록을 나란히 보여준다.
pub fn draw_filter(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            kit::caption(ui, theme, "closed · nothing hidden", false);
            draw_protocol_filter_button(ui, theme, "Filter", false);
        });
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            kit::caption(ui, theme, "closed · 1 of 4 hidden", false);
            draw_protocol_filter_button(ui, theme, "Filter · 3/4", true);
        });
    });
    // 목록 높이가 확보되도록 열린 드롭다운은 별도 행에 놓는다.
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        // 미지원 프로토콜을 제외한 상태를 보여준다. draft는 숨길 항목의 집합이다.
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            kit::caption(ui, theme, "open · draft = hidden set", false);
            let mut draft: std::collections::HashSet<String> =
                std::iter::once("snb".to_string()).collect();
            let items: Vec<ProtocolFilterItem<'_>> = FILTER_PROTOCOLS
                .iter()
                .map(|(name, unknown)| ProtocolFilterItem {
                    name,
                    unknown: *unknown,
                })
                .collect();
            // 공용 목록 뷰와 같은 최소폭을 사용한다.
            kit::frame_card_popover(
                ui,
                theme,
                tasty_ui_widgets::FILTER_DROPDOWN_MIN_WIDTH,
                kit::raised_fill(theme),
                |ui| {
                    kit::region_sym(ui, theme.spacing_md, theme.spacing_sm, |ui| {
                        // 내용에 맞춰 크기가 정해지는 카드에서도 목록 높이를 먼저 확보한다.
                        ui.set_min_height(tasty_ui_widgets::FILTER_DROPDOWN_MAX_HEIGHT.value());
                        draw_protocol_filter_body(
                            ui,
                            theme,
                            &items,
                            &ProtocolFilterLabels {
                                title: "Filter by protocol",
                                select_all: "Select all",
                                deselect_all: "Deselect all",
                                reset: "Reset",
                                apply: "Apply",
                                unknown: "unknown",
                                unknown_hint: "tasty has no dedicated form for this protocol.",
                            },
                            &mut draft,
                        );
                    });
                },
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "button",
                "funnel + label · raised when idle, accent when filtering",
            ),
            (
                "label",
                "Filter · <selected>/<total> once something is hidden",
            ),
            (
                "dropdown",
                "216 min-width · title · checkbox list · bulk row · Reset/Apply",
            ),
            (
                "unknown",
                "warn badge on a kind tasty has no dedicated form for",
            ),
            ("commit", "apply-on-confirm — the draft is the hidden set"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "button fill while filtering",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new(
                "surface-raised",
                "idle button · dropdown",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new(
                "border-strong",
                "idle button · dropdown border",
                theme.border_strong().to_egui(),
            ),
            TokenChip::new(
                "accent-warning",
                "unknown badge",
                theme.accent_warning().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "The button only appears when the stored profiles carry two or more protocols — \
         one protocol has nothing to filter. Checking a box does not hide a row: the draft \
         is committed on Apply, which is what makes a multi-box change one decision.",
    );
}
