use std::collections::HashMap;

use crate::i18n::t;
use crate::settings::{
    ActiveTabIndicator, EffectiveFont, FontOverride, FontSettings, HexColor, Settings,
};
use tasty_host_plugin::SettingsPageEntry;
use tasty_plugin_manifest::{SettingsItemDecl, SettingsPageContribute};
use tasty_type_appearance::theme::{
    FALLBACK_SURFACE, PartialColors, PartialSurfaceTheme, SurfaceTheme, Theme, ThemeColors,
};
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{HelpHint, TooltipPlacement, vspace};

/// Plugin sub-tab 식별: `(plugin_id, page_id)` 복합키로 일치하는 entry 를 찾는다.
///
/// `page_id` 단독 매칭은 서로 다른 plugin 이 동일 id 를 contribute 할 경우 첫
/// 매칭이 반환되어 다른 plugin 의 콘텐츠가 잘못 렌더된다. 전역 식별자는
/// `<plugin_id>/<page_id>` (manifest types 460-462 참고) 이므로 이 헬퍼를 통한
/// 복합키 매칭이 정답.
pub(super) fn find_plugin_settings_entry<'a>(
    entries: &'a [SettingsPageEntry],
    plugin_id: &str,
    page_id: &str,
) -> Option<&'a SettingsPageEntry> {
    entries
        .iter()
        .find(|e| e.plugin_id == plugin_id && e.page.id == page_id)
}

/// Draw a label followed by a HelpHint (?) glyph with tooltip. For use inside Grid rows.
fn label_with_tooltip(ui: &mut egui::Ui, label: &str, tooltip: &str) {
    let th = crate::theme::theme();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
        ui.label(label);
        HelpHint::new(tooltip)
            .placement(TooltipPlacement::Bottom)
            .show(ui, &th);
    });
}

/// Appearance 탭 콘텐츠. L2 사이드바(고정 6 섹션 + Appearance plugin page 합성·
/// 필터·선택·fallback)는 settings 셸이 소유하므로 여기서는 활성 `sub_tab` 의
/// 콘텐츠만 그린다.
pub fn draw_appearance_tab(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    sub_tab: &crate::settings_ui::AppearanceSubTab,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
    settings_pages: &[SettingsPageEntry],
) {
    use crate::settings_ui::AppearanceSubTab;
    match sub_tab {
        AppearanceSubTab::Theme => {
            draw_appearance_theme(ui, settings);
        }
        AppearanceSubTab::Colors => {
            draw_appearance_colors(ui, settings);
        }
        AppearanceSubTab::General => {
            draw_appearance_general(
                ui,
                settings,
                font_families,
                font_filter,
                preview_font_loaded,
            );
        }
        AppearanceSubTab::Display => {
            draw_appearance_display(ui, settings);
        }
        AppearanceSubTab::Tasty => {
            draw_appearance_tasty(ui, settings);
        }
        AppearanceSubTab::Terminal => {
            draw_appearance_terminal(
                ui,
                settings,
                font_families,
                font_filter,
                preview_font_loaded,
            );
        }
        AppearanceSubTab::Explorer => {
            draw_appearance_explorer(
                ui,
                settings,
                font_families,
                font_filter,
                preview_font_loaded,
            );
        }
        AppearanceSubTab::Plugin { plugin_id, page_id } => {
            if let Some(entry) = find_plugin_settings_entry(settings_pages, plugin_id, page_id) {
                draw_plugin_settings_page(
                    ui,
                    settings,
                    font_families,
                    font_filter,
                    preview_font_loaded,
                    plugin_id,
                    &entry.page,
                );
            }
        }
    }
}

/// Appearance > Theme: preset selection + note. Font settings live in
/// Appearance › General.
fn draw_appearance_theme(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    ui.label(
        egui::RichText::new(t("settings.appearance.theme.heading"))
            .strong()
            .color(th.text_primary()),
    );
    vspace(ui, th.spacing_sm);

    // ~/.tasty/themes/ 의 디스크 변경을 settings 화면 진입 시 한 번 더 반영.
    if let Err(e) = tasty_themes::rescan() {
        tracing::warn!("themes rescan failed: {e}");
    }
    let entries = tasty_themes::scan_themes();
    for entry in &entries {
        let is_current = settings.appearance.theme == entry.id;
        let clicked = draw_theme_swatch(ui, &th, &entry.label, &entry.file, is_current);
        if clicked && !is_current {
            // 테마 변경 = base 누적 + overrides 클리어. 적용은 settings 저장 후
            // (modal::on_save) GPU bridge 가 install_global 로 반영.
            tasty_themes::apply_theme(&mut settings.appearance, &entry.id);
        }
        ui.add_space(th.spacing_xs.value());
    }

    vspace(ui, th.spacing_sm);
    ui.label(
        egui::RichText::new(t("settings.appearance.theme.hint"))
            .small()
            .color(th.text_muted()),
    );
}

/// 한 테마의 대표 5색 `[bg, surface, accent, accent_alt, success]` 을 `ThemeFile`
/// 에서 파생한다. 디자인 `ThemeSwatch` 매핑:
/// bg=`palette.crust`, surface=`palette.base`, accent=`accent.blue`,
/// accent_alt=`accent.mauve`, success=`accent.green`.
///
/// 각 필드는 테마가 일부만 지정할 수 있어 `Option` 이며, `None` 이면 빌트인 mocha
/// fallback 색으로 대체한다(전역 적용 시 실제로 보이는 색과 일치).
fn representative_swatch(file: &tasty_themes::ThemeFile) -> [HexColor; 5] {
    let base = tasty_themes::mocha_fallback_colors();
    [
        file.palette.crust.unwrap_or(base.crust),
        file.palette.base.unwrap_or(base.base),
        file.accent.blue.unwrap_or(base.blue),
        file.accent.mauve.unwrap_or(base.mauve),
        file.accent.green.unwrap_or(base.green),
    ]
}

/// 프리셋 한 개를 5색 스와치 카드(스트라이프 38px + 라벨)로 그린다. 활성(현재
/// 선택) 시 accent 보더 + 1px ring 으로 강조. 반환값은 클릭 여부.
///
/// 색·보더·간격은 모두 `Theme` 토큰 경유(하드코딩 금지). 스와치 색은 `entry.file`
/// 에서 파생([`representative_swatch`]).
fn draw_theme_swatch(
    ui: &mut egui::Ui,
    th: &Theme,
    label: &str,
    file: &tasty_themes::ThemeFile,
    is_current: bool,
) -> bool {
    let swatch = representative_swatch(file);

    let pad = th.spacing_sm.value();
    let stripe_h = THEME_SWATCH_STRIPE_HEIGHT.value();
    // 라벨 줄 여백. 이 카드 높이 식은 `pad`(spacing_sm) · `gap`(spacing_xs) ·
    // `font_size_body` 를 더하는데 셋 다 배율을 탄다 — 이 항만 평상수면 1.2 에서
    // 글자는 커지고 여백은 그대로라 라벨이 카드에 낀다.
    let label_h = th.font_size_body.value() + th.spacing_xs.value();
    let gap = th.spacing_xs.value();
    let card_w = ui.available_width();
    let card_h = pad + stripe_h + gap + label_h + pad;

    let (rect, resp) = ui.allocate_exact_size(egui::vec2(card_w, card_h), egui::Sense::click());
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    let painter = ui.painter();
    let corner = th.corner_radius.value();

    // 카드 배경 (surface-raised).
    painter.rect_filled(rect, corner, th.surface_raised().to_egui());

    // 5색 스트라이프 — 동일 폭 5분할.
    let stripe_rect = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + pad, rect.min.y + pad),
        egui::vec2(card_w - pad * 2.0, stripe_h),
    );
    let seg_w = stripe_rect.width() / swatch.len() as f32;
    for (i, color) in swatch.iter().enumerate() {
        let seg = egui::Rect::from_min_size(
            egui::pos2(stripe_rect.min.x + seg_w * i as f32, stripe_rect.min.y),
            egui::vec2(seg_w, stripe_h),
        );
        painter.rect_filled(seg, 0.0, color.to_egui());
    }

    // 라벨 (스트라이프 하단).
    painter.text(
        egui::pos2(rect.min.x + pad, stripe_rect.max.y + gap),
        egui::Align2::LEFT_TOP,
        label,
        egui::FontId::proportional(th.font_size_body.value()),
        th.text_primary().to_egui(),
    );

    // 보더 — 활성: accent 보더 + 1px inner ring. 비활성: border-default(hover 시 강조).
    if is_current {
        painter.rect_stroke(
            rect,
            corner,
            egui::Stroke::new(th.border_width.value(), th.accent_primary().to_egui()),
            egui::StrokeKind::Inside,
        );
        let inner = rect.shrink(th.spacing_xs.value() / 2.0);
        painter.rect_stroke(
            inner,
            (corner - 1.0).max(0.0),
            egui::Stroke::new(th.border_width.value(), th.accent_primary().to_egui()),
            egui::StrokeKind::Inside,
        );
    } else {
        let border = if resp.hovered() {
            th.border_strong().to_egui()
        } else {
            th.border_default().to_egui()
        };
        painter.rect_stroke(
            rect,
            corner,
            egui::Stroke::new(th.border_width.value(), border),
            egui::StrokeKind::Inside,
        );
    }

    resp.clicked()
}

/// Convert FontSettings → EffectiveFont (used for default-font preview).
fn effective_from_settings(s: &FontSettings) -> EffectiveFont {
    EffectiveFont {
        font_family: s.font_family.clone(),
        font_size: s.font_size,
        custom_font_path: s.custom_font_path.clone(),
        line_height: s.line_height,
        font_scale_mode: s.font_scale_mode.clone(),
    }
}

/// Appearance > General: default font settings (single source of truth for
/// fields not overridden per-surface) + background opacity.
fn draw_appearance_general(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    ui.label(
        egui::RichText::new(t("settings.appearance.font.default_heading"))
            .strong()
            .color(th.text_primary()),
    );
    vspace(ui, th.spacing_xs);
    ui.label(
        egui::RichText::new(t("settings.appearance.font.default_hint"))
            .small()
            .color(th.text_muted()),
    );
    vspace(ui, th.spacing_sm);

    ui.columns(2, |columns| {
        font_settings_grid(
            &mut columns[0],
            &mut settings.appearance.default_font,
            font_families,
            font_filter,
            "default",
        );
        let preview_eff = effective_from_settings(&settings.appearance.default_font);
        let preview_colors = crate::theme::theme().surface("terminal").clone();
        draw_font_preview(
            &mut columns[1],
            &preview_eff,
            &preview_colors,
            &settings.appearance,
            "default",
            preview_font_loaded,
        );
    });

    vspace(ui, th.spacing_lg);
    ui.separator();
    vspace(ui, th.spacing_sm);

    egui::Grid::new("appearance_general_grid")
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            // 디자인 settings_window.jsx:225 — Ligatures Switch 행.
            ui.label(t("settings.appearance.ligatures_label"));
            tasty_ui_widgets::switch(ui, &th, &mut settings.appearance.ligatures, None, true);
            ui.end_row();

            ui.label(t("settings.appearance.background_opacity_label"));
            ui.add(egui::Slider::new(
                &mut settings.appearance.background_opacity,
                0.0..=1.0,
            ));
            ui.end_row();
        });
}

/// UI-scale 토글 카드 치수 (디자인 jsx: Display scale cards). 새 컴포넌트라
/// 파일 내 색-picker 상수와 동일하게 LogicalPx 로 둔다.
const DISPLAY_CARD_WIDTH: LogicalPx = LogicalPx(96.0);
const DISPLAY_CARD_HEIGHT: LogicalPx = LogicalPx(76.0);

/// Appearance > Display: UI scale (sm/md/lg) 토글 카드. 위젯만 교체 —
/// 적용 범위(전역 egui zoom)는 불변. 각 카드는 실제 배율로 렌더된 "Aa"
/// 프리뷰 + 라벨을 표시하고, 활성 카드는 accent 보더 + ring 으로 강조한다.
fn draw_appearance_display(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);
    ui.label(t("settings.appearance.ui_scale_label"));
    vspace(ui, th.spacing_sm);

    // (scale_key, label_key) — 순서대로 가로 배치.
    let cards = [
        ("small", "settings.appearance.ui_scale_small"),
        ("medium", "settings.appearance.ui_scale_medium"),
        ("large", "settings.appearance.ui_scale_large"),
    ];

    let mut selected: Option<&str> = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        for (scale_key, label_key) in cards {
            if display_scale_card(ui, &th, settings, scale_key, label_key) {
                selected = Some(scale_key);
            }
        }
    });
    if let Some(scale_key) = selected {
        settings.appearance.ui_scale = scale_key.to_string();
    }
}

/// 한 UI-scale 토글 카드 — surface-raised 배경 + 실제 배율의 "Aa" 프리뷰 +
/// 라벨. 활성 카드는 accent 보더 + 1px ring. 클릭되면 `true`.
fn display_scale_card(
    ui: &mut egui::Ui,
    th: &Theme,
    settings: &Settings,
    scale_key: &str,
    label_key: &str,
) -> bool {
    let is_active = settings.appearance.ui_scale == scale_key;
    let factor = crate::settings::AppearanceSettings::ui_scale_factor_for(scale_key);

    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(DISPLAY_CARD_WIDTH.value(), DISPLAY_CARD_HEIGHT.value()),
        egui::Sense::click(),
    );

    // 배경 (surface-raised).
    let radius = th.corner_radius.value();
    ui.painter()
        .rect_filled(rect, radius, egui::Color32::from(th.surface_raised()));

    // 보더 / ring.
    if is_active {
        // accent 보더 (내측 1px) + 바깥 1px accent ring.
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(
                th.border_width.value(),
                egui::Color32::from(th.border_focus()),
            ),
            egui::StrokeKind::Inside,
        );
        ui.painter().rect_stroke(
            rect.expand(th.border_width.value()),
            radius,
            egui::Stroke::new(
                th.border_width.value(),
                egui::Color32::from(th.border_focus()),
            ),
            egui::StrokeKind::Outside,
        );
    } else {
        ui.painter().rect_stroke(
            rect,
            radius,
            // 비활성 카드 보더(값-동일: surface2). border-role 접근자 부재 → surface_active() 로 값 보존
            egui::Stroke::new(
                th.border_width.value(),
                egui::Color32::from(th.surface_active()),
            ),
            egui::StrokeKind::Inside,
        );
    }

    let text_color = egui::Color32::from(if is_active {
        th.text_primary()
    } else {
        th.text_muted()
    });

    // "Aa" 프리뷰 — 실제 배율로 스케일된 글리프 샘플 (자연어 아님 → i18n 예외).
    let preview_size = th.font_size_heading.value() * factor;
    ui.painter().text(
        egui::pos2(
            rect.center().x,
            rect.min.y + DISPLAY_CARD_HEIGHT.value() * 0.4,
        ),
        egui::Align2::CENTER_CENTER,
        "Aa",
        egui::FontId::proportional(preview_size),
        text_color,
    );

    // 라벨 (Small / Medium / Large).
    ui.painter().text(
        egui::pos2(
            rect.center().x,
            rect.max.y - DISPLAY_CARD_HEIGHT.value() * 0.2,
        ),
        egui::Align2::CENTER_CENTER,
        t(label_key),
        egui::FontId::proportional(th.font_size_body.value()),
        text_color,
    );

    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    resp.clicked()
}

/// Reset the Tasty curated overrides (accent / sidebar bg) + active tab
/// indicator back to the current theme's defaults. Pure (no egui) so the reset
/// semantics can be unit-tested.
fn reset_tasty_to_theme_defaults(app: &mut crate::settings::AppearanceSettings) {
    app.theme_overrides.blue = None;
    app.theme_overrides.mantle = None;
    app.active_tab_indicator = ActiveTabIndicator::default();
}

/// Appearance > Tasty: app-chrome theming. Accent / Sidebar background are
/// curated shortcuts into `theme_overrides` (`blue` / `mantle`) — the same
/// single source the Colors picker edits. Active tab indicator selects the
/// `active_tab_indicator` style. "Use theme defaults" clears the curated
/// overrides and resets the indicator.
fn draw_appearance_tasty(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    ui.label(
        egui::RichText::new(t("settings.appearance.tasty.heading"))
            .strong()
            .color(th.text_primary()),
    );
    vspace(ui, th.spacing_xs);
    ui.label(
        egui::RichText::new(t("settings.appearance.tasty.hint"))
            .small()
            .color(th.text_muted()),
    );
    vspace(ui, th.spacing_sm);

    // ── Accent → theme_overrides.blue ──
    draw_tasty_color_row(
        ui,
        &th,
        settings,
        "settings.appearance.tasty.accent",
        |c| c.blue,
        |p| p.blue,
        |p, v| p.blue = v,
    );
    // ── Sidebar background → theme_overrides.mantle ──
    draw_tasty_color_row(
        ui,
        &th,
        settings,
        "settings.appearance.tasty.sidebar_bg",
        |c| c.mantle,
        |p| p.mantle,
        |p, v| p.mantle = v,
    );

    vspace(ui, th.spacing_md);

    // ── Active tab indicator (Underline / Fill / Dot) ──
    ui.label(
        egui::RichText::new(t("settings.appearance.tasty.indicator_label"))
            .color(th.text_primary()),
    );
    vspace(ui, th.spacing_xs);
    ui.horizontal(|ui| {
        for (variant, key) in [
            (
                ActiveTabIndicator::Underline,
                "settings.appearance.tasty.indicator_underline",
            ),
            (
                ActiveTabIndicator::Fill,
                "settings.appearance.tasty.indicator_fill",
            ),
            (
                ActiveTabIndicator::Dot,
                "settings.appearance.tasty.indicator_dot",
            ),
        ] {
            ui.selectable_value(
                &mut settings.appearance.active_tab_indicator,
                variant,
                t(key),
            );
        }
    });

    vspace(ui, th.spacing_md);

    // ── Use theme defaults ──
    if ui
        .button(t("settings.appearance.tasty.use_defaults"))
        .clicked()
    {
        reset_tasty_to_theme_defaults(&mut settings.appearance);
    }
}

/// A single Tasty curated color row — friendly label (i18n), hex input, swatch,
/// and a "Default" checkbox. `base`/`get`/`set` point at one field of
/// `theme_base`/`theme_overrides`, giving identical set/clear semantics to the
/// Colors picker's [`draw_color_picker_row`] but with a friendly label instead
/// of the raw mono token name.
fn draw_tasty_color_row(
    ui: &mut egui::Ui,
    th: &Theme,
    settings: &mut Settings,
    label_key: &str,
    base: fn(&ThemeColors) -> HexColor,
    get: fn(&PartialColors) -> Option<HexColor>,
    set: fn(&mut PartialColors, Option<HexColor>),
) {
    let base_color = base(&settings.appearance.theme_base);
    let cur = get(&settings.appearance.theme_overrides);
    let is_ov = cur.is_some();
    let val = cur.unwrap_or(base_color);
    let buf_id = ui.make_persistent_id(("appearance_tasty_hex", label_key));

    ui.horizontal(|ui| {
        // ── override dot ──
        let dot = COLOR_OVERRIDE_DOT_SIZE.value();
        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(dot, dot), egui::Sense::hover());
        if is_ov {
            ui.painter().circle_filled(
                dot_rect.center(),
                dot / 2.0,
                egui::Color32::from(th.accent_primary()),
            );
        }

        // ── friendly label ──
        ui.add_sized(
            egui::vec2(COLOR_FIELD_NAME_WIDTH.value(), COLOR_SWATCH_SIZE.value()),
            egui::Label::new(egui::RichText::new(t(label_key)).color(if is_ov {
                th.text_primary()
            } else {
                th.text_muted()
            })),
        );

        // ── hex 입력 ──
        if is_ov {
            let mut text = ui
                .data(|d| d.get_temp::<String>(buf_id))
                .unwrap_or_else(|| val.to_hex());
            let resp = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(COLOR_HEX_INPUT_WIDTH.value()),
            );
            if resp.changed()
                && let Some(parsed) = HexColor::from_hex(text.trim())
            {
                set(&mut settings.appearance.theme_overrides, Some(parsed));
            }
            ui.data_mut(|d| d.insert_temp(buf_id, text));
        } else {
            let mut text = base_color.to_hex();
            ui.add_enabled(
                false,
                egui::TextEdit::singleline(&mut text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(COLOR_HEX_INPUT_WIDTH.value()),
            );
        }

        // ── swatch (override 시 full, base 시 40% dim) ──
        let sw = COLOR_SWATCH_SIZE.value();
        let (sw_rect, _) = ui.allocate_exact_size(egui::vec2(sw, sw), egui::Sense::hover());
        let fill = if is_ov {
            egui::Color32::from(val)
        } else {
            val.with_alpha(SWATCH_INHERITED_ALPHA).to_egui()
        };
        ui.painter().rect_filled(sw_rect, 3.0, fill);
        ui.painter().rect_stroke(
            sw_rect,
            3.0,
            egui::Stroke::new(
                th.border_width.value(),
                egui::Color32::from(th.border_strong()),
            ),
            egui::StrokeKind::Inside,
        );

        // ── "Default" 체크박스 (디자인 settings_window.jsx:113 = <Checkbox>) ──
        let mut use_default = !is_ov;
        let default_label = t("settings.appearance.colors.default");
        if tasty_ui_widgets::checkbox(ui, th, &mut use_default, default_label, true).changed() {
            let ov = &mut settings.appearance.theme_overrides;
            if use_default {
                set(ov, None);
            } else {
                set(ov, Some(base_color));
                let seed = base_color.to_hex();
                ui.data_mut(|d| d.insert_temp(buf_id, seed));
            }
        }
    });
}

/// Appearance > Terminal: font override + preview + colors
fn draw_appearance_terminal(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
) {
    draw_surface_font_section(
        ui,
        settings,
        font_families,
        font_filter,
        preview_font_loaded,
        SurfaceFontTarget::Terminal,
    );

    draw_terminal_surface_colors(ui, settings);
}

/// Appearance › Explorer: 내장 파일 관리자 surface 의 폰트 override (T11). 저장
/// 슬롯은 `appearance.plugin_font_overrides["explorer"]` — `effective_font_for_kind
/// ("explorer")` 가 읽어 explorer surface 렌더에 적용된다.
fn draw_appearance_explorer(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
) {
    draw_surface_font_section(
        ui,
        settings,
        font_families,
        font_filter,
        preview_font_loaded,
        SurfaceFontTarget::Plugin {
            storage_key: "explorer",
        },
    );
}

/// surface kind id for the terminal — the `theme_overrides.surface_themes` /
/// `theme_base.surface_themes` map key the background pickers bind to.
const TERMINAL_SURFACE_ID: &str = "terminal";

/// Which terminal surface background a picker row binds to (Focused/Unfocused).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SurfaceBgField {
    Focused,
    Unfocused,
}

impl SurfaceBgField {
    /// Stable, language-independent key for the per-row hex edit buffer id.
    fn key(self) -> &'static str {
        match self {
            SurfaceBgField::Focused => "surface_focused_bg",
            SurfaceBgField::Unfocused => "surface_unfocused_bg",
        }
    }
    fn get(self, p: &PartialSurfaceTheme) -> Option<HexColor> {
        match self {
            SurfaceBgField::Focused => p.focused_bg,
            SurfaceBgField::Unfocused => p.unfocused_bg,
        }
    }
    fn set(self, p: &mut PartialSurfaceTheme, v: Option<HexColor>) {
        match self {
            SurfaceBgField::Focused => p.focused_bg = v,
            SurfaceBgField::Unfocused => p.unfocused_bg = v,
        }
    }
    fn base(self, st: &SurfaceTheme) -> HexColor {
        match self {
            SurfaceBgField::Focused => st.focused_bg,
            SurfaceBgField::Unfocused => st.unfocused_bg,
        }
    }
}

/// Current terminal surface bg override for `field` (`None` = follow base).
/// Pure (no egui) so set/clear/resolve can be unit-tested.
fn surface_bg_override(ov: &PartialColors, field: SurfaceBgField) -> Option<HexColor> {
    ov.surface_themes
        .get(TERMINAL_SURFACE_ID)
        .and_then(|p| field.get(p))
}

/// Set (`Some`) or clear (`None`) the terminal surface bg override for `field`.
/// Clearing prunes the map entry once its `PartialSurfaceTheme` is empty so the
/// override map doesn't accumulate `{"terminal": {}}` husks.
fn set_surface_bg_override(ov: &mut PartialColors, field: SurfaceBgField, val: Option<HexColor>) {
    match val {
        Some(c) => {
            let entry = ov
                .surface_themes
                .entry(TERMINAL_SURFACE_ID.to_string())
                .or_default();
            field.set(entry, Some(c));
        }
        None => {
            if let Some(p) = ov.surface_themes.get_mut(TERMINAL_SURFACE_ID) {
                field.set(p, None);
                if p.is_empty() {
                    ov.surface_themes.remove(TERMINAL_SURFACE_ID);
                }
            }
        }
    }
}

/// Base (preset) terminal surface bg for `field`, read from resolved
/// `theme_base` (no hardcoding). Falls back to `FALLBACK_SURFACE` if the base
/// has no `terminal` entry.
fn surface_bg_base(base: &ThemeColors, field: SurfaceBgField) -> HexColor {
    base.surface_themes
        .get(TERMINAL_SURFACE_ID)
        .map(|st| field.base(st))
        .unwrap_or_else(|| field.base(&FALLBACK_SURFACE))
}

/// Appearance › Terminal: surface background override pickers (Focused /
/// Unfocused). Writes into `theme_overrides.surface_themes["terminal"]`, the
/// same override layer the Colors picker edits (resolve via `apply_partial`).
fn draw_terminal_surface_colors(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);
    ui.label(
        egui::RichText::new(t("settings.appearance.terminal.surface_heading"))
            .strong()
            .color(th.text_primary()),
    );
    vspace(ui, th.spacing_xs);
    ui.label(
        egui::RichText::new(t("settings.appearance.terminal.surface_hint"))
            .small()
            .color(th.text_muted()),
    );
    vspace(ui, th.spacing_sm);
    draw_surface_bg_row(
        ui,
        &th,
        settings,
        t("settings.appearance.terminal.focused_bg"),
        SurfaceBgField::Focused,
    );
    draw_surface_bg_row(
        ui,
        &th,
        settings,
        t("settings.appearance.terminal.unfocused_bg"),
        SurfaceBgField::Unfocused,
    );
}

/// One terminal surface bg row — mirrors `draw_color_picker_row`'s visual
/// ([override dot] label · hex input · swatch · "Use default" checkbox) but
/// binds to the nested `surface_themes["terminal"]` override instead of a flat
/// `PartialColors` field. "Use default" checked = base-following (input
/// disabled, swatch dim) / unchecked = `Some(hex)` override.
fn draw_surface_bg_row(
    ui: &mut egui::Ui,
    th: &Theme,
    settings: &mut Settings,
    label: &str,
    field: SurfaceBgField,
) {
    let base = surface_bg_base(&settings.appearance.theme_base, field);
    let cur = surface_bg_override(&settings.appearance.theme_overrides, field);
    let is_ov = cur.is_some();
    let val = cur.unwrap_or(base);
    let buf_id = ui.make_persistent_id(("appearance_surface_hex", field.key()));

    ui.horizontal(|ui| {
        // ── override dot ──
        let dot = COLOR_OVERRIDE_DOT_SIZE.value();
        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(dot, dot), egui::Sense::hover());
        if is_ov {
            ui.painter().circle_filled(
                dot_rect.center(),
                dot / 2.0,
                egui::Color32::from(th.accent_primary()),
            );
        }

        // ── 행 라벨 (자연어 — 번역) ──
        ui.add_sized(
            egui::vec2(COLOR_FIELD_NAME_WIDTH.value(), COLOR_SWATCH_SIZE.value()),
            egui::Label::new(egui::RichText::new(label).color(if is_ov {
                th.text_primary()
            } else {
                th.text_muted()
            })),
        );

        // ── hex 입력 ──
        if is_ov {
            let mut text = ui
                .data(|d| d.get_temp::<String>(buf_id))
                .unwrap_or_else(|| val.to_hex());
            let resp = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(COLOR_HEX_INPUT_WIDTH.value()),
            );
            if resp.changed()
                && let Some(parsed) = HexColor::from_hex(text.trim())
            {
                set_surface_bg_override(
                    &mut settings.appearance.theme_overrides,
                    field,
                    Some(parsed),
                );
            }
            ui.data_mut(|d| d.insert_temp(buf_id, text));
        } else {
            let mut text = base.to_hex();
            ui.add_enabled(
                false,
                egui::TextEdit::singleline(&mut text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(COLOR_HEX_INPUT_WIDTH.value()),
            );
        }

        // ── swatch (override 시 full, base 시 40% dim) ──
        let sw = COLOR_SWATCH_SIZE.value();
        let (sw_rect, _) = ui.allocate_exact_size(egui::vec2(sw, sw), egui::Sense::hover());
        let fill = if is_ov {
            egui::Color32::from(val)
        } else {
            val.with_alpha(SWATCH_INHERITED_ALPHA).to_egui()
        };
        ui.painter().rect_filled(sw_rect, 3.0, fill);
        ui.painter().rect_stroke(
            sw_rect,
            3.0,
            egui::Stroke::new(
                th.border_width.value(),
                egui::Color32::from(th.border_strong()),
            ),
            egui::StrokeKind::Inside,
        );

        // ── "Use default" 체크박스 ──
        let mut use_default = !is_ov;
        if ui
            .checkbox(
                &mut use_default,
                t("settings.appearance.terminal.use_default"),
            )
            .changed()
        {
            let ov = &mut settings.appearance.theme_overrides;
            if use_default {
                set_surface_bg_override(ov, field, None);
            } else {
                set_surface_bg_override(ov, field, Some(base));
                let seed = base.to_hex();
                ui.data_mut(|d| d.insert_temp(buf_id, seed));
            }
        }
    });
}

/// Target of a font override section: either the host-internal `terminal_font`
/// field or a plugin-contributed slot keyed by `storage_key` inside
/// `appearance.plugin_font_overrides`.
///
/// Held by reference (`&'a`) because Plugin uses a borrowed storage key — the
/// owning string lives in `SettingsPageContribute::items`.
#[derive(Clone, Copy)]
enum SurfaceFontTarget<'a> {
    Terminal,
    /// `storage_key` is the key inside `plugin_font_overrides` *and* doubles as
    /// the surface id used to pick a theme `SurfaceTheme` for the preview.
    Plugin {
        storage_key: &'a str,
    },
}

impl<'a> SurfaceFontTarget<'a> {
    fn salt(self) -> &'a str {
        match self {
            SurfaceFontTarget::Terminal => "terminal",
            SurfaceFontTarget::Plugin { storage_key } => storage_key,
        }
    }

    fn override_mut(self, app: &mut crate::settings::AppearanceSettings) -> &mut FontOverride {
        match self {
            SurfaceFontTarget::Terminal => &mut app.terminal_font,
            SurfaceFontTarget::Plugin { storage_key } => app
                .plugin_font_overrides
                .entry(storage_key.to_string())
                .or_default(),
        }
    }

    fn effective(self, app: &crate::settings::AppearanceSettings) -> EffectiveFont {
        match self {
            SurfaceFontTarget::Terminal => app.effective_terminal_font(),
            SurfaceFontTarget::Plugin { storage_key } => app.effective_font_for_kind(storage_key),
        }
    }

    fn surface_id(self) -> &'a str {
        match self {
            SurfaceFontTarget::Terminal => "terminal",
            SurfaceFontTarget::Plugin { storage_key } => storage_key,
        }
    }
}

fn draw_surface_font_section(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
    target: SurfaceFontTarget<'_>,
) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_xs);
    ui.label(
        egui::RichText::new(t("settings.appearance.font.override_heading"))
            .strong()
            .color(th.text_primary()),
    );
    vspace(ui, th.spacing_xs);
    ui.label(
        egui::RichText::new(t("settings.appearance.font.override_hint"))
            .small()
            .color(th.text_muted()),
    );
    vspace(ui, th.spacing_sm);

    ui.columns(2, |columns| {
        let default_font = settings.appearance.default_font.clone();
        font_override_grid(
            &mut columns[0],
            target.override_mut(&mut settings.appearance),
            &default_font,
            font_families,
            font_filter,
            target.salt(),
        );

        let eff = target.effective(&settings.appearance);
        let colors = crate::theme::theme().surface(target.surface_id()).clone();
        draw_font_preview(
            &mut columns[1],
            &eff,
            &colors,
            &settings.appearance,
            target.salt(),
            preview_font_loaded,
        );
    });
}

// ── Theme-color override picker (Appearance › Colors) ──────────────────
// `AppearanceSettings.theme_overrides`(= flat `PartialColors`) 의 46 색 필드를
// 6개 collapsible 그룹으로 편집한다. 각 행은 한 `PartialColors` 필드에 1:1 바인딩:
// per-row "Default" 체크 = 필드 `None`(프리셋 base 추종) / 해제 = `Some(hex)`(override).
// base 값은 resolved `theme_base` 에서 읽는다(하드코딩 없음). 프리셋 전환 시
// `theme_overrides` 가 통째로 클리어되는 모델(`apply_theme`)과 일관 — picker 는
// 그 단일 출처를 채울 뿐이다.

/// swatch 한 변 / override dot 지름 / hex 입력 폭 (디자인 jsx: 18·5·96 px).
const COLOR_SWATCH_SIZE: LogicalPx = LogicalPx(18.0);

/// 오버라이드되지 않은(= 테마에서 상속받은) 색 스와치의 알파. 디자인이 적은
/// opacity 0.4 를 알파로 옮긴 값(102/255)이다. 대응 토큰이 없어 이름만 둔다.
const SWATCH_INHERITED_ALPHA: u8 = 102;
const COLOR_OVERRIDE_DOT_SIZE: LogicalPx = LogicalPx(5.0);
const COLOR_HEX_INPUT_WIDTH: LogicalPx = LogicalPx(96.0);
/// 색 토큰 이름 컬럼 폭 — 행 간 입력/스와치/체크박스 정렬용.
const COLOR_FIELD_NAME_WIDTH: LogicalPx = LogicalPx(150.0);

/// Theme 프리셋 스와치(design #5): 5색 스트라이프 높이 / 스트라이프~라벨 사이 여백.
const THEME_SWATCH_STRIPE_HEIGHT: LogicalPx = LogicalPx(38.0);

/// 한 색 행: 표시 이름(기술 토큰, 비번역) + base/override 접근자(fn 포인터).
struct ColorRowDef {
    /// 색 토큰 이름(`crust`/`blue`/`ansi_black` …). 자연어가 아닌 기술 식별자라
    /// 번역하지 않는다(i18n 예외 — 디자인도 mono 리터럴로 표기).
    name: &'static str,
    /// resolved `theme_base` 에서 이 필드의 base 색을 읽는다.
    base: fn(&ThemeColors) -> HexColor,
    /// `theme_overrides` 에서 현재 override 값(없으면 `None`).
    get: fn(&PartialColors) -> Option<HexColor>,
    /// `theme_overrides` 의 이 필드를 설정/클리어.
    set: fn(&mut PartialColors, Option<HexColor>),
}

/// collapsible 그룹: 표시명(i18n) + 선택적 note(i18n) + 기본 열림 + 행들.
struct ColorGroupDef {
    name_key: &'static str,
    note_key: Option<&'static str>,
    default_open: bool,
    rows: Vec<ColorRowDef>,
}

/// `PartialColors`/`ThemeColors` 의 한 필드를 fn 포인터 묶음으로 바인딩.
macro_rules! color_row {
    ($field:ident) => {
        ColorRowDef {
            name: stringify!($field),
            base: |c| c.$field,
            get: |p| p.$field,
            set: |p, v| p.$field = v,
        }
    };
}

/// 디자인 `PALETTE_GROUPS` 와 동일한 6그룹/46필드 구성.
/// 기본 열림: Surfaces · Text · Accents / 기본 접힘: Overlays · Terminal-specific · ANSI 16.
fn color_groups() -> Vec<ColorGroupDef> {
    vec![
        ColorGroupDef {
            name_key: "settings.appearance.colors.group.surfaces",
            note_key: Some("settings.appearance.colors.group.surfaces_note"),
            default_open: true,
            rows: vec![
                color_row!(crust),
                color_row!(mantle),
                color_row!(base),
                color_row!(surface0),
                color_row!(surface1),
                color_row!(surface2),
            ],
        },
        ColorGroupDef {
            name_key: "settings.appearance.colors.group.overlays",
            note_key: None,
            default_open: false,
            rows: vec![
                color_row!(overlay0),
                color_row!(overlay1),
                color_row!(overlay2),
            ],
        },
        ColorGroupDef {
            name_key: "settings.appearance.colors.group.text",
            note_key: None,
            default_open: true,
            rows: vec![
                color_row!(text),
                color_row!(subtext1),
                color_row!(subtext0),
                color_row!(placeholder),
            ],
        },
        ColorGroupDef {
            name_key: "settings.appearance.colors.group.accents",
            note_key: None,
            default_open: true,
            rows: vec![
                color_row!(blue),
                color_row!(green),
                color_row!(red),
                color_row!(yellow),
                color_row!(peach),
                color_row!(mauve),
                color_row!(teal),
                color_row!(sky),
                color_row!(lavender),
                color_row!(flamingo),
                color_row!(pink),
                color_row!(maroon),
                color_row!(rosewater),
            ],
        },
        ColorGroupDef {
            name_key: "settings.appearance.colors.group.terminal",
            note_key: None,
            default_open: false,
            rows: vec![
                color_row!(selection_bg),
                color_row!(vi_cursor_bg),
                color_row!(search_match_bg),
                color_row!(search_match_active_bg),
            ],
        },
        ColorGroupDef {
            name_key: "settings.appearance.colors.group.ansi",
            note_key: None,
            default_open: false,
            rows: vec![
                color_row!(ansi_black),
                color_row!(ansi_red),
                color_row!(ansi_green),
                color_row!(ansi_yellow),
                color_row!(ansi_blue),
                color_row!(ansi_magenta),
                color_row!(ansi_cyan),
                color_row!(ansi_white),
                color_row!(ansi_bright_black),
                color_row!(ansi_bright_red),
                color_row!(ansi_bright_green),
                color_row!(ansi_bright_yellow),
                color_row!(ansi_bright_blue),
                color_row!(ansi_bright_magenta),
                color_row!(ansi_bright_cyan),
                color_row!(ansi_bright_white),
            ],
        },
    ]
}

/// 그룹 내 현재 override 가 걸린 행 수.
fn group_changed_count(group: &ColorGroupDef, overrides: &PartialColors) -> usize {
    group
        .rows
        .iter()
        .filter(|r| (r.get)(overrides).is_some())
        .count()
}

/// Appearance > Colors: 프리셋 색 개별 override picker.
fn draw_appearance_colors(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    let groups = color_groups();

    // 전체 override 개수 (picker 가 다루는 46 flat 필드 한정 — surface_themes 등
    // 비-picker override 는 세지도 지우지도 않는다).
    let total: usize = {
        let ov = &settings.appearance.theme_overrides;
        groups.iter().map(|g| group_changed_count(g, ov)).sum()
    };

    // 헤더: 설명 + 우측 Reset all (N).
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if total > 0 {
                crate::i18n::t_fmt("settings.appearance.colors.reset_all_n", &total.to_string())
            } else {
                t("settings.appearance.colors.reset_all").to_string()
            };
            if ui
                .add_enabled(total > 0, egui::Button::new(label))
                .clicked()
            {
                let ov = &mut settings.appearance.theme_overrides;
                for r in groups.iter().flat_map(|g| &g.rows) {
                    (r.set)(ov, None);
                }
            }
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(t("settings.appearance.colors.intro"))
                            .small()
                            .color(th.text_muted()),
                    )
                    .wrap(),
                );
            });
        });
    });

    vspace(ui, th.spacing_xs);

    for group in &groups {
        draw_color_group(ui, &th, settings, group);
    }
}

/// 한 collapsible 색 그룹 — 헤더(이름·note·N changed·Reset) + 행 목록.
fn draw_color_group(ui: &mut egui::Ui, th: &Theme, settings: &mut Settings, group: &ColorGroupDef) {
    let changed = group_changed_count(group, &settings.appearance.theme_overrides);

    let id = ui.make_persistent_id(("appearance_colors_group", group.name_key));
    let state = egui::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        group.default_open,
    );

    vspace(ui, th.spacing_xs);
    let header = state.show_header(ui, |ui| {
        ui.label(
            egui::RichText::new(t(group.name_key))
                .monospace()
                .size(th.font_size_caption.value())
                .strong()
                .color(th.text_muted()),
        );
        if let Some(note_key) = group.note_key {
            ui.label(
                egui::RichText::new(format!("· {}", t(note_key)))
                    .small()
                    .color(th.text_muted()),
            );
        }
        if changed > 0 {
            ui.label(
                egui::RichText::new(crate::i18n::t_fmt(
                    "settings.appearance.colors.changed_n",
                    &changed.to_string(),
                ))
                .monospace()
                .size(th.font_size_caption.value())
                .color(th.accent_primary()),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t("settings.appearance.colors.reset")).clicked() {
                    let ov = &mut settings.appearance.theme_overrides;
                    for r in &group.rows {
                        (r.set)(ov, None);
                    }
                }
            });
        }
    });
    header.body(|ui| {
        for row in &group.rows {
            draw_color_picker_row(ui, th, settings, row);
        }
    });
}

/// 한 색 행: [override dot] 이름(mono) · hex 입력 · swatch · "Default" 체크박스.
/// "Default" 체크 = `None`(base 추종, 입력 비활성+swatch dim) / 해제 = `Some(hex)`.
fn draw_color_picker_row(
    ui: &mut egui::Ui,
    th: &Theme,
    settings: &mut Settings,
    row: &ColorRowDef,
) {
    let base = (row.base)(&settings.appearance.theme_base);
    let cur = (row.get)(&settings.appearance.theme_overrides);
    let is_ov = cur.is_some();
    let val = cur.unwrap_or(base);
    // hex 입력 버퍼는 egui 메모리에 행별로 보관 — override 값을 매 프레임 문자열로
    // 되돌리면 편집 중 부분 입력(`#89b4f`)이 즉시 덮어써져 타이핑이 불가능해진다.
    let buf_id = ui.make_persistent_id(("appearance_colors_hex", row.name));

    ui.horizontal(|ui| {
        // ── override dot ──
        let dot = COLOR_OVERRIDE_DOT_SIZE.value();
        let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(dot, dot), egui::Sense::hover());
        if is_ov {
            ui.painter().circle_filled(
                dot_rect.center(),
                dot / 2.0,
                egui::Color32::from(th.accent_primary()),
            );
        }

        // ── 색 토큰 이름 (mono, override 시 강조) ──
        ui.add_sized(
            egui::vec2(COLOR_FIELD_NAME_WIDTH.value(), COLOR_SWATCH_SIZE.value()),
            egui::Label::new(egui::RichText::new(row.name).monospace().color(if is_ov {
                th.text_primary()
            } else {
                th.text_muted()
            })),
        );

        // ── hex 입력 ──
        if is_ov {
            let mut text = ui
                .data(|d| d.get_temp::<String>(buf_id))
                .unwrap_or_else(|| val.to_hex());
            let resp = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(COLOR_HEX_INPUT_WIDTH.value()),
            );
            if resp.changed()
                && let Some(parsed) = HexColor::from_hex(text.trim())
            {
                (row.set)(&mut settings.appearance.theme_overrides, Some(parsed));
            }
            ui.data_mut(|d| d.insert_temp(buf_id, text));
        } else {
            // base 값을 읽기 전용으로 보여준다 (비활성·dim).
            let mut text = base.to_hex();
            ui.add_enabled(
                false,
                egui::TextEdit::singleline(&mut text)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(COLOR_HEX_INPUT_WIDTH.value()),
            );
        }

        // ── swatch (override 시 full, base 시 40% dim) ──
        let sw = COLOR_SWATCH_SIZE.value();
        let (sw_rect, _) = ui.allocate_exact_size(egui::vec2(sw, sw), egui::Sense::hover());
        let fill = if is_ov {
            egui::Color32::from(val)
        } else {
            // opacity 0.4 (디자인) — 패널 위에 얹혀 dim 하게 보인다.
            val.with_alpha(SWATCH_INHERITED_ALPHA).to_egui()
        };
        ui.painter().rect_filled(sw_rect, 3.0, fill);
        ui.painter().rect_stroke(
            sw_rect,
            3.0,
            egui::Stroke::new(
                th.border_width.value(),
                egui::Color32::from(th.border_strong()),
            ),
            egui::StrokeKind::Inside,
        );

        // ── "Default" 체크박스 (디자인 settings_window.jsx:113 = <Checkbox>) ──
        let mut use_default = !is_ov;
        let default_label = t("settings.appearance.colors.default");
        if tasty_ui_widgets::checkbox(ui, th, &mut use_default, default_label, true).changed() {
            let ov = &mut settings.appearance.theme_overrides;
            if use_default {
                (row.set)(ov, None);
            } else {
                // override 활성화: base 값으로 시드 + 입력 버퍼도 base 로 재설정해
                // 이전 세션의 stale 버퍼가 base 시드를 덮어쓰지 않게 한다.
                (row.set)(ov, Some(base));
                let seed = base.to_hex();
                ui.data_mut(|d| d.insert_temp(buf_id, seed));
            }
        }
    });
}

/// Appearance > Plugin-contributed page. Renders each `SettingsItemDecl` in the
/// page using a generic widget (currently only `FontOverride`).
///
/// The contract is fixed by `SettingsPageContribute`: host knows the *shape*
/// (FontOverride → label + override grid + preview), plugin owns the *storage*
/// (`appearance.plugin_font_overrides[storage_key]`). Color/Bool/Enum item
/// kinds will land in later TODOs and route through the same dispatch.
///
/// Note: surface 색 picker 가 사라졌다. theme TOML
/// (`~/.tasty/themes/<id>.toml` 의 `[surfaces.<storage_key>]`) 에서 직접 편집.
pub(super) fn draw_plugin_settings_page(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    preview_font_loaded: &mut HashMap<String, String>,
    plugin_id: &str,
    page: &SettingsPageContribute,
) {
    for item in &page.items {
        match item {
            SettingsItemDecl::FontOverride {
                id: _,
                label_key: _,
                storage_key,
            } => {
                draw_surface_font_section(
                    ui,
                    settings,
                    font_families,
                    font_filter,
                    preview_font_loaded,
                    SurfaceFontTarget::Plugin {
                        storage_key: storage_key.as_str(),
                    },
                );
            }
            SettingsItemDecl::Toggle {
                id: _,
                label_key,
                storage_key,
                default,
            } => {
                draw_plugin_toggle(ui, settings, plugin_id, label_key, storage_key, *default);
            }
            SettingsItemDecl::Select {
                id: _,
                label_key,
                storage_key,
                options,
                default,
            } => {
                draw_plugin_select(
                    ui,
                    settings,
                    plugin_id,
                    label_key,
                    storage_key,
                    options,
                    default,
                );
            }
            SettingsItemDecl::Number {
                id: _,
                label_key,
                storage_key,
                default,
                min,
                max,
                suffix_key,
            } => {
                draw_plugin_number(
                    ui,
                    settings,
                    plugin_id,
                    label_key,
                    storage_key,
                    *default,
                    *min,
                    *max,
                    suffix_key.as_deref(),
                );
            }
        }
    }
}

/// Plugin settings row 의 공통 레이아웃 — 라벨 좌 / 컨트롤 우 (디자인
/// `settings_window.jsx:240-248` HTML 행: label 좌, 컨트롤 우 Row). 컨트롤은
/// `right_to_left` 클로저 안에서 그려진다.
fn plugin_setting_row(ui: &mut egui::Ui, label: &str, control: impl FnOnce(&mut egui::Ui)) {
    let th = crate::theme::theme();
    ui.add_space(th.spacing_sm.value());
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(th.text_primary()));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
    });
}

/// `Toggle` → 디자인 Switch (host `tasty_ui_widgets::switch`). bool read/write.
fn draw_plugin_toggle(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    plugin_id: &str,
    label_key: &str,
    storage_key: &str,
    default: bool,
) {
    let cur = match settings.plugin_setting(plugin_id, storage_key) {
        Some(crate::settings::PluginSettingValue::Bool(b)) => *b,
        _ => default,
    };
    let mut val = cur;
    let th = crate::theme::theme();
    plugin_setting_row(ui, t(label_key), |ui| {
        tasty_ui_widgets::switch(ui, &th, &mut val, None, true);
    });
    if val != cur {
        settings.set_plugin_setting(
            plugin_id,
            storage_key,
            crate::settings::PluginSettingValue::Bool(val),
        );
    }
}

/// `Select` → 디자인 Select (host `tasty_ui_widgets::select`). options label_key 를
/// `t()` 로 표시하고 선택 `value`(String) read/write.
fn draw_plugin_select(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    plugin_id: &str,
    label_key: &str,
    storage_key: &str,
    options: &[tasty_plugin_manifest::SelectOptionDecl],
    default: &str,
) {
    let cur = match settings.plugin_setting(plugin_id, storage_key) {
        Some(crate::settings::PluginSettingValue::Text(s)) => s.clone(),
        _ => default.to_string(),
    };
    let mut idx = options.iter().position(|o| o.value == cur).unwrap_or(0);
    let labels: Vec<String> = options
        .iter()
        .map(|o| t(&o.label_key).to_string())
        .collect();
    let label_refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
    let th = crate::theme::theme();
    let salt = format!("plugin_select_{plugin_id}_{storage_key}");
    let mut changed = false;
    plugin_setting_row(ui, t(label_key), |ui| {
        changed = tasty_ui_widgets::select(
            ui,
            &th,
            &salt,
            &mut idx,
            &label_refs,
            th.field_width_md.value(),
            true,
        );
    });
    if changed && idx < options.len() {
        let new_val = options[idx].value.clone();
        if new_val != cur {
            settings.set_plugin_setting(
                plugin_id,
                storage_key,
                crate::settings::PluginSettingValue::Text(new_val),
            );
        }
    }
}

/// `Number` → 디자인 text `Input`(mono, width xs) + 선택적 suffix. min/max clamp.
/// f64 read/write (정수면 정수 표기). immediate mode 라 편집 버퍼는 egui 메모리에
/// `plugin_id`+`storage_key` id 로 프레임 간 보관한다.
#[allow(clippy::too_many_arguments)]
fn draw_plugin_number(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    plugin_id: &str,
    label_key: &str,
    storage_key: &str,
    default: f64,
    min: Option<f64>,
    max: Option<f64>,
    suffix_key: Option<&str>,
) {
    let cur = match settings.plugin_setting(plugin_id, storage_key) {
        Some(crate::settings::PluginSettingValue::Number(n)) => *n,
        _ => default,
    };
    let th = crate::theme::theme();
    // 정수면 정수 표기.
    let fmt = |n: f64| -> String {
        if n.fract() == 0.0 {
            format!("{n:.0}")
        } else {
            format!("{n}")
        }
    };
    // 프레임 간 유지되는 편집 버퍼.
    let buf_id = egui::Id::new(("plugin_number_buf", plugin_id, storage_key));
    let mut buf = ui
        .data_mut(|d| d.get_temp::<String>(buf_id))
        .unwrap_or_else(|| fmt(cur));

    let mut resp = None;
    plugin_setting_row(ui, t(label_key), |ui| {
        // right_to_left: 먼저 add 한 suffix 가 가장 우측, 그 왼쪽에 입력 필드.
        if let Some(sk) = suffix_key {
            ui.label(egui::RichText::new(t(sk)).color(th.text_muted()));
        }
        resp = Some(
            tasty_ui_widgets::Input::new()
                .mono(true)
                .width(th.field_width_xs.value())
                .show(ui, &th, &mut buf),
        );
    });
    let resp = resp.expect("input always drawn");

    if !resp.has_focus() {
        // 편집 중이 아니면 버퍼를 저장값으로 동기화(초기 표시 + 포커스 아웃 시 정규화).
        let synced = fmt(cur);
        if buf != synced {
            buf = synced;
        }
    } else if resp.changed() {
        // 편집 중 유효 f64 → clamp 후 저장(변경 시에만). 빈/무효 입력은 무시(마지막 유효값 유지).
        if let Ok(parsed) = buf.trim().parse::<f64>() {
            let mut clamped = parsed;
            if let Some(lo) = min {
                clamped = clamped.max(lo);
            }
            if let Some(hi) = max {
                clamped = clamped.min(hi);
            }
            if clamped != cur {
                settings.set_plugin_setting(
                    plugin_id,
                    storage_key,
                    crate::settings::PluginSettingValue::Number(clamped),
                );
            }
        }
    }

    ui.data_mut(|d| d.insert_temp(buf_id, buf));
}

/// Searchable font family combo. `value` is the family name in the underlying
/// data (`""` means "monospace default"). `salt` uniquifies the combo id and
/// the per-combo filter cache key.
fn font_family_picker(
    ui: &mut egui::Ui,
    value: &mut String,
    font_families: &Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    salt: &str,
    enabled: bool,
) {
    let th = crate::theme::theme();
    let display_name = if value.is_empty() {
        "monospace (default)".to_string()
    } else {
        value.clone()
    };
    let combo_id = format!("font_family_combo_{}", salt);
    let filter = font_filter.entry(salt.to_string()).or_default();

    ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_id_salt(combo_id)
            .selected_text(&display_name)
            .width(200.0)
            .height(300.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show_ui(ui, |ui| {
                ui.add(
                    egui::TextEdit::singleline(filter)
                        .hint_text(tasty_egui_theme::hint_text(
                            &crate::theme::theme(),
                            t("settings.appearance.search_hint"),
                        ))
                        .desired_width(190.0),
                );
                ui.separator();

                let filter_lower = filter.to_lowercase();
                if (filter_lower.is_empty() || "monospace".contains(&filter_lower))
                    && ui
                        .selectable_label(value.is_empty(), "monospace (default)")
                        .clicked()
                {
                    value.clear();
                }

                if let Some(families) = font_families {
                    egui::ScrollArea::vertical()
                        .max_height(th.font_family_menu_max_height().value())
                        .drag_to_scroll(false)
                        .show(ui, |ui| {
                            for family in families {
                                if !filter_lower.is_empty()
                                    && !family.to_lowercase().contains(&filter_lower)
                                {
                                    continue;
                                }
                                let selected = value == family;
                                if ui.selectable_label(selected, family).clicked() {
                                    *value = family.clone();
                                }
                            }
                        });
                } else {
                    ui.label(
                        egui::RichText::new(t("settings.appearance.loading_fonts"))
                            .color(th.text_muted()),
                    );
                }
            });
    });
}

/// Edit a `FontSettings` (no fallback semantics — every field is always set).
fn font_settings_grid(
    ui: &mut egui::Ui,
    font: &mut FontSettings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    salt: &str,
) {
    egui::Grid::new(format!("font_settings_grid_{}", salt))
        .num_columns(2)
        .spacing([12.0, 8.0])
        .show(ui, |ui| {
            ui.label(t("settings.appearance.font_family_label"));
            font_family_picker(
                ui,
                &mut font.font_family,
                font_families,
                font_filter,
                salt,
                true,
            );
            ui.end_row();

            ui.label(t("settings.appearance.custom_font_label"));
            ui.text_edit_singleline(&mut font.custom_font_path);
            ui.end_row();

            ui.label(t("settings.appearance.font_size_label"));
            ui.add(
                egui::DragValue::new(&mut font.font_size)
                    .range(6.0..=72.0)
                    .speed(0.5),
            );
            ui.end_row();

            label_with_tooltip(
                ui,
                t("settings.appearance.line_height_label"),
                t("settings.appearance.line_height_tooltip"),
            );
            ui.add(
                egui::DragValue::new(&mut font.line_height)
                    .range(0.8..=2.0)
                    .speed(0.05)
                    .max_decimals(2),
            );
            ui.end_row();

            label_with_tooltip(
                ui,
                t("settings.appearance.font_scale_mode_label"),
                t("settings.appearance.font_scale_mode_tooltip"),
            );
            font_scale_mode_combo(ui, &mut font.font_scale_mode, salt, true);
            ui.end_row();
        });
}

/// Edit a `FontOverride` against a `FontSettings` default. Each row has a
/// "use default" checkbox: checked → override field is `None` (input
/// disabled, default value shown for reference). Unchecked → override is
/// `Some(current_effective_value)` and the input is enabled.
fn font_override_grid(
    ui: &mut egui::Ui,
    ov: &mut FontOverride,
    default: &FontSettings,
    font_families: &mut Option<Vec<String>>,
    font_filter: &mut HashMap<String, String>,
    salt: &str,
) {
    egui::Grid::new(format!("font_override_grid_{}", salt))
        .num_columns(3)
        .spacing([8.0, 8.0])
        .show(ui, |ui| {
            // ── Font family ──
            ui.label(t("settings.appearance.font_family_label"));
            override_checkbox(
                ui,
                &mut ov.font_family,
                || default.font_family.clone(),
                salt,
            );
            let mut family_value = ov
                .font_family
                .clone()
                .unwrap_or_else(|| default.font_family.clone());
            font_family_picker(
                ui,
                &mut family_value,
                font_families,
                font_filter,
                salt,
                ov.font_family.is_some(),
            );
            if let Some(stored) = ov.font_family.as_mut() {
                *stored = family_value;
            }
            ui.end_row();

            // ── Custom font path ──
            ui.label(t("settings.appearance.custom_font_label"));
            override_checkbox(
                ui,
                &mut ov.custom_font_path,
                || default.custom_font_path.clone(),
                salt,
            );
            let mut path_value = ov
                .custom_font_path
                .clone()
                .unwrap_or_else(|| default.custom_font_path.clone());
            ui.add_enabled_ui(ov.custom_font_path.is_some(), |ui| {
                ui.text_edit_singleline(&mut path_value);
            });
            if let Some(stored) = ov.custom_font_path.as_mut() {
                *stored = path_value;
            }
            ui.end_row();

            // ── Font size ──
            ui.label(t("settings.appearance.font_size_label"));
            override_checkbox(ui, &mut ov.font_size, || default.font_size, salt);
            let mut size_value = ov.font_size.unwrap_or(default.font_size);
            ui.add_enabled_ui(ov.font_size.is_some(), |ui| {
                ui.add(
                    egui::DragValue::new(&mut size_value)
                        .range(6.0..=72.0)
                        .speed(0.5),
                );
            });
            if let Some(stored) = ov.font_size.as_mut() {
                *stored = size_value;
            }
            ui.end_row();

            // ── Line height ──
            label_with_tooltip(
                ui,
                t("settings.appearance.line_height_label"),
                t("settings.appearance.line_height_tooltip"),
            );
            override_checkbox(ui, &mut ov.line_height, || default.line_height, salt);
            let mut lh_value = ov.line_height.unwrap_or(default.line_height);
            ui.add_enabled_ui(ov.line_height.is_some(), |ui| {
                ui.add(
                    egui::DragValue::new(&mut lh_value)
                        .range(0.8..=2.0)
                        .speed(0.05)
                        .max_decimals(2),
                );
            });
            if let Some(stored) = ov.line_height.as_mut() {
                *stored = lh_value;
            }
            ui.end_row();

            // ── Font scale mode ──
            label_with_tooltip(
                ui,
                t("settings.appearance.font_scale_mode_label"),
                t("settings.appearance.font_scale_mode_tooltip"),
            );
            override_checkbox(
                ui,
                &mut ov.font_scale_mode,
                || default.font_scale_mode.clone(),
                salt,
            );
            let mut mode_value = ov
                .font_scale_mode
                .clone()
                .unwrap_or_else(|| default.font_scale_mode.clone());
            font_scale_mode_combo(ui, &mut mode_value, salt, ov.font_scale_mode.is_some());
            if let Some(stored) = ov.font_scale_mode.as_mut() {
                *stored = mode_value;
            }
            ui.end_row();
        });
}

/// "Use default" checkbox: checked when override is None.
/// Toggling on → set to None; toggling off → seed with the current default.
fn override_checkbox<T, F>(
    ui: &mut egui::Ui,
    slot: &mut Option<T>,
    default_provider: F,
    _salt: &str,
) where
    F: FnOnce() -> T,
{
    let th = crate::theme::theme();
    let mut use_default = slot.is_none();
    let label = t("settings.appearance.font.use_default_label");
    if tasty_ui_widgets::checkbox(ui, &th, &mut use_default, label, true).changed() {
        if use_default {
            *slot = None;
        } else if slot.is_none() {
            *slot = Some(default_provider());
        }
    }
}

fn font_scale_mode_combo(ui: &mut egui::Ui, value: &mut String, salt: &str, enabled: bool) {
    ui.add_enabled_ui(enabled, |ui| {
        let combo_id = format!("font_scale_mode_{}", salt);
        egui::ComboBox::from_id_salt(combo_id)
            .selected_text(match value.as_str() {
                "auto" => t("settings.appearance.font_scale_mode_auto"),
                _ => t("settings.appearance.font_scale_mode_fixed"),
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    value,
                    "auto".to_string(),
                    t("settings.appearance.font_scale_mode_auto"),
                );
                ui.selectable_value(
                    value,
                    "fixed".to_string(),
                    t("settings.appearance.font_scale_mode_fixed"),
                );
            });
    });
}

/// Draw a 2-row colored preview block for an `EffectiveFont`. `slot` is a
/// short id ("default"/"terminal"/"markdown"/"explorer") used as both the egui
/// font family slot name and the cache key in `preview_font_loaded`.
fn draw_font_preview(
    ui: &mut egui::Ui,
    eff: &EffectiveFont,
    colors: &SurfaceTheme,
    appearance: &crate::settings::AppearanceSettings,
    slot: &str,
    preview_font_loaded: &mut HashMap<String, String>,
) {
    let th = crate::theme::theme();
    ui.heading(t("settings.appearance.preview_heading"));
    vspace(ui, th.spacing_xs);

    let slot_name = format!("preview_{}", slot);
    let display_family = if eff.font_family.is_empty() {
        "monospace".to_string()
    } else {
        eff.font_family.clone()
    };

    // Decide which egui FontFamily to render text in. We try to load the
    // requested family into a per-slot named family the first time we see it,
    // and remember success/failure in `preview_font_loaded` so we don't retry.
    let preview_family = if eff.font_family.is_empty() && eff.custom_font_path.is_empty() {
        egui::FontFamily::Monospace
    } else {
        let key = format!("{}|{}", eff.font_family, eff.custom_font_path);
        let failed_marker = format!("\x00:{}", key);
        let cached = preview_font_loaded
            .get(&slot_name)
            .cloned()
            .unwrap_or_default();
        if cached == key {
            egui::FontFamily::Name(slot_name.clone().into())
        } else if cached == failed_marker {
            egui::FontFamily::Monospace
        } else {
            // First attempt this frame: rebuild the full FontDefinitions
            // (surface families + this preview slot) and install it.
            let fonts = crate::adapters::ui::font_registry::build_font_definitions(
                appearance,
                Some((&slot_name, eff)),
            );
            ui.ctx().set_fonts(fonts);
            // set_fonts() replaces the entire FontDefinitions, so other
            // preview slots are no longer registered. Clear them so they
            // get re-loaded when their tab is revisited.
            preview_font_loaded.retain(|k, _| k == &slot_name);
            preview_font_loaded.insert(slot_name.clone(), key);
            // The family won't be available until the next frame; fall back
            // to Monospace this frame.
            egui::FontFamily::Monospace
        }
    };

    let sample_lines = [
        "AaBbCcDdEeFfGg",
        "\u{AC00}\u{B098}\u{B2E4}\u{B77C}\u{B9C8}\u{BC14}\u{C0AC}", // 가나다라마바사
        "1234567890",
        "\u{30A2}\u{30AB}\u{30B5}\u{30BF}\u{30CA}\u{30CF}\u{30DE}\u{30E9}\u{30E4}\u{30EF}", // アカサタナハマラヤワ
    ];

    let focused_bg32 = colors.focused_bg.to_egui();
    let unfocused_bg32 = colors.unfocused_bg.to_egui();
    let fg32 = colors.focused_fg.to_egui();

    let font_size = eff.font_size.max(1.0);
    let preview_font = egui::FontId::new(font_size, preview_family);
    let line_height = font_size * 1.4;
    let padding = 8.0;
    let block_height = line_height * sample_lines.len() as f32 + padding * 2.0;

    // ── Focused preview ──
    ui.label(
        egui::RichText::new(t("settings.appearance.preview_focused"))
            .size(th.font_size_caption.value())
            .color(th.text_muted()),
    );
    let (focused_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), block_height),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(focused_rect, 2.0, focused_bg32);
    for (i, line) in sample_lines.iter().enumerate() {
        let pos = focused_rect.min + egui::vec2(padding, padding + line_height * i as f32);
        ui.painter().text(
            pos,
            egui::Align2::LEFT_TOP,
            line,
            preview_font.clone(),
            fg32,
        );
    }

    vspace(ui, th.spacing_sm);

    // ── Unfocused preview ──
    ui.label(
        egui::RichText::new(t("settings.appearance.preview_unfocused"))
            .size(th.font_size_caption.value())
            .color(th.text_muted()),
    );
    let (unfocused_rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), block_height),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(unfocused_rect, 2.0, unfocused_bg32);
    for (i, line) in sample_lines.iter().enumerate() {
        let pos = unfocused_rect.min + egui::vec2(padding, padding + line_height * i as f32);
        ui.painter().text(
            pos,
            egui::Align2::LEFT_TOP,
            line,
            preview_font.clone(),
            fg32,
        );
    }

    vspace(ui, th.spacing_sm);
    ui.label(
        egui::RichText::new(crate::i18n::t_fmt(
            "settings.appearance.preview_font_info",
            &format!("{} / {:.1}px", display_family, font_size),
        ))
        .size(th.font_size_caption.value())
        .color(th.text_muted()),
    );
}

#[cfg(test)]
mod swatch_tests {
    use super::{HexColor, representative_swatch};
    use tasty_themes::ThemeFile;

    #[test]
    // 테스트 기대값 리터럴 구성 — 색을 "디자인"하는 게 아니라 파서 출력과 비교할
    // 고정 팔레트 값을 만든다 (clippy.toml disallowed-methods 예외 컨벤션).
    #[allow(clippy::disallowed_methods)]
    fn swatch_derives_from_theme_file_colors() {
        let text = r##"
            [palette]
            crust = "#11111b"
            base = "#1e1e2e"
            [accent]
            blue = "#89b4fa"
            mauve = "#cba6f7"
            green = "#a6e3a1"
        "##;
        let file = ThemeFile::parse(text).expect("parse");
        // [bg, surface, accent, accent_alt, success]
        assert_eq!(
            representative_swatch(&file),
            [
                HexColor::from_rgb(0x11, 0x11, 0x1b),
                HexColor::from_rgb(0x1e, 0x1e, 0x2e),
                HexColor::from_rgb(0x89, 0xb4, 0xfa),
                HexColor::from_rgb(0xcb, 0xa6, 0xf7),
                HexColor::from_rgb(0xa6, 0xe3, 0xa1),
            ]
        );
    }

    #[test]
    fn swatch_falls_back_to_mocha_base_when_unspecified() {
        // 색을 하나도 지정하지 않은 테마 → 5색 모두 mocha fallback.
        let file = ThemeFile::default();
        let base = tasty_themes::mocha_fallback_colors();
        assert_eq!(
            representative_swatch(&file),
            [base.crust, base.base, base.blue, base.mauve, base.green]
        );
    }
}

#[cfg(test)]
mod tests {
    use super::find_plugin_settings_entry;
    use tasty_host_plugin::SettingsPageEntry;
    use tasty_plugin_manifest::{SettingsCategory, SettingsPageContribute};

    fn entry(plugin_id: &str, page_id: &str) -> SettingsPageEntry {
        SettingsPageEntry {
            plugin_id: plugin_id.into(),
            page: SettingsPageContribute {
                id: page_id.into(),
                title_key: format!("{plugin_id}.{page_id}.title"),
                category: SettingsCategory::Appearance,
                items: vec![],
            },
        }
    }

    /// 서로 다른 plugin 이 동일한 `page_id` ("theme") 를 contribute 해도
    /// `(plugin_id, page_id)` 복합키 매칭이 각각의 entry 를 정확히 가려낸다.
    #[test]
    fn same_page_id_from_different_plugins_resolves_correctly() {
        let entries = vec![entry("alpha", "theme"), entry("beta", "theme")];

        let found_alpha = find_plugin_settings_entry(&entries, "alpha", "theme")
            .expect("alpha/theme should be found");
        assert_eq!(found_alpha.plugin_id, "alpha");

        let found_beta = find_plugin_settings_entry(&entries, "beta", "theme")
            .expect("beta/theme should be found");
        assert_eq!(found_beta.plugin_id, "beta");

        // page_id 만 보고 첫 매칭을 반환하던 버그 회귀 방지: beta 조회 시
        // alpha 가 잘못 반환되면 안 된다.
        assert_ne!(found_beta.plugin_id, found_alpha.plugin_id);
    }

    #[test]
    fn missing_plugin_returns_none() {
        let entries = vec![entry("alpha", "theme")];
        assert!(find_plugin_settings_entry(&entries, "gamma", "theme").is_none());
        assert!(find_plugin_settings_entry(&entries, "alpha", "other").is_none());
    }

    // ── Colors override picker — 상태 변환 로직 (egui 위젯 외) ──
    use super::{ColorGroupDef, color_groups, group_changed_count};
    use crate::settings::HexColor;
    use tasty_type_appearance::theme::PartialColors;

    fn find_row<'a>(groups: &'a [ColorGroupDef], name: &str) -> &'a super::ColorRowDef {
        groups
            .iter()
            .flat_map(|g| &g.rows)
            .find(|r| r.name == name)
            .unwrap_or_else(|| panic!("row {name} should exist"))
    }

    /// 디자인 `PALETTE_GROUPS` 와 동일한 6그룹/46필드 + 기본 열림/접힘 분류.
    #[test]
    fn color_groups_match_design_layout() {
        let groups = color_groups();
        let counts: Vec<(usize, bool)> = groups
            .iter()
            .map(|g| (g.rows.len(), g.default_open))
            .collect();
        // Surfaces(6,열림) Overlays(3,접힘) Text(4,열림) Accents(13,열림)
        // Terminal-specific(4,접힘) ANSI 16(16,접힘).
        assert_eq!(
            counts,
            vec![
                (6, true),
                (3, false),
                (4, true),
                (13, true),
                (4, false),
                (16, false),
            ]
        );
        let total: usize = groups.iter().map(|g| g.rows.len()).sum();
        assert_eq!(total, 46, "flat PartialColors 46 fields");
    }

    /// "Default" 체크박스 토글 = 필드 Some(base)⇄None — set/get 라운드트립.
    #[test]
    fn toggle_sets_and_clears_override_field() {
        let groups = color_groups();
        let blue = find_row(&groups, "blue");
        let mut ov = PartialColors::default();
        assert!((blue.get)(&ov).is_none());

        let c = HexColor::from_hex("#112233").unwrap();
        (blue.set)(&mut ov, Some(c)); // 체크 해제 → Some
        assert_eq!((blue.get)(&ov), Some(c));

        (blue.set)(&mut ov, None); // 다시 체크 → None
        assert!((blue.get)(&ov).is_none());
    }

    /// base 접근자는 resolved `theme_base` 의 해당 필드를 읽는다 (하드코딩 아님).
    #[test]
    fn base_reads_from_theme_base() {
        let mut base = tasty_themes::mocha_fallback_colors();
        let marker = HexColor::from_hex("#abcdef").unwrap();
        base.crust = marker;
        let groups = color_groups();
        let crust = find_row(&groups, "crust");
        assert_eq!((crust.base)(&base), marker);
    }

    /// 그룹 Reset 은 그 그룹 필드만 클리어하고 다른 그룹 override 는 보존.
    #[test]
    fn group_reset_clears_only_its_fields() {
        let groups = color_groups();
        let blue = find_row(&groups, "blue"); // Accents
        let crust = find_row(&groups, "crust"); // Surfaces
        let c = HexColor::from_hex("#010203").unwrap();

        let mut ov = PartialColors::default();
        (blue.set)(&mut ov, Some(c));
        (crust.set)(&mut ov, Some(c));

        // Accents 그룹만 reset.
        let accents = groups
            .iter()
            .find(|g| g.name_key.ends_with(".accents"))
            .unwrap();
        for r in &accents.rows {
            (r.set)(&mut ov, None);
        }
        assert!((blue.get)(&ov).is_none(), "blue cleared");
        assert_eq!((crust.get)(&ov), Some(c), "crust preserved");
        assert_eq!(group_changed_count(accents, &ov), 0);
    }

    // ── Tasty curated section (accent/sidebar bg → theme_overrides + indicator) ──
    use super::reset_tasty_to_theme_defaults;
    use crate::settings::{ActiveTabIndicator, AppearanceSettings};

    /// Tasty › Accent/Sidebar bg 는 `theme_overrides.blue`/`.mantle` 에 set/clear.
    #[test]
    fn tasty_accent_sidebar_map_to_blue_and_mantle() {
        let mut app = AppearanceSettings::default();
        let c = HexColor::from_hex("#445566").unwrap();
        assert!(app.theme_overrides.blue.is_none());
        assert!(app.theme_overrides.mantle.is_none());

        app.theme_overrides.blue = Some(c); // Accent
        app.theme_overrides.mantle = Some(c); // Sidebar bg
        assert_eq!(app.theme_overrides.blue, Some(c));
        assert_eq!(app.theme_overrides.mantle, Some(c));
    }

    /// "Use theme defaults" = accent/sidebar override clear + indicator 기본값 복귀.
    #[test]
    fn tasty_use_defaults_clears_overrides_and_resets_indicator() {
        let mut app = AppearanceSettings::default();
        let c = HexColor::from_hex("#778899").unwrap();
        app.theme_overrides.blue = Some(c);
        app.theme_overrides.mantle = Some(c);
        app.active_tab_indicator = ActiveTabIndicator::Dot;
        // 무관 override 는 보존되어야 한다 (curated 두 필드만 건드림).
        app.theme_overrides.red = Some(c);

        reset_tasty_to_theme_defaults(&mut app);

        assert!(app.theme_overrides.blue.is_none(), "accent cleared");
        assert!(app.theme_overrides.mantle.is_none(), "sidebar bg cleared");
        assert_eq!(
            app.active_tab_indicator,
            ActiveTabIndicator::default(),
            "indicator reset to default"
        );
        assert_eq!(
            app.theme_overrides.red,
            Some(c),
            "unrelated override preserved"
        );
    }

    /// Reset all 은 46 flat 필드만 클리어하고 surface_themes(비-picker) 는 보존.
    #[test]
    fn reset_all_preserves_surface_themes() {
        let groups = color_groups();
        let mut ov = PartialColors::default();
        let c = HexColor::from_hex("#0a0b0c").unwrap();
        find_row(&groups, "blue");
        (find_row(&groups, "blue").set)(&mut ov, Some(c));
        ov.surface_themes.insert(
            "terminal".to_string(),
            tasty_type_appearance::theme::PartialSurfaceTheme {
                focused_bg: Some(c),
                ..Default::default()
            },
        );

        // Reset all = picker 가 다루는 flat 필드만 None.
        for r in groups.iter().flat_map(|g| &g.rows) {
            (r.set)(&mut ov, None);
        }
        let flat_changed: usize = groups.iter().map(|g| group_changed_count(g, &ov)).sum();
        assert_eq!(flat_changed, 0, "all 46 flat overrides cleared");
        assert!(
            ov.surface_themes.contains_key("terminal"),
            "surface_themes override preserved"
        );
    }

    // ── Terminal surface bg picker (focused/unfocused background) ──
    use super::{SurfaceBgField, set_surface_bg_override, surface_bg_base, surface_bg_override};

    /// override set → resolve = override 색, clear(None) → entry pruned + base 추종.
    #[test]
    fn surface_bg_override_set_clear_roundtrip() {
        let mut ov = PartialColors::default();
        assert!(surface_bg_override(&ov, SurfaceBgField::Focused).is_none());

        let c = HexColor::from_hex("#123456").unwrap();
        set_surface_bg_override(&mut ov, SurfaceBgField::Focused, Some(c));
        assert_eq!(surface_bg_override(&ov, SurfaceBgField::Focused), Some(c));
        // 다른 필드는 독립.
        assert!(surface_bg_override(&ov, SurfaceBgField::Unfocused).is_none());

        // clear → None + 빈 entry 제거.
        set_surface_bg_override(&mut ov, SurfaceBgField::Focused, None);
        assert!(surface_bg_override(&ov, SurfaceBgField::Focused).is_none());
        assert!(
            !ov.surface_themes.contains_key("terminal"),
            "empty surface override entry pruned"
        );
    }

    /// 한 필드 clear 시 같은 entry 의 다른 필드 override 는 보존(entry 유지).
    #[test]
    fn surface_bg_clear_preserves_sibling_field() {
        let mut ov = PartialColors::default();
        let c = HexColor::from_hex("#111111").unwrap();
        set_surface_bg_override(&mut ov, SurfaceBgField::Focused, Some(c));
        set_surface_bg_override(&mut ov, SurfaceBgField::Unfocused, Some(c));

        set_surface_bg_override(&mut ov, SurfaceBgField::Focused, None);
        assert!(surface_bg_override(&ov, SurfaceBgField::Focused).is_none());
        assert_eq!(surface_bg_override(&ov, SurfaceBgField::Unfocused), Some(c));
        assert!(
            ov.surface_themes.contains_key("terminal"),
            "entry kept while sibling override remains"
        );
    }

    /// base 색은 resolved `theme_base.surface_themes["terminal"]` 에서 읽는다.
    #[test]
    fn surface_bg_base_reads_theme_base() {
        let mut base = tasty_themes::mocha_fallback_colors();
        let marker = HexColor::from_hex("#abcdef").unwrap();
        base.surface_themes
            .get_mut("terminal")
            .expect("mocha base has terminal surface")
            .focused_bg = marker;
        assert_eq!(surface_bg_base(&base, SurfaceBgField::Focused), marker);
    }
}
