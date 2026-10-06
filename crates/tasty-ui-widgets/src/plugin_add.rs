//! Plugins 창 Add plugin 프리뷰의 매니페스트 카드, 신뢰 판정 상자, 액션 바.
//! 본체와 갤러리가 함께 호출한다.
//!
//! 이 view는 매니페스트를 읽거나 서명을 검증하지 않는다. 문구와 판정 결과는 호출부가 넘기고
//! 눌린 동작만 돌려준다. fingerprint 줄은 복사 동작을 가진 호출부가 그린다.

use tasty_type_appearance::color::HexColor;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::button::{Button, ButtonVariant};
use crate::chip::{TagVariant, tag};
use crate::control::ControlSize;
use crate::plugin_avatar::{PluginAvatarSize, plugin_avatar};
use crate::tokens::STRUCT_GAP_3;
use crate::tooltip::{Tooltip, tooltip_hover_delay_elapsed};

/// 신뢰 상자와 액션 바의 가로 여백, 카드와 신뢰 상자 사이 간격.
/// 디자인 `plugins_window.jsx`의 `--tasty-size-14`·`gap: 14`이며 역할 토큰이 없다.
pub const PLUGIN_ADD_INSET: LogicalPx = LogicalPx(14.0);

/// 신뢰 판정 다섯 가지. 상자 톤은 추가가 무엇을 하는지를 나타낸다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PluginTrustKind {
    /// 신뢰 목록의 키로 서명됐다. 그대로 추가한다.
    Trusted,
    /// 신뢰 목록에 없는 키다. 추가하면 키를 신뢰한다.
    UnknownKey,
    /// 신뢰한 게시자지만 권한이 늘었다. 추가하면 새 권한 묶음을 신뢰한다.
    PermissionsChanged,
    /// 서명은 있지만 공개 키 파일이 없어 추가할 수 없다.
    MissingPubkey,
    /// 서명을 검증하지 못해 추가할 수 없다. fingerprint 줄이 없다.
    SignatureError,
}

impl PluginTrustKind {
    fn tone(self, theme: &Theme) -> HexColor {
        match self {
            PluginTrustKind::Trusted => theme.accent_success(),
            PluginTrustKind::UnknownKey | PluginTrustKind::PermissionsChanged => {
                theme.accent_warning()
            }
            PluginTrustKind::MissingPubkey | PluginTrustKind::SignatureError => {
                theme.accent_danger()
            }
        }
    }

    fn glyph(self) -> tasty_icons::Icon {
        match self {
            PluginTrustKind::Trusted => tasty_icons::SHIELD_CHECK,
            PluginTrustKind::UnknownKey | PluginTrustKind::PermissionsChanged => {
                tasty_icons::ALERT_TRIANGLE
            }
            PluginTrustKind::MissingPubkey | PluginTrustKind::SignatureError => {
                tasty_icons::ALERT_CIRCLE
            }
        }
    }

    /// 상자 아래에 fingerprint 줄을 두는지. 서명 오류는 식별할 서명이 없다.
    pub fn shows_fingerprint(self) -> bool {
        !matches!(
            self,
            PluginTrustKind::Trusted | PluginTrustKind::SignatureError
        )
    }
}

/// 신뢰 판정 상자. 톤 색의 tint 채움과 테두리를 쓴다.
/// `Trusted`는 글리프와 `body` 한 줄이고 나머지는 제목 줄, 본문, fingerprint 줄 순이다.
/// `fingerprint`는 [`PluginTrustKind::shows_fingerprint`]가 참일 때만 불린다.
pub fn plugin_trust_box(
    ui: &mut egui::Ui,
    theme: &Theme,
    kind: PluginTrustKind,
    title: &str,
    body: &str,
    fingerprint: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let tone = kind.tone(theme).to_egui();
    let glyph = theme.icon_glyph_size_md.value();
    let gap = theme.spacing_sm.value();
    let term_sm = theme.font_size_term_sm.value();
    egui::Frame::new()
        .fill(tone.gamma_multiply(theme.tint_fill_alpha()))
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            tone.gamma_multiply(theme.tint_border_alpha()),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::symmetric(
            PLUGIN_ADD_INSET.value() as i8,
            theme.spacing_md.value() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            if kind == PluginTrustKind::Trusted {
                ui.horizontal(|ui| {
                    glyph_cell(ui, kind, glyph, tone);
                    ui.add(
                        egui::Label::new(egui::RichText::new(body).size(term_sm).color(tone))
                            .wrap(),
                    );
                });
                return;
            }
            // 600 굵기는 굵은 UI 글꼴이 없어 크기와 색으로 근사한다.
            ui.horizontal(|ui| {
                glyph_cell(ui, kind, glyph, tone);
                ui.label(
                    egui::RichText::new(title)
                        .size(theme.font_size_body.value())
                        .color(tone),
                );
            });
            ui.add(
                egui::Label::new(
                    egui::RichText::new(body)
                        .size(term_sm)
                        .line_height(Some(term_sm * theme.line_height_ui))
                        .color(theme.text_secondary().to_egui()),
                )
                .wrap(),
            );
            if kind.shows_fingerprint() {
                fingerprint(ui);
            }
        })
        .response
}

fn glyph_cell(ui: &mut egui::Ui, kind: PluginTrustKind, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    kind.glyph().image(size, color).paint_at(ui, rect);
}

/// 매니페스트 카드의 값과 머리글.
pub struct PluginManifestCardView<'a> {
    pub name: &'a str,
    pub version: &'a str,
    pub id: &'a str,
    pub authors: &'a [String],
    pub description: &'a str,
    pub permissions_label: &'a str,
    pub permissions: &'a [String],
    pub surface_kinds_label: &'a str,
    pub surface_kinds: &'a [String],
    pub source_label: &'a str,
    pub source: &'a str,
    pub homepage_label: &'a str,
    /// 비어 있으면 Homepage 줄을 그리지 않는다.
    pub homepage: &'a str,
    /// 권한·surface 종류가 없을 때의 문구.
    pub none: &'a str,
}

/// 매니페스트 카드에서 눌린 동작.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PluginManifestCardOutput {
    /// Homepage 링크를 눌렀다. 호출부가 기본 브라우저로 연다.
    pub open_homepage: bool,
}

/// 매니페스트 카드. identity 줄(아바타 lg · 이름 + 버전 Tag · id · 작성자), 설명,
/// Permissions·Surface kinds Tag 목록, Source, Homepage를 `space-md` 간격으로 쌓는다.
pub fn plugin_manifest_card(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &PluginManifestCardView<'_>,
) -> PluginManifestCardOutput {
    let mut output = PluginManifestCardOutput::default();
    egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_lg.value() as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = egui::vec2(0.0, theme.spacing_md.value());
            identity(ui, theme, view);
            if !view.description.is_empty() {
                let body = theme.font_size_body.value();
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(view.description)
                            .size(body)
                            .line_height(Some(body * theme.line_height_ui))
                            .color(theme.text_secondary().to_egui()),
                    )
                    .wrap(),
                );
            }
            tag_list(
                ui,
                theme,
                view.permissions_label,
                view.permissions,
                view.none,
            );
            tag_list(
                ui,
                theme,
                view.surface_kinds_label,
                view.surface_kinds,
                view.none,
            );
            mono_field(ui, theme, view.source_label, |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(view.source)
                            .monospace()
                            .size(theme.font_size_term_sm.value())
                            .color(theme.text_secondary().to_egui()),
                    )
                    .truncate(),
                );
            });
            if !view.homepage.is_empty() {
                mono_field(ui, theme, view.homepage_label, |ui| {
                    let resp = ui
                        .add(
                            egui::Label::new(
                                egui::RichText::new(view.homepage)
                                    .monospace()
                                    .size(theme.font_size_caption.value())
                                    .color(theme.text_secondary().to_egui()),
                            )
                            .truncate()
                            .sense(egui::Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    output.open_homepage = resp.clicked();
                });
            }
        });
    output
}

/// 아바타 lg 오른쪽에 이름 + 버전 Tag, 그 아래 `id · 첫 작성자 +N`.
fn identity(ui: &mut egui::Ui, theme: &Theme, view: &PluginManifestCardView<'_>) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
        plugin_avatar(ui, theme, view.name, PluginAvatarSize::Detail);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                // 600 굵기는 크기와 색으로 근사한다.
                ui.label(
                    egui::RichText::new(view.name)
                        .size(theme.font_size_max.value())
                        .color(theme.text_primary().to_egui()),
                );
                tag(
                    ui,
                    theme,
                    &format!("v{}", view.version),
                    TagVariant::Default,
                    false,
                );
            });
            ui.add_space(STRUCT_GAP_3.value());
            let resp = ui.add(
                egui::Label::new(
                    egui::RichText::new(id_line(view.id, view.authors))
                        .monospace()
                        .size(theme.font_size_caption.value())
                        .color(theme.text_muted().to_egui()),
                )
                .truncate()
                .sense(egui::Sense::hover()),
            );
            if view.authors.len() > 1
                && tooltip_hover_delay_elapsed(ui.ctx(), theme, resp.id, resp.hovered())
            {
                Tooltip::new(&view.authors.join(", "))
                    .id_source(resp.id)
                    .show(ui, theme, resp.rect);
            }
        });
    });
}

/// `id · 첫 작성자`에 작성자가 더 있으면 ` +N`을 붙인다. 작성자가 없으면 id만.
fn id_line(id: &str, authors: &[String]) -> String {
    match authors {
        [] => id.to_owned(),
        [first] => format!("{id} · {first}"),
        [first, rest @ ..] => format!("{id} · {first} +{}", rest.len()),
    }
}

/// mono 대문자 머리글. 디자인 `Mono`(micro, text-muted, caps).
fn mono_header(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .monospace()
            .size(theme.font_size_micro.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// 머리글과 값 한 줄을 `space-xs`로 묶는다.
fn mono_field(ui: &mut egui::Ui, theme: &Theme, label: &str, value: impl FnOnce(&mut egui::Ui)) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        mono_header(ui, theme, label);
        value(ui);
    });
}

/// 머리글과 Tag 목록을 `space-sm`으로 묶는다. 목록이 비면 text-muted `none` 문구.
fn tag_list(ui: &mut egui::Ui, theme: &Theme, label: &str, items: &[String], none: &str) {
    let gap = theme.spacing_sm.value();
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = gap;
        mono_header(ui, theme, label);
        if items.is_empty() {
            ui.label(
                egui::RichText::new(none)
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
            );
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            for item in items {
                tag(ui, theme, item, TagVariant::Default, false);
            }
        });
    });
}

/// 액션 바 문구.
pub struct PluginAddBarView<'a> {
    /// 왼쪽 문구. 추가할 수 없으면 이유, 아니면 부여할 권한 수.
    pub left: &'a str,
    pub cancel: &'a str,
    /// `Add plugin` 또는 미신뢰일 때 `Trust & add`.
    pub add: &'a str,
    pub add_enabled: bool,
}

/// 액션 바에서 눌린 버튼.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PluginAddBarClicks {
    pub add: bool,
    pub cancel: bool,
}

/// 위 구분선 아래에 왼쪽 문구, 오른쪽 Cancel(ghost) + 추가(primary)를 둔다.
/// 추가할 수 없으면 추가 버튼을 숨기지 않고 disabled로 둔다.
pub fn plugin_add_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &PluginAddBarView<'_>,
) -> PluginAddBarClicks {
    let mut clicks = PluginAddBarClicks::default();
    let response = egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(
            PLUGIN_ADD_INSET.value() as i8,
            theme.spacing_md.value() as i8,
        ))
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), ControlSize::Md.height(theme)),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    ui.label(
                        egui::RichText::new(view.left)
                            .size(theme.font_size_caption.value())
                            .color(theme.text_muted().to_egui()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        clicks.add = Button::new(view.add)
                            .variant(ButtonVariant::Primary)
                            .enabled(view.add_enabled)
                            .show(ui, theme)
                            .clicked();
                        clicks.cancel = Button::new(view.cancel)
                            .variant(ButtonVariant::Ghost)
                            .show(ui, theme)
                            .clicked();
                    });
                },
            );
        })
        .response;
    let rect = response.rect;
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    clicks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_line_names_the_first_author_and_counts_the_rest() {
        let authors = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(id_line("com.a.b", &[]), "com.a.b");
        assert_eq!(id_line("com.a.b", &authors(&["ann"])), "com.a.b · ann");
        assert_eq!(
            id_line("com.a.b", &authors(&["ann", "bo", "cy"])),
            "com.a.b · ann +2"
        );
    }

    #[test]
    fn only_signature_errors_and_trusted_boxes_have_no_fingerprint() {
        assert!(!PluginTrustKind::Trusted.shows_fingerprint());
        assert!(PluginTrustKind::UnknownKey.shows_fingerprint());
        assert!(PluginTrustKind::PermissionsChanged.shows_fingerprint());
        assert!(PluginTrustKind::MissingPubkey.shows_fingerprint());
        assert!(!PluginTrustKind::SignatureError.shows_fingerprint());
    }
}
