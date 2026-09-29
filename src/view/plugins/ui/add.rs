use crate::i18n::{t, t_fmt};
use crate::theme;
use tasty_type_geometry::length::LogicalPx;

/// 미리보기 이름의 primitive 폰트 크기. ui_scale을 적용하지 않는다(ADR-0035).
const ADD_PREVIEW_NAME_PRIMITIVE_16: LogicalPx = LogicalPx(16.0);

use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, vspace};

use super::attention::fingerprint_line;
use super::{
    AddPreview, AddTrustReason, AddTrustState, PluginsAction, PluginsSnapshot, PluginsUiState,
};

pub(super) fn draw_add_tab(
    ctx: &egui::Context,
    snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    actions: &mut Vec<PluginsAction>,
) {
    let th = theme::theme();

    egui::CentralPanel::default().show(ctx, |ui| {
        vspace(ui, th.spacing_md);
        if ui_state.add_preview.is_some() {
            draw_add_preview(ui, snapshot, ui_state, actions, &th);
        } else {
            draw_add_input(ui, snapshot, ui_state, &th);
        }
    });
}

/// `Add` 탭의 초기 화면 — 경로 입력 + 확인 + 찾기.
fn draw_add_input(
    ui: &mut egui::Ui,
    snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    th: &theme::Theme,
) {
    ui.label(t("plugins.add_path_label"));
    vspace(ui, th.spacing_sm);

    let mut submitted = false;
    ui.horizontal(|ui| {
        let edit = egui::TextEdit::singleline(&mut ui_state.add_path_input)
            .hint_text(tasty_egui_theme::hint_text(
                &crate::theme::theme(),
                t("plugins.add_path_placeholder"),
            ))
            .desired_width(ui.available_width() - 90.0);
        let resp = ui.add(edit);
        if resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            submitted = true;
        }
        if ui.button(t("plugins.add_confirm_path")).clicked() {
            submitted = true;
        }
    });

    if submitted {
        try_validate_path(ui_state, snapshot);
    }

    if let Some(err) = &ui_state.add_error {
        vspace(ui, th.spacing_sm);
        ui.label(egui::RichText::new(err).color(egui::Color32::from(th.accent_danger())));
    }

    vspace(ui, th.spacing_lg);
    ui.separator();
    vspace(ui, th.spacing_md);

    if ui.button(t("plugins.add_browse")).clicked() {
        let dialog = rfd::FileDialog::new();
        if let Some(path) = crate::stall_watchdog::without_stall_watch(|| dialog.pick_folder()) {
            ui_state.add_path_input = path.to_string_lossy().to_string();
            try_validate_path(ui_state, snapshot);
        }
    }
}

/// 검증된 매니페스트 정보를 보여주고 추가/취소 버튼.
fn draw_add_preview(
    ui: &mut egui::Ui,
    _snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    actions: &mut Vec<PluginsAction>,
    th: &theme::Theme,
) {
    // 표시 중 원본 preview가 필요하므로 복사해 사용한다.
    let preview = ui_state.add_preview.clone().expect("checked by caller");

    ui.heading(t("plugins.add_preview_heading"));
    vspace(ui, th.spacing_sm);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 60.0)
        .drag_to_scroll(false)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&preview.name)
                        .size(ADD_PREVIEW_NAME_PRIMITIVE_16.value())
                        .color(egui::Color32::from(th.text_primary())),
                );
                ui.label(format!("v{}", preview.version));
            });
            ui.label(
                egui::RichText::new(&preview.id)
                    .small()
                    .color(egui::Color32::from(th.text_muted())),
            );
            vspace(ui, th.spacing_sm);

            if !preview.description.is_empty() {
                ui.label(&preview.description);
                vspace(ui, th.spacing_sm);
            }
            if !preview.authors.is_empty() {
                ui.label(format!(
                    "{}: {}",
                    t("plugins.authors"),
                    preview.authors.join(", ")
                ));
            }
            if !preview.homepage.is_empty() {
                ui.label(format!("{}: {}", t("plugins.homepage"), preview.homepage));
            }
            vspace(ui, th.spacing_sm);

            ui.label(format!(
                "{}: {}",
                t("plugins.add_source_path"),
                preview.src_path
            ));
            vspace(ui, th.spacing_sm);

            ui.label(format!("{}:", t("plugins.surface_kinds")));
            if preview.surface_kinds.is_empty() {
                ui.label(t("plugins.none"));
            } else {
                ui.label(preview.surface_kinds.join(", "));
            }
            vspace(ui, th.spacing_sm);

            ui.label(format!("{}:", t("plugins.permissions")));
            if preview.permissions.is_empty() {
                ui.label(t("plugins.none"));
            } else {
                for token in &preview.permissions {
                    ui.label(format!("• {token}"));
                }
            }

            if let Some(msg) = &preview.already_installed {
                vspace(ui, th.spacing_md);
                ui.label(
                    egui::RichText::new(msg).color(egui::Color32::from(th.accent_attention())),
                );
            }
        });

    // Untrusted plugin 경고 — 빨간색 영역. 이미 설치된 plugin 은 표시 X
    // (그쪽이 더 의미 있는 메시지).
    if preview.already_installed.is_none() {
        draw_untrusted_warning(ui, &preview, th);
    }

    vspace(ui, th.spacing_md);
    ui.separator();
    vspace(ui, th.spacing_sm);
    // 추가할 수 없는 매니페스트는 Add를 숨기지 않고 disabled로 두고, 이유를 왼쪽에 적는다.
    let blocked_key = add_blocked_reason_key(&preview);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), ControlSize::Md.height(th)),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
            if let Some(key) = blocked_key {
                ui.label(
                    egui::RichText::new(t(key))
                        .size(th.font_size_caption.value())
                        .color(egui::Color32::from(th.text_muted())),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let add = Button::new(t("plugins.add_button"))
                    .variant(ButtonVariant::Primary)
                    .enabled(blocked_key.is_none())
                    .show(ui, th);
                let cancel = Button::new(t("button.cancel"))
                    .variant(ButtonVariant::Ghost)
                    .show(ui, th);
                if add.clicked() {
                    let action = match &preview.trust_state {
                        AddTrustState::Trusted => PluginsAction::Install {
                            src_path: preview.src_path.clone(),
                        },
                        AddTrustState::UntrustedWithPubkey {
                            fingerprint,
                            pubkey_b64,
                            ..
                        } => PluginsAction::TrustAndInstall {
                            src_path: preview.src_path.clone(),
                            plugin_id: preview.id.clone(),
                            pubkey_b64: pubkey_b64.clone(),
                            permissions: preview.permissions.clone(),
                            publisher_fingerprint: fingerprint.clone(),
                        },
                        // 이 상태는 blocked_key가 있어 버튼이 disabled라 도달하지 않는다. 안전망으로 일반 Install.
                        AddTrustState::UntrustedNoPubkey { .. } | AddTrustState::SigError(_) => {
                            PluginsAction::Install {
                                src_path: preview.src_path.clone(),
                            }
                        }
                    };
                    actions.push(action);
                    reset_add_state(ui_state);
                }
                if cancel.clicked() {
                    reset_add_state(ui_state);
                }
            });
        },
    );
}

/// 추가할 수 없는 매니페스트의 이유 키. 추가할 수 있으면 `None`.
fn add_blocked_reason_key(preview: &AddPreview) -> Option<&'static str> {
    if preview.already_installed.is_some() {
        return Some("plugins.add_blocked_installed");
    }
    match preview.trust_state {
        AddTrustState::UntrustedNoPubkey { .. } => Some("plugins.add_blocked_no_pubkey"),
        AddTrustState::SigError(_) => Some("plugins.add_blocked_sig_error"),
        AddTrustState::Trusted | AddTrustState::UntrustedWithPubkey { .. } => None,
    }
}

/// `Add Plugin` 탭 하단의 출처 미상 plugin 경고 영역. accent_danger 빨간 박스.
fn draw_untrusted_warning(ui: &mut egui::Ui, preview: &AddPreview, th: &theme::Theme) {
    let red = egui::Color32::from(th.accent_danger());
    match &preview.trust_state {
        AddTrustState::Trusted => {}
        AddTrustState::UntrustedWithPubkey {
            fingerprint,
            reason,
            ..
        } => {
            vspace(ui, th.spacing_md);
            ui.separator();
            vspace(ui, th.spacing_sm);
            let title = match reason {
                AddTrustReason::PermissionsChanged => t("plugins.trust_permissions_changed_title"),
                AddTrustReason::UnknownKey => t("plugins.trust_unknown_title"),
            };
            ui.label(egui::RichText::new(title).strong().color(red));
            ui.label(
                egui::RichText::new(t("plugins.trust_unknown_body"))
                    .color(egui::Color32::from(th.text_primary())),
            );
            fingerprint_line(ui, th, fingerprint);
        }
        AddTrustState::UntrustedNoPubkey {
            fingerprint,
            reason,
        } => {
            vspace(ui, th.spacing_md);
            ui.separator();
            vspace(ui, th.spacing_sm);
            let title = match reason {
                AddTrustReason::PermissionsChanged => t("plugins.trust_permissions_changed_title"),
                AddTrustReason::UnknownKey => t("plugins.trust_unknown_title"),
            };
            ui.label(egui::RichText::new(title).strong().color(red));
            ui.label(egui::RichText::new(t("plugins.trust_no_pubkey")).color(red));
            fingerprint_line(ui, th, fingerprint);
        }
        AddTrustState::SigError(msg) => {
            vspace(ui, th.spacing_md);
            ui.separator();
            vspace(ui, th.spacing_sm);
            ui.label(
                egui::RichText::new(t("plugins.trust_sig_error_title"))
                    .strong()
                    .color(red),
            );
            ui.label(egui::RichText::new(msg).color(red));
        }
    }
}

/// `Add` 탭의 상태를 초기 입력 화면으로 되돌린다.
fn reset_add_state(ui_state: &mut PluginsUiState) {
    ui_state.add_preview = None;
    ui_state.add_error = None;
    ui_state.add_path_input.clear();
}

/// 입력 경로로 매니페스트를 로드하고 preview/에러를 채운다.
fn try_validate_path(ui_state: &mut PluginsUiState, snapshot: &PluginsSnapshot) {
    let raw = ui_state.add_path_input.trim().to_string();
    ui_state.add_error = None;
    ui_state.add_preview = None;
    if raw.is_empty() {
        return;
    }
    let path = std::path::PathBuf::from(&raw);
    match crate::plugin::Manifest::load(&path).and_then(|m| {
        crate::plugin_bridge::manifest_validate::validate_bin_extras(&m)?;
        Ok(m)
    }) {
        Ok(manifest) => {
            let already = snapshot
                .plugins
                .iter()
                .any(|p| p.id == manifest.id)
                .then(|| t_fmt("plugins.add_already_installed", &manifest.id));
            let trust_state = compute_trust_state(&path);
            ui_state.add_preview = Some(AddPreview {
                src_path: path.to_string_lossy().to_string(),
                id: manifest.id.clone(),
                name: manifest.name,
                version: manifest.version,
                description: manifest.description,
                authors: manifest.authors,
                homepage: manifest.homepage,
                surface_kinds: manifest
                    .surface_kinds
                    .iter()
                    .map(|k| k.kind.clone())
                    .collect(),
                permissions: manifest.permissions,
                already_installed: already,
                trust_state,
            });
        }
        Err(e) => {
            ui_state.add_error = Some(t_fmt("plugins.add_invalid_manifest", &e.to_string()));
        }
    }
}

/// 매니페스트 sig 검증 + `.pub` sidecar 조회 결과를 UI 가 분기 가능한 enum 으로
/// 매핑.
fn compute_trust_state(dir: &std::path::Path) -> AddTrustState {
    use tasty_host_plugin::bundle_sig::{
        TrustDecision, UntrustedReason, read_pubkey_sidecar, verify_bundle_signature,
    };
    use tasty_host_plugin::known_plugins::KnownPluginEntry;

    match verify_bundle_signature(dir) {
        Ok(TrustDecision::Trusted) => AddTrustState::Trusted,
        Ok(TrustDecision::Untrusted {
            fingerprint,
            reason,
            ..
        }) => {
            let mapped_reason = match reason {
                UntrustedReason::UnknownKey => AddTrustReason::UnknownKey,
                UntrustedReason::PermissionsChanged => AddTrustReason::PermissionsChanged,
            };
            match read_pubkey_sidecar(dir) {
                Some(pk) => AddTrustState::UntrustedWithPubkey {
                    fingerprint,
                    pubkey_b64: KnownPluginEntry::encode_pubkey(&pk),
                    reason: mapped_reason,
                },
                None => AddTrustState::UntrustedNoPubkey {
                    fingerprint,
                    reason: mapped_reason,
                },
            }
        }
        Err(e) => AddTrustState::SigError(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(trust_state: AddTrustState, already_installed: Option<String>) -> AddPreview {
        AddPreview {
            src_path: "/tmp/p".into(),
            id: "com.example.p".into(),
            name: "p".into(),
            version: "0.1.0".into(),
            description: String::new(),
            authors: Vec::new(),
            homepage: String::new(),
            surface_kinds: Vec::new(),
            permissions: Vec::new(),
            already_installed,
            trust_state,
        }
    }

    #[test]
    fn blocked_reason_names_each_state_that_cannot_be_added() {
        let no_pubkey = AddTrustState::UntrustedNoPubkey {
            fingerprint: "fp".into(),
            reason: AddTrustReason::UnknownKey,
        };
        let with_pubkey = AddTrustState::UntrustedWithPubkey {
            fingerprint: "fp".into(),
            pubkey_b64: "k".into(),
            reason: AddTrustReason::UnknownKey,
        };
        assert_eq!(
            add_blocked_reason_key(&preview(AddTrustState::Trusted, Some("x".into()))),
            Some("plugins.add_blocked_installed")
        );
        // 이미 설치됐다는 이유가 서명 이유보다 먼저다.
        assert_eq!(
            add_blocked_reason_key(&preview(
                AddTrustState::SigError("bad".into()),
                Some("x".into())
            )),
            Some("plugins.add_blocked_installed")
        );
        assert_eq!(
            add_blocked_reason_key(&preview(no_pubkey, None)),
            Some("plugins.add_blocked_no_pubkey")
        );
        assert_eq!(
            add_blocked_reason_key(&preview(AddTrustState::SigError("bad".into()), None)),
            Some("plugins.add_blocked_sig_error")
        );
        assert_eq!(
            add_blocked_reason_key(&preview(AddTrustState::Trusted, None)),
            None
        );
        assert_eq!(add_blocked_reason_key(&preview(with_pubkey, None)), None);
    }
}
