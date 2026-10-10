//! 플러그인 추가 화면 예제. 경로 선택 블록, 안내 상자, 매니페스트 카드, 신뢰 상자, 액션 바를
//! 본체와 같은 공용 view로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    ControlSize, PLUGIN_ADD_INSET, PluginAddBarView, PluginAddPickerView, PluginManifestCardView,
    PluginTrustKind, plugin_add_bar, plugin_add_empty_hint, plugin_add_path_picker,
    plugin_add_read_error, plugin_manifest_card, plugin_trust_box,
};

/// 디자인 `SAMPLE_MANIFEST`. 미신뢰 · 공개키 있음.
const SAMPLE_FINGERPRINT: &str = "9f2c 4ad1 b770 e3a6  ·  ed25519";

/// 16바이트를 넘는 colon-hex fingerprint. 본체가 보이는 SHA-256 전체 표기와 같은 길이다.
const LONG_FINGERPRINT: &str = "1a:2b:3c:4d:5e:6f:70:81:92:a3:b4:c5:d6:e7:f8:09:\
                                 10:21:32:43:54:65:76:87:98:a9:ba:cb:dc:ed:fe:0f";

/// 경로 선택 블록 아래 문단. 본체 `plugins.add_help`의 영어 문구다.
fn help() -> &'static str {
    crate::i18n::t("plugins.add_help")
}

fn sample_strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// 시안 `AddPluginForm` — 경로 선택 블록 아래에 안내 상자 또는 매니페스트 카드와 신뢰 상자,
/// 맨 아래 액션 바. 본체 `draw_add_form`과 같은 공용 view를 쓴다.
pub(super) fn form_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, verified: bool) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let bar_h = ControlSize::Md.height(theme) + theme.spacing_md.value() * 2.0;
    let body = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.max.y - bar_h));
    let inner = body.shrink(theme.spacing_md.value());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_max_width(theme.measure_xl.value().min(inner.width()));
    child.spacing_mut().item_spacing.y = theme.spacing_lg.value();

    let mut path = if verified { "~/dev/tasty-logwatch" } else { "" }.to_owned();
    plugin_add_path_picker(
        &mut child,
        theme,
        &PluginAddPickerView {
            label: "Plugin folder",
            placeholder: "~/dev/my-plugin",
            find: "Find folder…",
            verify: "Verify",
            verify_enabled: verified,
            help: help(),
        },
        &mut path,
    );
    if verified {
        child.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = PLUGIN_ADD_INSET.value();
            manifest_card(ui, theme, false);
            trust_box(ui, theme, PluginTrustKind::UnknownKey, SAMPLE_FINGERPRINT);
        });
    } else {
        plugin_add_empty_hint(
            &mut child,
            theme,
            "Choose a folder and press ",
            "Verify",
            " to read its manifest.",
        );
    }

    let bar = egui::Rect::from_min_max(egui::pos2(rect.min.x, body.max.y), rect.max);
    let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar));
    if verified {
        action_bar(&mut bar_ui, theme, None, false, 3);
    } else {
        plugin_add_bar(
            &mut bar_ui,
            theme,
            &PluginAddBarView {
                left: "",
                cancel: "Cancel",
                add: None,
                add_enabled: false,
            },
        );
    }
}

/// 시안 `SAMPLE_MANIFEST` 카드. `open_values`면 Homepage 링크와 빈 목록(`None`)을 보인다.
fn manifest_card(ui: &mut egui::Ui, theme: &Theme, open_values: bool) {
    let authors = sample_strings(&["aurelia"]);
    let perms = if open_values {
        Vec::new()
    } else {
        sample_strings(&["fs:read", "fs:watch", "ipc:logwatch.*"])
    };
    let surfaces = if open_values {
        Vec::new()
    } else {
        sample_strings(&["logwatch.viewer"])
    };
    plugin_manifest_card(
        ui,
        theme,
        &PluginManifestCardView {
            name: "logwatch",
            version: "0.3.1",
            id: "com.aurelia.logwatch",
            authors: &authors,
            description: "Tails and highlights structured log files as a dedicated surface — severity filters, a jump-to-error gutter, and live follow on the active workspace.",
            permissions_label: "Permissions",
            permissions: &perms,
            surface_kinds_label: "Surface kinds",
            surface_kinds: &surfaces,
            source_label: "Source",
            source: "~/dev/tasty-logwatch",
            homepage_label: "Homepage",
            homepage: if open_values {
                "https://github.com/aurelia/tasty-logwatch"
            } else {
                ""
            },
            none: "None",
        },
    );
}

/// Verify 전 안내 상자(점선), 매니페스트 읽기 오류 상자, 검증 실패 상자. 본체 열 폭 `width` 안에 셋을 쌓는다.
pub(super) fn hint_slot(ui: &mut egui::Ui, theme: &Theme, width: f32) {
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();
        plugin_add_empty_hint(
            ui,
            theme,
            "Choose a folder and press ",
            "Verify",
            " to read its manifest.",
        );
        plugin_add_read_error(
            ui,
            theme,
            crate::i18n::t("plugins.add_read_error"),
            "TOML parse error at line 4, column 9: expected `=`",
        );
        plugin_add_read_error(
            ui,
            theme,
            crate::i18n::t("plugins.add_invalid"),
            "binary \"bin/imgview\" not found in the plugin folder",
        );
    });
}

/// batch 2 회신의 열린 값 — Homepage 링크, 빈 목록 `None`, 긴 fingerprint 의 앞뒤 8바이트.
pub(super) fn open_values(ui: &mut egui::Ui, theme: &Theme, width: f32) {
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = PLUGIN_ADD_INSET.value();
        manifest_card(ui, theme, true);
        trust_box(ui, theme, PluginTrustKind::UnknownKey, LONG_FINGERPRINT);
    });
}

/// 디자인 `TRUST_KIND`의 제목과 본문.
fn trust_copy(kind: PluginTrustKind) -> (&'static str, &'static str) {
    match kind {
        PluginTrustKind::Trusted => ("", crate::i18n::t("plugins.trust_trusted_body")),
        PluginTrustKind::UnknownKey => (
            crate::i18n::t("plugins.trust_unknown_title"),
            crate::i18n::t("plugins.trust_unknown_body"),
        ),
        PluginTrustKind::PermissionsChanged => (
            "Permissions changed",
            crate::i18n::t("plugins.trust_permissions_changed_body"),
        ),
        PluginTrustKind::MissingPubkey => (
            crate::i18n::t("plugins.trust_missing_pubkey_title"),
            crate::i18n::t("plugins.trust_missing_pubkey_body"),
        ),
        PluginTrustKind::SignatureError => (
            crate::i18n::t("plugins.trust_sig_error_title"),
            crate::i18n::t("plugins.trust_sig_error_body"),
        ),
    }
}

fn trust_box(ui: &mut egui::Ui, theme: &Theme, kind: PluginTrustKind, fingerprint: &str) {
    let (title, body) = trust_copy(kind);
    plugin_trust_box(ui, theme, kind, title, body, |ui| {
        super::attention::fingerprint_line(ui, theme, fingerprint);
    });
}

/// 디자인 `grantsLabel`.
fn grants(perms: usize) -> String {
    match perms {
        0 => "No permissions".to_owned(),
        n => crate::i18n::t_count("plugins.add_grants", n as u64, &[&n.to_string()]),
    }
}

/// 프리뷰 하단 액션 바 — 본체 `draw_add_footer`의 버튼 줄.
/// `blocked`가 있으면 추가 버튼을 disabled로 두고 이유를 왼쪽에 적는다.
fn action_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    blocked: Option<&str>,
    trusted: bool,
    perms: usize,
) {
    let grants = grants(perms);
    plugin_add_bar(
        ui,
        theme,
        &PluginAddBarView {
            left: blocked.unwrap_or(&grants),
            cancel: "Cancel",
            add: Some(if blocked.is_none() && !trusted {
                "Trust & add"
            } else {
                "Add plugin"
            }),
            add_enabled: blocked.is_none(),
        },
    );
}

/// 디자인 `AddBarG` 일곱 줄. 추가 가능(신뢰·미신뢰), 막힌 세 이유, 권한 1개·0개.
pub(super) fn add_bars(ui: &mut egui::Ui, theme: &Theme, width: f32) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
        for (blocked, trusted, perms) in [
            (None, true, 3),
            (None, false, 3),
            (Some("Already installed"), true, 3),
            (
                Some(crate::i18n::t("plugins.add_blocked_missing_pubkey")),
                true,
                3,
            ),
            (None, true, 1),
            (None, true, 0),
            (
                Some(crate::i18n::t("plugins.add_blocked_sig_error")),
                true,
                3,
            ),
        ] {
            egui::Frame::new()
                .fill(theme.bg_panel().to_egui())
                .show(ui, |ui| {
                    ui.set_width(width);
                    action_bar(ui, theme, blocked, trusted, perms);
                });
        }
    });
}

/// 디자인 `TrustBoxG` 다섯 가지.
pub(super) fn trust_boxes(ui: &mut egui::Ui, theme: &Theme, width: f32) {
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
        for kind in [
            PluginTrustKind::Trusted,
            PluginTrustKind::UnknownKey,
            PluginTrustKind::PermissionsChanged,
            PluginTrustKind::MissingPubkey,
            PluginTrustKind::SignatureError,
        ] {
            trust_box(ui, theme, kind, SAMPLE_FINGERPRINT);
        }
    });
}
