//! 플러그인 추가의 경로 입력과 매니페스트 확인 예제.
//! 프리뷰는 본체와 같은 공용 매니페스트 카드·신뢰 상자·액션 바를 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, PLUGIN_ADD_INSET, PluginAddBarView, PluginManifestCardView,
    PluginTrustKind, plugin_add_bar, plugin_manifest_card, plugin_trust_box,
};

/// 경로 입력 오른쪽의 Verify 버튼 공간을 확보한다.
fn field_width(theme: &Theme, available: f32) -> f32 {
    (available - theme.field_width_xs.value()).max(theme.field_width_xs.value())
}

/// 경로 입력 상태.
pub(super) fn input_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let inner = rect.shrink(theme.spacing_md.value());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.spacing_mut().item_spacing.y = theme.spacing_sm.value();

    child.label(
        egui::RichText::new("Plugin folder path")
            .size(theme.font_size_body.value())
            .color(theme.text_primary().to_egui()),
    );
    child.horizontal(|ui| {
        let h = theme.item_height_interactive.value();
        let w = field_width(theme, ui.available_width());
        let (r, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        ui.painter().rect(
            r,
            theme.corner_radius.value(),
            theme.surface_raised().to_egui(),
            egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
            egui::StrokeKind::Inside,
        );
        ui.painter().text(
            egui::pos2(r.min.x + theme.spacing_sm.value(), r.center().y),
            egui::Align2::LEFT_CENTER,
            "/path/to/plugin/directory",
            egui::FontId::proportional(theme.font_size_body.value()),
            theme.text_placeholder().to_egui(),
        );
        Button::new("Verify")
            .variant(ButtonVariant::Secondary)
            .show(ui, theme);
    });
    child.separator();
    Button::new("Find plugin folder…")
        .variant(ButtonVariant::Secondary)
        .show(&mut child, theme);
}

/// 디자인 `SAMPLE_MANIFEST`. 미신뢰 · 공개키 있음.
const SAMPLE_FINGERPRINT: &str = "9f2c 4ad1 b770 e3a6  ·  ed25519";

fn sample_strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

/// 매니페스트 프리뷰 상태 (미신뢰 · 공개키 있음). 본체 `draw_add_preview`와 같은 공용 view를 쓴다.
pub(super) fn preview_pane(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter_at(rect)
        .rect_filled(rect, 0.0, theme.bg_panel().to_egui());
    let bar_h = ControlSize::Md.height(theme) + theme.spacing_md.value() * 2.0;
    let body = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, rect.max.y - bar_h));
    let inner = body.shrink(theme.spacing_md.value());
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_max_width(theme.measure_xl.value().min(inner.width()));
    child.spacing_mut().item_spacing.y = PLUGIN_ADD_INSET.value();

    let authors = sample_strings(&["aurelia"]);
    let perms = sample_strings(&["fs:read", "fs:watch", "ipc:logwatch.*"]);
    let surfaces = sample_strings(&["logwatch.viewer"]);
    plugin_manifest_card(
        &mut child,
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
            homepage: "",
            none: "None",
        },
    );
    trust_box(&mut child, theme, PluginTrustKind::UnknownKey);

    let bar = egui::Rect::from_min_max(egui::pos2(rect.min.x, body.max.y), rect.max);
    let mut bar_ui = ui.new_child(egui::UiBuilder::new().max_rect(bar));
    action_bar(&mut bar_ui, theme, None, false, 3);
}

/// 디자인 `TRUST_KIND`의 제목과 본문.
fn trust_copy(kind: PluginTrustKind) -> (&'static str, &'static str) {
    match kind {
        PluginTrustKind::Trusted => (
            "",
            "Signed by a trusted publisher — its key is in your trust store.",
        ),
        PluginTrustKind::UnknownKey => (
            "Unverified publisher",
            "This plugin isn't signed by a key in your trust store. It runs with the permissions above on every launch — review them, and only add plugins from sources you trust. Adding it also trusts this key.",
        ),
        PluginTrustKind::PermissionsChanged => (
            "Permissions changed",
            "This publisher is trusted, but this version asks for permissions the trusted version did not have. Review the list above; adding it trusts the new set.",
        ),
        PluginTrustKind::MissingPubkey => (
            "Public key file missing",
            "The manifest is signed by a key that isn't in your trust store, and tasty-plugin.toml.pub is missing or unreadable, so the key can't be added. Ask the publisher for this public key file.",
        ),
        PluginTrustKind::SignatureError => (
            "Signature check failed",
            "The signature could not be verified. The plugin can't be added until the publisher ships a valid signature.",
        ),
    }
}

fn trust_box(ui: &mut egui::Ui, theme: &Theme, kind: PluginTrustKind) {
    let (title, body) = trust_copy(kind);
    plugin_trust_box(ui, theme, kind, title, body, |ui| {
        super::attention::fingerprint_line(ui, theme, SAMPLE_FINGERPRINT);
    });
}

/// 디자인 `grantsLabel`.
fn grants(perms: usize) -> String {
    match perms {
        0 => "No permissions".to_owned(),
        1 => "Grants 1 permission".to_owned(),
        n => format!("Grants {n} permissions"),
    }
}

/// 프리뷰 하단 액션 바 — 본체 `draw_add_preview`의 버튼 줄.
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
            add: if blocked.is_none() && !trusted {
                "Trust & add"
            } else {
                "Add plugin"
            },
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
                Some("Signed, but the publisher's public key file is missing"),
                true,
                3,
            ),
            (None, true, 1),
            (None, true, 0),
            (Some("Signature check failed"), true, 3),
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
            trust_box(ui, theme, kind);
        }
    });
}
