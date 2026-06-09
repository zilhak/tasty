use crate::i18n::{t, t_fmt};
use crate::theme;

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
        ui.add_space(12.0);
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
    ui.add_space(6.0);

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
        ui.add_space(6.0);
        ui.label(egui::RichText::new(err).color(egui::Color32::from(th.red)));
    }

    ui.add_space(20.0);
    ui.separator();
    ui.add_space(12.0);

    if ui.button(t("plugins.add_browse")).clicked() {
        let dialog = rfd::FileDialog::new();
        if let Some(path) = dialog.pick_folder() {
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
    // `take()`는 cancel/add 모두에서 preview를 소비하기 위함이지만, 이 함수가
    // 끝날 때까지 표시할 데이터가 필요하므로 clone 후 다시 넣지 않는다.
    let preview = ui_state.add_preview.clone().expect("checked by caller");

    ui.heading(t("plugins.add_preview_heading"));
    ui.add_space(8.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(ui.available_height() - 60.0)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(&preview.name)
                        .size(16.0)
                        .color(egui::Color32::from(th.text)),
                );
                ui.label(format!("v{}", preview.version));
            });
            ui.label(
                egui::RichText::new(&preview.id)
                    .small()
                    .color(egui::Color32::from(th.subtext0)),
            );
            ui.add_space(8.0);

            if !preview.description.is_empty() {
                ui.label(&preview.description);
                ui.add_space(6.0);
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
            ui.add_space(8.0);

            ui.label(format!(
                "{}: {}",
                t("plugins.add_source_path"),
                preview.src_path
            ));
            ui.add_space(8.0);

            ui.label(format!("{}:", t("plugins.surface_kinds")));
            if preview.surface_kinds.is_empty() {
                ui.label(t("plugins.none"));
            } else {
                ui.label(preview.surface_kinds.join(", "));
            }
            ui.add_space(8.0);

            ui.label(format!("{}:", t("plugins.permissions")));
            if preview.permissions.is_empty() {
                ui.label(t("plugins.none"));
            } else {
                for token in &preview.permissions {
                    ui.label(format!("• {token}"));
                }
            }

            if let Some(msg) = &preview.already_installed {
                ui.add_space(10.0);
                ui.label(egui::RichText::new(msg).color(egui::Color32::from(th.peach)));
            }
        });

    // Untrusted plugin 경고 — 빨간색 영역. 이미 설치된 plugin 은 표시 X
    // (그쪽이 더 의미 있는 메시지).
    if preview.already_installed.is_none() {
        draw_untrusted_warning(ui, &preview, th);
    }

    ui.add_space(10.0);
    ui.separator();
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let can_add = preview.already_installed.is_none()
            && !matches!(
                preview.trust_state,
                AddTrustState::UntrustedNoPubkey { .. } | AddTrustState::SigError(_)
            );
        let add_btn = ui.add_enabled(can_add, egui::Button::new(t("plugins.add_button")));
        if add_btn.clicked() {
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
                // can_add=false 이므로 도달 불가. 안전망으로 일반 Install fallthrough.
                AddTrustState::UntrustedNoPubkey { .. } | AddTrustState::SigError(_) => {
                    PluginsAction::Install {
                        src_path: preview.src_path.clone(),
                    }
                }
            };
            actions.push(action);
            reset_add_state(ui_state);
        }
        if ui.button(t("button.cancel")).clicked() {
            reset_add_state(ui_state);
        }
    });
}

/// `Add Plugin` 탭 하단의 출처 미상 plugin 경고 영역. theme.red 빨간색 박스.
fn draw_untrusted_warning(ui: &mut egui::Ui, preview: &AddPreview, th: &theme::Theme) {
    let red = egui::Color32::from(th.red);
    match &preview.trust_state {
        AddTrustState::Trusted => {}
        AddTrustState::UntrustedWithPubkey {
            fingerprint,
            reason,
            ..
        } => {
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            let title = match reason {
                AddTrustReason::PermissionsChanged => t("plugins.trust_permissions_changed_title"),
                AddTrustReason::UnknownKey => t("plugins.trust_unknown_title"),
            };
            ui.label(egui::RichText::new(title).strong().color(red));
            ui.label(
                egui::RichText::new(t("plugins.trust_unknown_body"))
                    .color(egui::Color32::from(th.text)),
            );
            ui.label(t_fmt("plugins.trust_fingerprint", fingerprint));
        }
        AddTrustState::UntrustedNoPubkey {
            fingerprint,
            reason,
        } => {
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
            let title = match reason {
                AddTrustReason::PermissionsChanged => t("plugins.trust_permissions_changed_title"),
                AddTrustReason::UnknownKey => t("plugins.trust_unknown_title"),
            };
            ui.label(egui::RichText::new(title).strong().color(red));
            ui.label(egui::RichText::new(t("plugins.trust_no_pubkey")).color(red));
            ui.label(t_fmt("plugins.trust_fingerprint", fingerprint));
        }
        AddTrustState::SigError(msg) => {
            ui.add_space(12.0);
            ui.separator();
            ui.add_space(8.0);
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
