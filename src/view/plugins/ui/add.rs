use crate::i18n::{t, t_fmt};
use crate::terminal_link;
use crate::theme;

use tasty_ui_widgets::{
    PLUGIN_ADD_INSET, PluginAddBarClicks, PluginAddBarView, PluginManifestCardView,
    PluginTrustKind, plugin_add_bar, plugin_manifest_card, plugin_trust_box, vspace,
};

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

/// 검증된 매니페스트를 카드와 신뢰 판정 상자로 보여 주고, 아래 액션 바에서 추가하거나 취소한다.
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
    let blocked_key = add_blocked_reason_key(&preview);
    // 액션 바가 창 안에 남도록, 같은 내용을 보이지 않게 먼저 그려 높이를 잰다.
    let footer_height = {
        let mut probe = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(ui.available_rect_before_wrap())
                .sizing_pass()
                .invisible(),
        );
        draw_preview_footer(&mut probe, &preview, blocked_key, th);
        probe.min_rect().height()
    };
    let scroll_height =
        (ui.available_height() - footer_height - ui.spacing().item_spacing.y).max(0.0);
    // 카드와 신뢰 상자가 같은 폭을 쓰도록 열 폭을 스크롤 영역 밖에서 정한다.
    let column_width = th.measure_xl.value().min(ui.available_width());
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .max_height(scroll_height)
        .drag_to_scroll(false)
        .show(ui, |ui| {
            ui.set_width(column_width);
            ui.spacing_mut().item_spacing.y = PLUGIN_ADD_INSET.value();
            let card = plugin_manifest_card(
                ui,
                th,
                &PluginManifestCardView {
                    name: &preview.name,
                    version: &preview.version,
                    id: &preview.id,
                    authors: &preview.authors,
                    description: &preview.description,
                    permissions_label: t("plugins.permissions"),
                    permissions: &preview.permissions,
                    surface_kinds_label: t("plugins.surface_kinds"),
                    surface_kinds: &preview.surface_kinds,
                    source_label: t("plugins.add_source_path"),
                    source: &preview.src_path,
                    homepage_label: t("plugins.homepage"),
                    homepage: &preview.homepage,
                    none: t("plugins.add_none"),
                },
            );
            if card.open_homepage && !terminal_link::open_uri(&preview.homepage) {
                tracing::warn!(homepage = %preview.homepage, "plugin homepage did not open");
            }
            draw_trust_box(ui, &preview.trust_state, th);
        });

    let footer = draw_preview_footer(ui, &preview, blocked_key, th);
    if footer.add {
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
            AddTrustState::UntrustedNoPubkey { .. } | AddTrustState::SigError => {
                PluginsAction::Install {
                    src_path: preview.src_path.clone(),
                }
            }
        };
        actions.push(action);
        reset_add_state(ui_state);
    }
    if footer.cancel {
        reset_add_state(ui_state);
    }
}

/// 프리뷰 하단 액션 바. 높이 측정과 실제 그리기가 같은 함수를 쓴다.
/// 추가할 수 없으면 추가 버튼을 disabled로 두고 이유를, 아니면 부여할 권한 수를 왼쪽에 적는다.
fn draw_preview_footer(
    ui: &mut egui::Ui,
    preview: &AddPreview,
    blocked_key: Option<&'static str>,
    th: &theme::Theme,
) -> PluginAddBarClicks {
    let left = match blocked_key {
        Some(key) => t(key).to_owned(),
        None => grants_label(preview.permissions.len()),
    };
    plugin_add_bar(
        ui,
        th,
        &PluginAddBarView {
            left: &left,
            cancel: t("button.cancel"),
            add: t(add_button_key(preview, blocked_key)),
            add_enabled: blocked_key.is_none(),
        },
    )
}

/// 부여할 권한 수 문구.
fn grants_label(count: usize) -> String {
    match count {
        0 => t("plugins.add_grants_none").to_owned(),
        1 => t("plugins.add_grants_one").to_owned(),
        n => t_fmt("plugins.add_grants_many", &n.to_string()),
    }
}

/// 추가 버튼 키. 추가하면 키나 새 권한 묶음을 신뢰하게 되는 경우만 `Trust & add`다.
fn add_button_key(preview: &AddPreview, blocked_key: Option<&'static str>) -> &'static str {
    match (&preview.trust_state, blocked_key) {
        (AddTrustState::UntrustedWithPubkey { .. }, None) => "plugins.add_trust_and_add",
        _ => "plugins.add_button",
    }
}

/// 추가할 수 없는 매니페스트의 이유 키. 추가할 수 있으면 `None`.
fn add_blocked_reason_key(preview: &AddPreview) -> Option<&'static str> {
    if preview.already_installed {
        return Some("plugins.add_blocked_installed");
    }
    match preview.trust_state {
        AddTrustState::UntrustedNoPubkey { .. } => Some("plugins.add_blocked_missing_pubkey"),
        AddTrustState::SigError => Some("plugins.add_blocked_sig_error"),
        AddTrustState::Trusted | AddTrustState::UntrustedWithPubkey { .. } => None,
    }
}

/// 신뢰 상태의 상자 종류와 fingerprint.
fn trust_kind(state: &AddTrustState) -> (PluginTrustKind, Option<&str>) {
    let untrusted = |reason: &AddTrustReason| match reason {
        AddTrustReason::UnknownKey => PluginTrustKind::UnknownKey,
        AddTrustReason::PermissionsChanged => PluginTrustKind::PermissionsChanged,
    };
    match state {
        AddTrustState::Trusted => (PluginTrustKind::Trusted, None),
        AddTrustState::UntrustedWithPubkey {
            fingerprint,
            reason,
            ..
        } => (untrusted(reason), Some(fingerprint)),
        AddTrustState::UntrustedNoPubkey { fingerprint } => {
            (PluginTrustKind::MissingPubkey, Some(fingerprint))
        }
        AddTrustState::SigError => (PluginTrustKind::SignatureError, None),
    }
}

/// 상자 종류의 제목·본문 키. `Trusted`는 본문 한 줄만 쓴다.
fn trust_copy_keys(kind: PluginTrustKind) -> (&'static str, &'static str) {
    match kind {
        PluginTrustKind::Trusted => ("", "plugins.trust_trusted_body"),
        PluginTrustKind::UnknownKey => {
            ("plugins.trust_unknown_title", "plugins.trust_unknown_body")
        }
        PluginTrustKind::PermissionsChanged => (
            "plugins.trust_permissions_changed_title",
            "plugins.trust_permissions_changed_body",
        ),
        PluginTrustKind::MissingPubkey => (
            "plugins.trust_missing_pubkey_title",
            "plugins.trust_missing_pubkey_body",
        ),
        PluginTrustKind::SignatureError => (
            "plugins.trust_sig_error_title",
            "plugins.trust_sig_error_body",
        ),
    }
}

/// 매니페스트 카드 아래의 신뢰 판정 상자. 이미 설치된 플러그인도 판정대로 그린다.
fn draw_trust_box(ui: &mut egui::Ui, state: &AddTrustState, th: &theme::Theme) {
    let (kind, fingerprint) = trust_kind(state);
    let (title_key, body_key) = trust_copy_keys(kind);
    let title = if title_key.is_empty() {
        ""
    } else {
        t(title_key)
    };
    plugin_trust_box(ui, th, kind, title, t(body_key), |ui| {
        if let Some(fp) = fingerprint {
            fingerprint_line(ui, th, fp);
        }
    });
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
            let already = snapshot.plugins.iter().any(|p| p.id == manifest.id);
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
                None => AddTrustState::UntrustedNoPubkey { fingerprint },
            }
        }
        Err(e) => {
            // 신뢰 상자는 고정 문구만 보이므로 검증 실패의 원인은 로그로 남긴다.
            tracing::warn!(dir = %dir.display(), error = %e, "plugin signature check failed");
            AddTrustState::SigError
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(trust_state: AddTrustState, already_installed: bool) -> AddPreview {
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
        };
        let with_pubkey = AddTrustState::UntrustedWithPubkey {
            fingerprint: "fp".into(),
            pubkey_b64: "k".into(),
            reason: AddTrustReason::UnknownKey,
        };
        assert_eq!(
            add_blocked_reason_key(&preview(AddTrustState::Trusted, true)),
            Some("plugins.add_blocked_installed")
        );
        // 이미 설치됐다는 이유가 서명 이유보다 먼저다.
        assert_eq!(
            add_blocked_reason_key(&preview(AddTrustState::SigError, true)),
            Some("plugins.add_blocked_installed")
        );
        assert_eq!(
            add_blocked_reason_key(&preview(no_pubkey, false)),
            Some("plugins.add_blocked_missing_pubkey")
        );
        assert_eq!(
            add_blocked_reason_key(&preview(AddTrustState::SigError, false)),
            Some("plugins.add_blocked_sig_error")
        );
        assert_eq!(
            add_blocked_reason_key(&preview(AddTrustState::Trusted, false)),
            None
        );
        assert_eq!(add_blocked_reason_key(&preview(with_pubkey, false)), None);
    }

    /// 창 안에 그려진 글자 사각형들. 클립 밖으로 나간 글자는 사용자에게 보이지 않으므로 뺀다.
    fn visible_text_rects(
        output: &egui::FullOutput,
        screen: egui::Rect,
    ) -> Vec<(String, egui::Rect)> {
        fn walk(
            shape: &egui::Shape,
            clip: egui::Rect,
            screen: egui::Rect,
            out: &mut Vec<(String, egui::Rect)>,
        ) {
            match shape {
                egui::Shape::Vec(shapes) => {
                    for s in shapes {
                        walk(s, clip, screen, out);
                    }
                }
                egui::Shape::Text(text) => {
                    let rect = text.galley.rect.translate(text.pos.to_vec2());
                    if clip.contains_rect(rect) && screen.contains_rect(rect) {
                        out.push((text.galley.text().to_string(), rect));
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for clipped in &output.shapes {
            walk(&clipped.shape, clipped.clip_rect, screen, &mut out);
        }
        out
    }

    /// 막힌 프리뷰에서도 액션 바(막힌 이유·Cancel·Add plugin)가 창 안에 보이고,
    /// 충분히 높은 창이면 신뢰 상자 제목도 보인다.
    #[test]
    fn action_bar_stays_inside_the_window_below_a_trust_warning() {
        let states = [
            AddTrustState::UntrustedNoPubkey {
                fingerprint: "16:43:83:e3:a7:6d:5c:20".into(),
            },
            AddTrustState::SigError,
        ];
        for trust_state in states {
            let reason = t(add_blocked_reason_key(&preview(trust_state.clone(), false))
                .expect("blocked state"))
            .to_string();
            let wanted = [
                reason,
                t("button.cancel").to_string(),
                t("plugins.add_button").to_string(),
            ];
            let trust_title = t(trust_copy_keys(trust_kind(&trust_state).0).0).to_string();
            for height in [300.0, 560.0, 760.0] {
                let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(880.0, height));
                let mut ui_state = PluginsUiState {
                    add_preview: Some(preview(trust_state.clone(), false)),
                    ..Default::default()
                };
                let mut actions = Vec::new();
                let ctx = egui::Context::default();
                let raw = egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                };
                let output = ctx.run(raw, |ctx| {
                    draw_add_tab(
                        ctx,
                        &PluginsSnapshot::default(),
                        &mut ui_state,
                        &mut actions,
                    );
                });
                let texts = visible_text_rects(&output, screen);
                // 낮은 창에서는 카드와 신뢰 상자가 스크롤 영역 아래로 밀려도 된다.
                let shown = wanted
                    .iter()
                    .chain((height >= 560.0).then_some(&trust_title));
                for label in shown {
                    assert!(
                        texts.iter().any(|(text, _)| text == label),
                        "{trust_state:?} at height {height}: {label:?} is not visible inside the window"
                    );
                }
            }
        }
    }
}
