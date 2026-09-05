use crate::file::format::FileFormatRegistry;
use crate::file::handler::{
    FileHandlerRegistry, HandlerAction, HandlerOwner, UserHandlerActionDecl, UserHandlerUpsertDecl,
};
use crate::i18n::t;

use super::{AddHandlerActionKind, AddHandlerForm, FileHandlerEditDraft, draw_intro_block};
use tasty_ui_widgets::vspace;

/// Handlers sub-tab — Enabled 토글, user-origin 삭제, user 추가 form.
pub(super) fn draw_handlers(
    ui: &mut egui::Ui,
    fh: &mut FileHandlerEditDraft,
    file_format: &FileFormatRegistry,
    file_handler: &FileHandlerRegistry,
) {
    draw_intro_block(
        ui,
        "settings.file_handler.handlers.description",
        &[
            "settings.file_handler.handlers.bullet_what",
            "settings.file_handler.handlers.bullet_priority",
            "settings.file_handler.handlers.bullet_owner",
            "settings.file_handler.handlers.bullet_action_surface",
            "settings.file_handler.handlers.bullet_action_ipc",
            "settings.file_handler.handlers.bullet_action_system",
        ],
    );

    let th = crate::theme::theme();
    let ids = file_handler.list_handlers();
    if ids.is_empty() {
        ui.label(t("settings.file_handler.handlers.empty"));
    } else {
        let mut rows: Vec<crate::file::handler::FileHandler> = ids
            .iter()
            .filter_map(|id| file_handler.handler(id))
            .collect();
        rows.sort_by(|a, b| {
            a.priority
                .cmp(&b.priority)
                .then_with(|| a.id.as_str().cmp(b.id.as_str()))
        });

        egui::Grid::new("file_handler_handlers_grid")
            .num_columns(7)
            .striped(true)
            .spacing(egui::vec2(th.spacing_md.value(), th.spacing_xs.value()))
            .show(ui, |ui| {
                ui.strong(t("settings.file_handler.handlers.col_status"));
                ui.strong(t("settings.file_handler.handlers.col_priority"));
                ui.strong(t("settings.file_handler.handlers.col_id"));
                ui.strong(t("settings.file_handler.handlers.col_owner"));
                ui.strong(t("settings.file_handler.handlers.col_detector"));
                ui.strong(t("settings.file_handler.handlers.col_action"));
                ui.strong("");
                ui.end_row();

                for h in &rows {
                    let want_enabled = fh
                        .handler_enabled
                        .get(&h.id)
                        .copied()
                        .unwrap_or(!h.disabled);
                    let mut checked = want_enabled;
                    if tasty_ui_widgets::switch(ui, &th, &mut checked, None, true).changed() {
                        fh.handler_enabled.insert(h.id.clone(), checked);
                    }
                    ui.label(h.priority.to_string());
                    ui.label(h.id.as_str());
                    ui.label(handler_owner_label(&h.owner));
                    ui.label(h.detector.as_str());
                    ui.label(handler_action_summary(&h.action));
                    if matches!(h.owner, HandlerOwner::User) {
                        let pending = fh.remove_handler.contains(&h.id);
                        ui.horizontal(|ui| {
                            if pending {
                                ui.weak(t("settings.file_handler.common.pending_remove"));
                                if ui.small_button(t("button.cancel")).clicked() {
                                    fh.remove_handler.remove(&h.id);
                                }
                            } else if ui
                                .small_button(t("settings.file_handler.handlers.remove_user"))
                                .clicked()
                            {
                                fh.remove_handler.insert(h.id.clone());
                            }
                        });
                    } else {
                        ui.label("");
                    }
                    ui.end_row();
                }
            });
    }

    vspace(ui, th.spacing_md);
    ui.separator();
    vspace(ui, th.spacing_xs);

    draw_add_handler_form(ui, fh, file_format);
}

fn draw_add_handler_form(
    ui: &mut egui::Ui,
    fh: &mut FileHandlerEditDraft,
    file_format: &FileFormatRegistry,
) {
    let th = crate::theme::theme();
    let toggle_label = if fh.add_handler_form.open {
        t("settings.file_handler.handlers.add_close")
    } else {
        t("settings.file_handler.handlers.add_open")
    };
    if ui.button(toggle_label).clicked() {
        fh.add_handler_form.open = !fh.add_handler_form.open;
        fh.add_handler_form.error = None;
    }
    if !fh.add_handler_form.open {
        for decl in &fh.add_handler {
            ui.weak(format!(
                "{} user/{}",
                t("settings.file_handler.handlers.pending_add"),
                decl.id.strip_prefix("user/").unwrap_or(&decl.id)
            ));
        }
        return;
    }
    let detector_ids = file_format.list_detectors();
    egui::Frame::new()
        .inner_margin(egui::vec2(th.spacing_sm.value(), th.spacing_xs.value()))
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .show(ui, |ui| {
            egui::Grid::new("file_handler_handlers_add_grid")
                .num_columns(2)
                .spacing(egui::vec2(th.spacing_sm.value(), th.spacing_xs.value()))
                .show(ui, |ui| {
                    ui.label(t("settings.file_handler.handlers.field_short_name"));
                    ui.add(
                        egui::TextEdit::singleline(&mut fh.add_handler_form.short_name_input)
                            .hint_text("my-viewer")
                            .desired_width(th.field_width_lg.value()),
                    );
                    ui.end_row();

                    ui.label(t("settings.file_handler.handlers.field_detector"));
                    egui::ComboBox::from_id_salt("file_handler_handlers_add_detector")
                        .selected_text(if fh.add_handler_form.detector_id_input.is_empty() {
                            t("settings.file_handler.handlers.field_detector_select").to_string()
                        } else {
                            fh.add_handler_form.detector_id_input.clone()
                        })
                        .show_ui(ui, |ui| {
                            for id in &detector_ids {
                                ui.selectable_value(
                                    &mut fh.add_handler_form.detector_id_input,
                                    id.as_str().to_string(),
                                    id.as_str(),
                                );
                            }
                        });
                    ui.end_row();

                    ui.label(t("settings.file_handler.handlers.field_priority"));
                    ui.add(
                        egui::TextEdit::singleline(&mut fh.add_handler_form.priority_input)
                            .hint_text("100")
                            .desired_width(80.0),
                    );
                    ui.end_row();

                    ui.label(t("settings.file_handler.handlers.field_action_kind"));
                    egui::ComboBox::from_id_salt("file_handler_handlers_add_action_kind")
                        .selected_text(match fh.add_handler_form.action_kind {
                            AddHandlerActionKind::OpenSurface => {
                                t("settings.file_handler.handlers.action_open_surface").to_string()
                            }
                            AddHandlerActionKind::Ipc => {
                                t("settings.file_handler.handlers.action_ipc").to_string()
                            }
                            AddHandlerActionKind::System => {
                                t("settings.file_handler.handlers.action_system").to_string()
                            }
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut fh.add_handler_form.action_kind,
                                AddHandlerActionKind::OpenSurface,
                                t("settings.file_handler.handlers.action_open_surface"),
                            );
                            ui.selectable_value(
                                &mut fh.add_handler_form.action_kind,
                                AddHandlerActionKind::Ipc,
                                t("settings.file_handler.handlers.action_ipc"),
                            );
                            ui.selectable_value(
                                &mut fh.add_handler_form.action_kind,
                                AddHandlerActionKind::System,
                                t("settings.file_handler.handlers.action_system"),
                            );
                        });
                    ui.end_row();

                    match fh.add_handler_form.action_kind {
                        AddHandlerActionKind::OpenSurface => {
                            ui.label(t("settings.file_handler.handlers.field_surface_kind"));
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut fh.add_handler_form.action_surface_kind,
                                )
                                .hint_text("markdown_view")
                                .desired_width(th.field_width_lg.value()),
                            );
                            ui.end_row();
                            ui.label(t("settings.file_handler.handlers.field_param_key"));
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut fh.add_handler_form.action_param_key,
                                )
                                .hint_text("file")
                                .desired_width(120.0),
                            );
                            ui.end_row();
                        }
                        AddHandlerActionKind::Ipc => {
                            ui.label(t("settings.file_handler.handlers.field_ipc_method"));
                            ui.add(
                                egui::TextEdit::singleline(
                                    &mut fh.add_handler_form.action_ipc_method,
                                )
                                .hint_text("com.example.foo.open")
                                .desired_width(240.0),
                            );
                            ui.end_row();
                        }
                        AddHandlerActionKind::System => {}
                    }
                });
            if let Some(err) = &fh.add_handler_form.error {
                ui.colored_label(crate::theme::theme().red, err);
            }
            ui.horizontal(|ui| {
                if ui.button(t("button.add")).clicked() {
                    match build_add_handler_decl(&fh.add_handler_form) {
                        Ok(decl) => {
                            fh.add_handler.push(decl);
                            fh.add_handler_form = AddHandlerForm::default();
                        }
                        Err(e) => fh.add_handler_form.error = Some(e),
                    }
                }
                if ui.button(t("button.cancel")).clicked() {
                    fh.add_handler_form = AddHandlerForm::default();
                }
            });
        });
    for decl in &fh.add_handler {
        ui.weak(format!(
            "{} {}",
            t("settings.file_handler.handlers.pending_add"),
            decl.id
        ));
    }
}

fn build_add_handler_decl(form: &AddHandlerForm) -> Result<UserHandlerUpsertDecl, String> {
    let short = form.short_name_input.trim();
    if short.is_empty() {
        return Err(t("settings.file_handler.handlers.err_short_name_empty").to_string());
    }
    if !crate::file::handler::is_valid_handler_short_name(short) {
        return Err(t("settings.file_handler.handlers.err_short_name_invalid").to_string());
    }
    let detector = form.detector_id_input.trim();
    if detector.is_empty() {
        return Err(t("settings.file_handler.handlers.err_detector_missing").to_string());
    }
    if !crate::file::format::is_valid_detector_id(detector) {
        return Err(t("settings.file_handler.handlers.err_detector_invalid").to_string());
    }
    let priority_str = form.priority_input.trim();
    let priority = if priority_str.is_empty() {
        100
    } else {
        priority_str
            .parse::<i32>()
            .map_err(|_| t("settings.file_handler.handlers.err_priority_invalid").to_string())?
    };
    let action = match form.action_kind {
        AddHandlerActionKind::OpenSurface => {
            let sk = form.action_surface_kind.trim();
            if sk.is_empty() {
                return Err(t("settings.file_handler.handlers.err_surface_kind_empty").to_string());
            }
            let pk = form.action_param_key.trim();
            UserHandlerActionDecl::OpenSurface {
                surface_kind: sk.to_string(),
                param_key: if pk.is_empty() {
                    "file".to_string()
                } else {
                    pk.to_string()
                },
            }
        }
        AddHandlerActionKind::Ipc => {
            let m = form.action_ipc_method.trim();
            if m.is_empty() {
                return Err(t("settings.file_handler.handlers.err_ipc_method_empty").to_string());
            }
            UserHandlerActionDecl::Ipc {
                method: m.to_string(),
            }
        }
        AddHandlerActionKind::System => UserHandlerActionDecl::System,
    };
    Ok(UserHandlerUpsertDecl {
        id: format!("user/{}", short),
        detector: Some(detector.to_string()),
        priority: Some(priority),
        display_name_i18n_key: None,
        disabled: None,
        action: Some(action),
    })
}

fn handler_owner_label(owner: &HandlerOwner) -> String {
    match owner {
        HandlerOwner::Host => "host".into(),
        HandlerOwner::Plugin(id) => format!("plugin:{}", id),
        HandlerOwner::User => "user".into(),
    }
}

fn handler_action_summary(action: &HandlerAction) -> String {
    match action {
        HandlerAction::OpenSurface { surface_kind, .. } => format!("surface:{}", surface_kind),
        HandlerAction::Ipc { method, .. } => format!("ipc:{}", method),
        HandlerAction::System => "system".into(),
    }
}
