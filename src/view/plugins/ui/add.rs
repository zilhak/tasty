use crate::i18n::{t, t_fmt};
use crate::terminal_link;
use crate::theme;

use tasty_ui_widgets::{
    PLUGIN_ADD_INSET, PluginAddBarClicks, PluginAddBarView, PluginAddPickerView,
    PluginManifestCardView, PluginTrustKind, plugin_add_bar, plugin_add_empty_hint,
    plugin_add_path_picker, plugin_add_read_error, plugin_manifest_card, plugin_trust_box, vspace,
};

use super::attention::fingerprint_line;
use super::{
    AddError, AddPreview, AddTrustReason, AddTrustState, PluginsAction, PluginsSnapshot,
    PluginsUiState,
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
        draw_add_form(ui, snapshot, ui_state, actions, &th);
    });
}

/// 경로 선택 블록 아래에 안내 상자 또는 매니페스트 카드와 신뢰 판정 상자를 두고,
/// 맨 아래 액션 바에서 추가하거나 취소한다. 경로를 고치면 확인한 매니페스트를 버린다.
fn draw_add_form(
    ui: &mut egui::Ui,
    snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    actions: &mut Vec<PluginsAction>,
    th: &theme::Theme,
) {
    // 액션 바가 창 안에 남도록, 같은 내용을 보이지 않게 먼저 그려 높이를 잰다.
    let footer_height = {
        let mut probe = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(ui.available_rect_before_wrap())
                .sizing_pass()
                .invisible(),
        );
        draw_add_footer(&mut probe, ui_state.add_preview.as_ref());
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
            ui.spacing_mut().item_spacing.y = th.spacing_lg.value();
            draw_path_picker(ui, snapshot, ui_state, th);
            if let Some(error) = &ui_state.add_error {
                plugin_add_read_error(ui, th, t(error.title_key()), error.reason());
            } else if let Some(preview) = &ui_state.add_preview {
                draw_preview(ui, preview, th);
            } else {
                let template = t("plugins.add_empty_hint");
                let (before, after) = template.split_once("{}").unwrap_or((template, ""));
                plugin_add_empty_hint(ui, th, before, t("plugins.add_confirm_path"), after);
            }
        });

    let preview = ui_state.add_preview.clone();
    let footer = draw_add_footer(ui, preview.as_ref());
    if footer.add
        && let Some(preview) = &preview
    {
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

/// 경로 입력 · 폴더 찾기 · Verify 와 설명 문단.
fn draw_path_picker(
    ui: &mut egui::Ui,
    snapshot: &PluginsSnapshot,
    ui_state: &mut PluginsUiState,
    th: &theme::Theme,
) {
    let picker = plugin_add_path_picker(
        ui,
        th,
        &PluginAddPickerView {
            label: t("plugins.add_path_label"),
            placeholder: t("plugins.add_path_placeholder"),
            find: t("plugins.add_browse"),
            verify: t("plugins.add_confirm_path"),
            verify_enabled: !ui_state.add_path_input.trim().is_empty(),
            help: t("plugins.add_help"),
        },
        &mut ui_state.add_path_input,
    );
    if picker.changed {
        ui_state.add_preview = None;
        ui_state.add_error = None;
    }
    if picker.find {
        let dialog = rfd::FileDialog::new();
        if let Some(path) = crate::stall_watchdog::without_stall_watch(|| dialog.pick_folder()) {
            ui_state.add_path_input = path.to_string_lossy().to_string();
            try_validate_path(ui_state, snapshot);
        }
    }
    if picker.verify {
        try_validate_path(ui_state, snapshot);
    }
}

/// 확인한 매니페스트의 카드와 신뢰 판정 상자.
fn draw_preview(ui: &mut egui::Ui, preview: &AddPreview, th: &theme::Theme) {
    ui.vertical(|ui| {
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
}

/// 하단 액션 바. 높이 측정과 실제 그리기가 같은 함수를 쓴다.
/// 매니페스트가 없으면 Cancel 만 둔다. 추가할 수 없으면 추가 버튼을 disabled로 두고 이유를,
/// 아니면 부여할 권한 수를 왼쪽에 적는다.
fn draw_add_footer(ui: &mut egui::Ui, preview: Option<&AddPreview>) -> PluginAddBarClicks {
    let th = theme::theme();
    let blocked_key = preview.and_then(add_blocked_reason_key);
    let left = match (preview, blocked_key) {
        (None, _) => String::new(),
        (Some(_), Some(key)) => t(key).to_owned(),
        (Some(preview), None) => grants_label(preview.permissions.len()),
    };
    plugin_add_bar(
        ui,
        &th,
        &PluginAddBarView {
            left: &left,
            cancel: t("button.cancel"),
            add: preview.map(|p| t(add_button_key(p, blocked_key))),
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
    // 읽기·파싱 실패와 선언 검사 실패는 상자 제목이 다르다.
    let checked = match crate::plugin::Manifest::read(&path) {
        Err(e) => Err(AddError::Read(e.to_string())),
        Ok(m) => m
            .validate()
            .and_then(|()| crate::plugin_bridge::manifest_validate::validate_bin_extras(&m))
            .map(|()| m)
            .map_err(|e| AddError::Invalid(e.to_string())),
    };
    match checked {
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
        Err(error) => {
            // 오류 원문은 번역하지 않고 상자의 둘째 줄에 그대로 보인다.
            ui_state.add_error = Some(error);
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
    use crate::view::plugins::ui::text_probe::visible_text_rects;

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

    /// 막힌 프리뷰에서도 액션 바(막힌 이유·Cancel·Add plugin)가 창 안에 보이고,
    /// 충분히 높은 창이면 신뢰 상자 제목도 보인다.
    #[test]
    fn action_bar_stays_inside_the_window_below_a_trust_warning() {
        // 다른 시험의 전역 번역 초기화와 경쟁하지 않도록 그리기 전에 초기화한다.
        crate::i18n::init("en");
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

    /// 매니페스트 읽기 실패와 선언 검사 실패는 서로 다른 제목의 오류 상자로 남는다.
    #[test]
    fn validation_failure_is_told_apart_from_a_read_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let validate = |ui_state: &mut PluginsUiState| {
            ui_state.add_path_input = dir.path().to_string_lossy().into_owned();
            try_validate_path(ui_state, &PluginsSnapshot::default());
        };
        let mut ui_state = PluginsUiState::default();

        // 파일이 없으면 읽기 실패다.
        validate(&mut ui_state);
        let missing = ui_state.add_error.clone().expect("missing manifest");
        assert_eq!(missing.title_key(), "plugins.add_read_error");

        // TOML로 읽을 수 없어도 읽기 실패다.
        std::fs::write(dir.path().join("tasty-plugin.toml"), "id = ").expect("write");
        validate(&mut ui_state);
        let broken = ui_state.add_error.clone().expect("broken toml");
        assert_eq!(broken.title_key(), "plugins.add_read_error");

        // 읽었지만 선언 검사에 실패하면 검증 실패이고, 둘째 줄은 검사 메시지다.
        std::fs::write(
            dir.path().join("tasty-plugin.toml"),
            r#"
                manifest_version = 1
                id = "Not A Valid Id"
                name = "X"
                version = "0.1"
                api_version = "1"
                [entry]
                type = "process"
                command = "x"
            "#,
        )
        .expect("write");
        validate(&mut ui_state);
        let invalid = ui_state.add_error.clone().expect("invalid manifest");
        assert_eq!(invalid.title_key(), "plugins.add_invalid");
        assert!(
            invalid.reason().contains("invalid plugin id"),
            "{invalid:?}"
        );
        assert!(ui_state.add_preview.is_none());
    }
}
