//! Generated from `dtcg/tasty.tokens.json` — DO NOT EDIT.
//! 재생성: `cargo run -p tasty-design-tokens --bin generate`.
//!
//! Tier 3 (component) 치수·색 접근자. `generated::component` 의 raw
//! const 와 달리 **`&Theme` 경유** — 치수는 zoom-resolve 된 필드를
//! 반환하거나(semantic 종착) `ui_zoom` 을 직접 곱하고(primitive 직접
//! 종착), 색은 semantic 접근자 체인 또는 component→component 접근자
//! 상호 호출로 이어붙인다.

use crate::color::HexColor;
use tasty_type_geometry::length::LogicalPx;

impl crate::theme::Theme {
    /// `component.badge-bg` → `{semantic.surface-raised}`
    #[inline]
    pub fn badge_bg(&self) -> HexColor {
        self.surface_raised()
    }

    /// `component.badge-dot-size` → `{component.status-dot-size}` = 8px
    #[inline]
    pub fn badge_dot_size(&self) -> LogicalPx {
        self.status_dot_size
    }

    /// `component.badge-fg` → `{semantic.text-secondary}`
    #[inline]
    pub fn badge_fg(&self) -> HexColor {
        self.text_secondary()
    }

    /// `component.badge-font-size` → `{semantic.font-size-micro}` = 10px
    #[inline]
    pub fn badge_font_size(&self) -> LogicalPx {
        self.font_size_micro
    }

    /// `component.badge-padding-x` → `{semantic.space-xs}` = 4px
    #[inline]
    pub fn badge_padding_x(&self) -> LogicalPx {
        self.spacing_xs
    }

    /// `component.badge-radius` → `{semantic.radius-sm}` = 2px
    #[inline]
    pub fn badge_radius(&self) -> LogicalPx {
        self.corner_radius_sm
    }

    /// `component.badge-size` → `{primitive.size-16}` = 16px
    #[inline]
    pub fn badge_size(&self) -> LogicalPx {
        LogicalPx((16.0 * self.ui_zoom).round())
    }

    /// `component.banner-body-font-size` → `{semantic.font-size-caption}` = 11px
    #[inline]
    pub fn banner_body_font_size(&self) -> LogicalPx {
        self.font_size_caption
    }

    /// `component.banner-countdown-font-size` → `{semantic.font-size-micro}` = 10px
    #[inline]
    pub fn banner_countdown_font_size(&self) -> LogicalPx {
        self.font_size_micro
    }

    /// `component.banner-gap` → `{semantic.space-md}` = 12px
    #[inline]
    pub fn banner_gap(&self) -> LogicalPx {
        self.spacing_md
    }

    /// `component.banner-margin` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn banner_margin(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.banner-padding-x` → `{semantic.space-md}` = 12px
    #[inline]
    pub fn banner_padding_x(&self) -> LogicalPx {
        self.spacing_md
    }

    /// `component.banner-padding-y` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn banner_padding_y(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.banner-radius` → `{primitive.radius-8}` = 8px
    #[inline]
    pub fn banner_radius(&self) -> LogicalPx {
        LogicalPx((8.0 * self.ui_zoom).round())
    }

    /// `component.banner-title-font-size` → `{semantic.font-size-body}` = 13px
    #[inline]
    pub fn banner_title_font_size(&self) -> LogicalPx {
        self.font_size_body
    }

    /// `component.button-agent-bg` → `{semantic.accent-agent}`
    #[inline]
    pub fn button_agent_bg(&self) -> HexColor {
        self.accent_agent()
    }

    /// `component.button-agent-fg` → `{semantic.text-on-accent}`
    #[inline]
    pub fn button_agent_fg(&self) -> HexColor {
        self.text_on_accent()
    }

    /// `component.button-danger-bg` → `{semantic.accent-danger}`
    #[inline]
    pub fn button_danger_bg(&self) -> HexColor {
        self.accent_danger()
    }

    /// `component.button-danger-fg` → `{semantic.text-on-accent}`
    #[inline]
    pub fn button_danger_fg(&self) -> HexColor {
        self.text_on_accent()
    }

    /// `component.button-fg` → `{semantic.text-primary}`
    #[inline]
    pub fn button_fg(&self) -> HexColor {
        self.text_primary()
    }

    /// `component.button-font-size` → `{semantic.font-size-body}` = 13px
    #[inline]
    pub fn button_font_size(&self) -> LogicalPx {
        self.font_size_body
    }

    /// `component.button-gap` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn button_gap(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.button-ghost-fg` → `{semantic.text-secondary}`
    #[inline]
    pub fn button_ghost_fg(&self) -> HexColor {
        self.text_secondary()
    }

    /// `component.button-ghost-fg-hover` → `{semantic.text-primary}`
    #[inline]
    pub fn button_ghost_fg_hover(&self) -> HexColor {
        self.text_primary()
    }

    /// `component.button-height` → `{semantic.control-height}` = 28px
    #[inline]
    pub fn button_height(&self) -> LogicalPx {
        self.item_height_interactive
    }

    /// `component.button-height-lg` → `{primitive.size-32}` = 32px
    #[inline]
    pub fn button_height_lg(&self) -> LogicalPx {
        LogicalPx((32.0 * self.ui_zoom).round())
    }

    /// `component.button-height-sm` → `{semantic.control-height-tab}` = 24px
    #[inline]
    pub fn button_height_sm(&self) -> LogicalPx {
        self.item_height_tab
    }

    /// `component.button-overlay-active` → `{semantic.overlay-active}`
    #[inline]
    pub fn button_overlay_active(&self) -> HexColor {
        self.overlay_active()
    }

    /// `component.button-overlay-hover` → `{semantic.overlay-hover}`
    #[inline]
    pub fn button_overlay_hover(&self) -> HexColor {
        self.overlay_hover()
    }

    /// `component.button-padding-x` → `{semantic.space-md}` = 12px
    #[inline]
    pub fn button_padding_x(&self) -> LogicalPx {
        self.spacing_md
    }

    /// `component.button-primary-bg` → `{semantic.accent-primary}`
    #[inline]
    pub fn button_primary_bg(&self) -> HexColor {
        self.accent_primary()
    }

    /// `component.button-primary-fg` → `{semantic.text-on-accent}`
    #[inline]
    pub fn button_primary_fg(&self) -> HexColor {
        self.text_on_accent()
    }

    /// `component.button-radius` → `{semantic.radius}` = 4px
    #[inline]
    pub fn button_radius(&self) -> LogicalPx {
        self.corner_radius
    }

    /// `component.button-secondary-bg` → `{semantic.surface-raised}`
    #[inline]
    pub fn button_secondary_bg(&self) -> HexColor {
        self.surface_raised()
    }

    /// `component.button-secondary-border` → `{semantic.border-default}`
    #[inline]
    pub fn button_secondary_border(&self) -> HexColor {
        self.border_default()
    }

    /// `component.button-secondary-border-hover` → `{semantic.border-strong}`
    #[inline]
    pub fn button_secondary_border_hover(&self) -> HexColor {
        self.border_strong()
    }

    /// `component.checkbox-bg` → `{semantic.surface-raised}`
    #[inline]
    pub fn checkbox_bg(&self) -> HexColor {
        self.surface_raised()
    }

    /// `component.checkbox-bg-checked` → `{semantic.accent-primary}`
    #[inline]
    pub fn checkbox_bg_checked(&self) -> HexColor {
        self.accent_primary()
    }

    /// `component.checkbox-border` → `{semantic.border-strong}`
    #[inline]
    pub fn checkbox_border(&self) -> HexColor {
        self.border_strong()
    }

    /// `component.checkbox-border-focus` → `{semantic.border-focus}`
    #[inline]
    pub fn checkbox_border_focus(&self) -> HexColor {
        self.border_focus()
    }

    /// `component.checkbox-check-fg` → `{semantic.text-on-accent}`
    #[inline]
    pub fn checkbox_check_fg(&self) -> HexColor {
        self.text_on_accent()
    }

    /// `component.checkbox-radius` → `{semantic.radius-sm}` = 2px
    #[inline]
    pub fn checkbox_radius(&self) -> LogicalPx {
        self.corner_radius_sm
    }

    /// `component.checkbox-size` → `{primitive.size-16}` = 16px
    #[inline]
    pub fn checkbox_size(&self) -> LogicalPx {
        LogicalPx((16.0 * self.ui_zoom).round())
    }

    /// `component.help-hint-color` → `{semantic.text-muted}`
    #[inline]
    pub fn help_hint_color(&self) -> HexColor {
        self.text_muted()
    }

    /// `component.help-hint-color-hover` → `{semantic.text-secondary}`
    #[inline]
    pub fn help_hint_color_hover(&self) -> HexColor {
        self.text_secondary()
    }

    /// `component.help-hint-gap` → `{semantic.space-xs}` = 4px
    #[inline]
    pub fn help_hint_gap(&self) -> LogicalPx {
        self.spacing_xs
    }

    /// `component.help-hint-size` → `{semantic.icon-size-sm}` = 14px
    #[inline]
    pub fn help_hint_size(&self) -> LogicalPx {
        self.icon_glyph_size_sm
    }

    /// `component.icon-button-bg-active` → `{semantic.overlay-active}`
    #[inline]
    pub fn icon_button_bg_active(&self) -> HexColor {
        self.overlay_active()
    }

    /// `component.icon-button-fg` → `{semantic.text-secondary}`
    #[inline]
    pub fn icon_button_fg(&self) -> HexColor {
        self.text_secondary()
    }

    /// `component.icon-button-fg-hover` → `{semantic.text-primary}`
    #[inline]
    pub fn icon_button_fg_hover(&self) -> HexColor {
        self.text_primary()
    }

    /// `component.icon-button-overlay-hover` → `{semantic.overlay-hover}`
    #[inline]
    pub fn icon_button_overlay_hover(&self) -> HexColor {
        self.overlay_hover()
    }

    /// `component.icon-button-radius` → `{semantic.radius}` = 4px
    #[inline]
    pub fn icon_button_radius(&self) -> LogicalPx {
        self.corner_radius
    }

    /// `component.icon-button-size` → `{semantic.control-height}` = 28px
    #[inline]
    pub fn icon_button_size(&self) -> LogicalPx {
        self.item_height_interactive
    }

    /// `component.icon-button-size-sm` → `{primitive.size-24}` = 24px
    #[inline]
    pub fn icon_button_size_sm(&self) -> LogicalPx {
        LogicalPx((24.0 * self.ui_zoom).round())
    }

    /// `component.input-bg` → `{semantic.surface-raised}`
    #[inline]
    pub fn input_bg(&self) -> HexColor {
        self.surface_raised()
    }

    /// `component.input-border` → `{semantic.border-default}`
    #[inline]
    pub fn input_border(&self) -> HexColor {
        self.border_default()
    }

    /// `component.input-border-focus` → `{semantic.border-focus}`
    #[inline]
    pub fn input_border_focus(&self) -> HexColor {
        self.border_focus()
    }

    /// `component.input-border-invalid` → `{semantic.accent-danger}`
    #[inline]
    pub fn input_border_invalid(&self) -> HexColor {
        self.accent_danger()
    }

    /// `component.input-fg` → `{semantic.text-primary}`
    #[inline]
    pub fn input_fg(&self) -> HexColor {
        self.text_primary()
    }

    /// `component.input-font-size` → `{semantic.font-size-body}` = 13px
    #[inline]
    pub fn input_font_size(&self) -> LogicalPx {
        self.font_size_body
    }

    /// `component.input-gap` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn input_gap(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.input-height` → `{semantic.control-height}` = 28px
    #[inline]
    pub fn input_height(&self) -> LogicalPx {
        self.item_height_interactive
    }

    /// `component.input-icon-fg` → `{semantic.text-muted}`
    #[inline]
    pub fn input_icon_fg(&self) -> HexColor {
        self.text_muted()
    }

    /// `component.input-padding-x` → `{semantic.space-md}` = 12px
    #[inline]
    pub fn input_padding_x(&self) -> LogicalPx {
        self.spacing_md
    }

    /// `component.input-placeholder` → `{semantic.text-placeholder}`
    #[inline]
    pub fn input_placeholder(&self) -> HexColor {
        self.text_placeholder()
    }

    /// `component.input-radius` → `{semantic.radius}` = 4px
    #[inline]
    pub fn input_radius(&self) -> LogicalPx {
        self.corner_radius
    }

    /// `component.kbd-bg` → `{semantic.surface-raised}`
    #[inline]
    pub fn kbd_bg(&self) -> HexColor {
        self.surface_raised()
    }

    /// `component.kbd-border` → `{semantic.border-strong}`
    #[inline]
    pub fn kbd_border(&self) -> HexColor {
        self.border_strong()
    }

    /// `component.kbd-fg` → `{semantic.text-secondary}`
    #[inline]
    pub fn kbd_fg(&self) -> HexColor {
        self.text_secondary()
    }

    /// `component.kbd-font-size` → `{semantic.font-size-micro}` = 10px
    #[inline]
    pub fn kbd_font_size(&self) -> LogicalPx {
        self.font_size_micro
    }

    /// `component.kbd-gap` → `{primitive.size-3}` = 3px
    #[inline]
    pub fn kbd_gap(&self) -> LogicalPx {
        LogicalPx((3.0 * self.ui_zoom).round())
    }

    /// `component.kbd-padding-x` → `{semantic.space-xs}` = 4px
    #[inline]
    pub fn kbd_padding_x(&self) -> LogicalPx {
        self.spacing_xs
    }

    /// `component.kbd-radius` → `{semantic.radius-sm}` = 2px
    #[inline]
    pub fn kbd_radius(&self) -> LogicalPx {
        self.corner_radius_sm
    }

    /// `component.kbd-shadow-depth` → `{primitive.size-2}` = 2px
    #[inline]
    pub fn kbd_shadow_depth(&self) -> LogicalPx {
        LogicalPx((2.0 * self.ui_zoom).round())
    }

    /// `component.kbd-size` → `{primitive.size-16}` = 16px
    #[inline]
    pub fn kbd_size(&self) -> LogicalPx {
        LogicalPx((16.0 * self.ui_zoom).round())
    }

    /// `component.menu-bg` → `{semantic.surface-raised}`
    #[inline]
    pub fn menu_bg(&self) -> HexColor {
        self.surface_raised()
    }

    /// `component.menu-border` → `{semantic.border-default}`
    #[inline]
    pub fn menu_border(&self) -> HexColor {
        self.border_default()
    }

    /// `component.menu-item-bg-hover` → `{semantic.overlay-hover}`
    #[inline]
    pub fn menu_item_bg_hover(&self) -> HexColor {
        self.overlay_hover()
    }

    /// `component.menu-item-fg` → `{semantic.text-secondary}`
    #[inline]
    pub fn menu_item_fg(&self) -> HexColor {
        self.text_secondary()
    }

    /// `component.menu-item-fg-hover` → `{semantic.text-primary}`
    #[inline]
    pub fn menu_item_fg_hover(&self) -> HexColor {
        self.text_primary()
    }

    /// `component.menu-item-height` → `{semantic.control-height}` = 28px
    #[inline]
    pub fn menu_item_height(&self) -> LogicalPx {
        self.item_height_interactive
    }

    /// `component.menu-item-padding-x` → `{semantic.space-md}` = 12px
    #[inline]
    pub fn menu_item_padding_x(&self) -> LogicalPx {
        self.spacing_md
    }

    /// `component.menu-item-radius` → `{semantic.radius-sm}` = 2px
    #[inline]
    pub fn menu_item_radius(&self) -> LogicalPx {
        self.corner_radius_sm
    }

    /// `component.menu-item-shortcut-font-size` → `{semantic.font-size-micro}` = 10px
    #[inline]
    pub fn menu_item_shortcut_font_size(&self) -> LogicalPx {
        self.font_size_micro
    }

    /// `component.menu-radius` → `{semantic.radius}` = 4px
    #[inline]
    pub fn menu_radius(&self) -> LogicalPx {
        self.corner_radius
    }

    /// `component.plugins-list-width` → `{primitive.size-288}` = 288px
    #[inline]
    pub fn plugins_list_width(&self) -> LogicalPx {
        LogicalPx((288.0 * self.ui_zoom).round())
    }

    /// `component.remote-label-col` → `{primitive.size-112}` = 112px
    #[inline]
    pub fn remote_label_col(&self) -> LogicalPx {
        LogicalPx((112.0 * self.ui_zoom).round())
    }

    /// `component.select-bg` → `{semantic.surface-raised}`
    #[inline]
    pub fn select_bg(&self) -> HexColor {
        self.surface_raised()
    }

    /// `component.select-border` → `{semantic.border-default}`
    #[inline]
    pub fn select_border(&self) -> HexColor {
        self.border_default()
    }

    /// `component.select-border-focus` → `{semantic.border-focus}`
    #[inline]
    pub fn select_border_focus(&self) -> HexColor {
        self.border_focus()
    }

    /// `component.select-chevron-fg` → `{semantic.text-muted}`
    #[inline]
    pub fn select_chevron_fg(&self) -> HexColor {
        self.text_muted()
    }

    /// `component.select-chevron-offset` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn select_chevron_offset(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.select-chevron-room` → `{primitive.size-28}` = 28px
    #[inline]
    pub fn select_chevron_room(&self) -> LogicalPx {
        LogicalPx((28.0 * self.ui_zoom).round())
    }

    /// `component.select-fg` → `{semantic.text-primary}`
    #[inline]
    pub fn select_fg(&self) -> HexColor {
        self.text_primary()
    }

    /// `component.select-font-size` → `{semantic.font-size-body}` = 13px
    #[inline]
    pub fn select_font_size(&self) -> LogicalPx {
        self.font_size_body
    }

    /// `component.select-height` → `{semantic.control-height}` = 28px
    #[inline]
    pub fn select_height(&self) -> LogicalPx {
        self.item_height_interactive
    }

    /// `component.select-padding-x` → `{semantic.space-md}` = 12px
    #[inline]
    pub fn select_padding_x(&self) -> LogicalPx {
        self.spacing_md
    }

    /// `component.select-radius` → `{semantic.radius}` = 4px
    #[inline]
    pub fn select_radius(&self) -> LogicalPx {
        self.corner_radius
    }

    /// `component.settings-row-min-height` → `{primitive.size-32}` = 32px
    #[inline]
    pub fn settings_row_min_height(&self) -> LogicalPx {
        LogicalPx((32.0 * self.ui_zoom).round())
    }

    /// `component.settings-sidebar-width` → `{primitive.size-200}` = 200px
    #[inline]
    pub fn settings_sidebar_width(&self) -> LogicalPx {
        LogicalPx((200.0 * self.ui_zoom).round())
    }

    /// `component.sidebar-button-label-font-size` → `{semantic.font-size-caption}` = 11px
    #[inline]
    pub fn sidebar_button_label_font_size(&self) -> LogicalPx {
        self.sidebar_button_label_font_size
    }

    /// `component.sidebar-collapsed-icon-height` → `{primitive.size-22}` = 22px
    #[inline]
    pub fn sidebar_collapsed_icon_height(&self) -> LogicalPx {
        self.sidebar_collapsed_icon_height
    }

    /// `component.sidebar-collapsed-slot-width` → `{primitive.size-32}` = 32px
    #[inline]
    pub fn sidebar_collapsed_slot_width(&self) -> LogicalPx {
        self.sidebar_collapsed_slot_width
    }

    /// `component.sidebar-collapsed-workspace-height` → `{primitive.size-28}` = 28px
    #[inline]
    pub fn sidebar_collapsed_workspace_height(&self) -> LogicalPx {
        self.sidebar_collapsed_workspace_height
    }

    /// `component.sidebar-logo-collapsed-size` → `{primitive.size-24}` = 24px
    #[inline]
    pub fn sidebar_logo_collapsed_size(&self) -> LogicalPx {
        self.sidebar_logo_collapsed_size
    }

    /// `component.sidebar-logo-size` → `{primitive.size-22}` = 22px
    #[inline]
    pub fn sidebar_logo_size(&self) -> LogicalPx {
        self.sidebar_logo_size
    }

    /// `component.sidebar-section-heading-font-size` → `{semantic.font-size-micro}` = 10px
    #[inline]
    pub fn sidebar_section_heading_font_size(&self) -> LogicalPx {
        self.sidebar_section_heading_font_size
    }

    /// `component.sidebar-wordmark-font-size` → `{semantic.font-size-brand-wordmark}` = 17px
    #[inline]
    pub fn sidebar_wordmark_font_size(&self) -> LogicalPx {
        self.sidebar_wordmark_font_size
    }

    /// `component.spinner-indicator` → `{semantic.accent-primary}`
    #[inline]
    pub fn spinner_indicator(&self) -> HexColor {
        self.accent_primary()
    }

    /// `component.spinner-size` → `{primitive.size-16}` = 16px
    #[inline]
    pub fn spinner_size(&self) -> LogicalPx {
        self.spinner_size
    }

    /// `component.spinner-track` → `{semantic.surface-active}`
    #[inline]
    pub fn spinner_track(&self) -> HexColor {
        self.surface_active()
    }

    /// `component.status-dot-agent` → `{semantic.accent-agent}`
    #[inline]
    pub fn status_dot_agent(&self) -> HexColor {
        self.accent_agent()
    }

    /// `component.status-dot-attached-ring` → `{semantic.accent-attached}`
    #[inline]
    pub fn status_dot_attached_ring(&self) -> HexColor {
        self.border_attached()
    }

    /// `component.status-dot-attached-ring-offset` → `{primitive.size-2}` = 2px
    #[inline]
    pub fn status_dot_attached_ring_offset(&self) -> LogicalPx {
        LogicalPx((2.0 * self.ui_zoom).round())
    }

    /// `component.status-dot-attached-ring-width` → `{primitive.size-2}` = 2px
    #[inline]
    pub fn status_dot_attached_ring_width(&self) -> LogicalPx {
        LogicalPx((2.0 * self.ui_zoom).round())
    }

    /// `component.status-dot-danger` → `{semantic.accent-danger}`
    #[inline]
    pub fn status_dot_danger(&self) -> HexColor {
        self.accent_danger()
    }

    /// `component.status-dot-info` → `{semantic.accent-info}`
    #[inline]
    pub fn status_dot_info(&self) -> HexColor {
        self.accent_info()
    }

    /// `component.status-dot-size` → `{primitive.size-8}` = 8px
    #[inline]
    pub fn status_dot_size(&self) -> LogicalPx {
        self.status_dot_size
    }

    /// `component.status-dot-success` → `{semantic.accent-success}`
    #[inline]
    pub fn status_dot_success(&self) -> HexColor {
        self.accent_success()
    }

    /// `component.status-dot-warning` → `{semantic.accent-warning}`
    #[inline]
    pub fn status_dot_warning(&self) -> HexColor {
        self.accent_warning()
    }

    /// `component.swatch-radius` → `{semantic.radius-sm}` = 2px
    #[inline]
    pub fn swatch_radius(&self) -> LogicalPx {
        self.corner_radius_sm
    }

    /// `component.swatch-size` → `{primitive.size-16}` = 16px
    #[inline]
    pub fn swatch_size(&self) -> LogicalPx {
        LogicalPx((16.0 * self.ui_zoom).round())
    }

    /// `component.switch-overlay-active-bg` → `{semantic.accent-primary}`
    #[inline]
    pub fn switch_overlay_active_bg(&self) -> HexColor {
        self.accent_primary()
    }

    /// `component.switch-overlay-active-fg` → `{semantic.text-on-accent}`
    #[inline]
    pub fn switch_overlay_active_fg(&self) -> HexColor {
        self.text_on_accent()
    }

    /// `component.switch-overlay-bg` → `{component.kbd-bg}`
    #[inline]
    pub fn switch_overlay_bg(&self) -> HexColor {
        self.kbd_bg()
    }

    /// `component.switch-overlay-border` → `{component.kbd-border}`
    #[inline]
    pub fn switch_overlay_border(&self) -> HexColor {
        self.kbd_border()
    }

    /// `component.switch-overlay-fg` → `{component.kbd-fg}`
    #[inline]
    pub fn switch_overlay_fg(&self) -> HexColor {
        self.kbd_fg()
    }

    /// `component.switch-overlay-shadow-depth` → `{component.kbd-shadow-depth}` = 2px
    #[inline]
    pub fn switch_overlay_shadow_depth(&self) -> LogicalPx {
        self.kbd_shadow_depth()
    }

    /// `component.switch-overlay-size` → `{component.kbd-size}` = 16px
    #[inline]
    pub fn switch_overlay_size(&self) -> LogicalPx {
        self.kbd_size()
    }

    /// `component.switch-radius` → `{semantic.radius-pill}` = 9999px (sentinel — 완전 원형용 상한값)
    #[inline]
    pub fn switch_radius(&self) -> LogicalPx {
        LogicalPx((9999.0 * self.ui_zoom).round())
    }

    /// `component.switch-thumb-bg` → `{semantic.text-muted}`
    #[inline]
    pub fn switch_thumb_bg(&self) -> HexColor {
        self.text_muted()
    }

    /// `component.switch-thumb-bg-on` → `{semantic.text-on-accent}`
    #[inline]
    pub fn switch_thumb_bg_on(&self) -> HexColor {
        self.text_on_accent()
    }

    /// `component.switch-thumb-inset` → `{primitive.size-2}` = 2px
    #[inline]
    pub fn switch_thumb_inset(&self) -> LogicalPx {
        LogicalPx((2.0 * self.ui_zoom).round())
    }

    /// `component.switch-thumb-size` → `{primitive.size-12}` = 12px
    #[inline]
    pub fn switch_thumb_size(&self) -> LogicalPx {
        LogicalPx((12.0 * self.ui_zoom).round())
    }

    /// `component.switch-thumb-travel` → `{primitive.size-12}` = 12px
    #[inline]
    pub fn switch_thumb_travel(&self) -> LogicalPx {
        LogicalPx((12.0 * self.ui_zoom).round())
    }

    /// `component.switch-track-bg` → `{semantic.surface-active}`
    #[inline]
    pub fn switch_track_bg(&self) -> HexColor {
        self.surface_active()
    }

    /// `component.switch-track-bg-on` → `{semantic.accent-primary}`
    #[inline]
    pub fn switch_track_bg_on(&self) -> HexColor {
        self.accent_primary()
    }

    /// `component.switch-track-height` → `{primitive.size-16}` = 16px
    #[inline]
    pub fn switch_track_height(&self) -> LogicalPx {
        LogicalPx((16.0 * self.ui_zoom).round())
    }

    /// `component.switch-track-width` → `{primitive.size-28}` = 28px
    #[inline]
    pub fn switch_track_width(&self) -> LogicalPx {
        LogicalPx((28.0 * self.ui_zoom).round())
    }

    /// `component.tab-bg` → `{semantic.bg-sidebar}`
    #[inline]
    pub fn tab_bg(&self) -> HexColor {
        self.bg_sidebar()
    }

    /// `component.tab-bg-active` → `{semantic.bg-panel}`
    #[inline]
    pub fn tab_bg_active(&self) -> HexColor {
        self.bg_panel()
    }

    /// `component.tab-close-radius` → `{semantic.radius-sm}` = 2px
    #[inline]
    pub fn tab_close_radius(&self) -> LogicalPx {
        self.corner_radius_sm
    }

    /// `component.tab-close-size` → `{primitive.size-16}` = 16px
    #[inline]
    pub fn tab_close_size(&self) -> LogicalPx {
        LogicalPx((16.0 * self.ui_zoom).round())
    }

    /// `component.tab-dot-size` → `{component.status-dot-size}` = 8px
    #[inline]
    pub fn tab_dot_size(&self) -> LogicalPx {
        self.status_dot_size
    }

    /// `component.tab-fg` → `{semantic.text-muted}`
    #[inline]
    pub fn tab_fg(&self) -> HexColor {
        self.text_muted()
    }

    /// `component.tab-fg-active` → `{semantic.text-primary}`
    #[inline]
    pub fn tab_fg_active(&self) -> HexColor {
        self.text_primary()
    }

    /// `component.tab-fg-hover` → `{semantic.text-secondary}`
    #[inline]
    pub fn tab_fg_hover(&self) -> HexColor {
        self.text_secondary()
    }

    /// `component.tab-gap` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn tab_gap(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.tab-height` → `{semantic.control-height-tab}` = 24px
    #[inline]
    pub fn tab_height(&self) -> LogicalPx {
        self.item_height_tab
    }

    /// `component.tab-icon-size` → `{semantic.icon-size-sm}` = 14px
    #[inline]
    pub fn tab_icon_size(&self) -> LogicalPx {
        self.icon_glyph_size_sm
    }

    /// `component.tab-indicator` → `{semantic.accent-primary}`
    #[inline]
    pub fn tab_indicator(&self) -> HexColor {
        self.accent_primary()
    }

    /// `component.tab-indicator-width` → `{primitive.size-2}` = 2px
    #[inline]
    pub fn tab_indicator_width(&self) -> LogicalPx {
        self.tab_indicator_width
    }

    /// `component.tab-padding-x` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn tab_padding_x(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.tab-separator` → `{semantic.separator}`
    #[inline]
    pub fn tab_separator(&self) -> HexColor {
        self.separator
    }

    /// `component.tab-strip-width` → `{semantic.tab-width}` = 150px
    #[inline]
    pub fn tab_strip_width(&self) -> LogicalPx {
        self.tab_width
    }

    /// `component.table-border` → `{semantic.separator}`
    #[inline]
    pub fn table_border(&self) -> HexColor {
        self.separator
    }

    /// `component.table-cell-height` → `{semantic.control-height}` = 28px
    #[inline]
    pub fn table_cell_height(&self) -> LogicalPx {
        self.item_height_interactive
    }

    /// `component.table-cell-padding-x` → `{semantic.space-md}` = 12px
    #[inline]
    pub fn table_cell_padding_x(&self) -> LogicalPx {
        self.spacing_md
    }

    /// `component.table-cell-padding-y` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn table_cell_padding_y(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.table-font-size` → `{semantic.font-size-body}` = 13px
    #[inline]
    pub fn table_font_size(&self) -> LogicalPx {
        self.font_size_body
    }

    /// `component.table-header-bg` → `{semantic.bg-sidebar}`
    #[inline]
    pub fn table_header_bg(&self) -> HexColor {
        self.bg_sidebar()
    }

    /// `component.table-header-fg` → `{semantic.text-muted}`
    #[inline]
    pub fn table_header_fg(&self) -> HexColor {
        self.text_muted()
    }

    /// `component.table-header-font-size` → `{semantic.font-size-caption}` = 11px
    #[inline]
    pub fn table_header_font_size(&self) -> LogicalPx {
        self.font_size_caption
    }

    /// `component.table-row-bg-hover` → `{semantic.overlay-hover}`
    #[inline]
    pub fn table_row_bg_hover(&self) -> HexColor {
        self.overlay_hover()
    }

    /// `component.table-row-bg-selected` → `{semantic.surface-active}`
    #[inline]
    pub fn table_row_bg_selected(&self) -> HexColor {
        self.surface_active()
    }

    /// `component.table-row-fg` → `{semantic.text-secondary}`
    #[inline]
    pub fn table_row_fg(&self) -> HexColor {
        self.text_secondary()
    }

    /// `component.tag-bg` → `{semantic.surface-raised}`
    #[inline]
    pub fn tag_bg(&self) -> HexColor {
        self.surface_raised()
    }

    /// `component.tag-border` → `{semantic.border-default}`
    #[inline]
    pub fn tag_border(&self) -> HexColor {
        self.border_default()
    }

    /// `component.tag-dot-size` → `{primitive.size-8}` = 8px
    #[inline]
    pub fn tag_dot_size(&self) -> LogicalPx {
        LogicalPx((8.0 * self.ui_zoom).round())
    }

    /// `component.tag-fg` → `{semantic.text-secondary}`
    #[inline]
    pub fn tag_fg(&self) -> HexColor {
        self.text_secondary()
    }

    /// `component.tag-font-size` → `{semantic.font-size-micro}` = 10px
    #[inline]
    pub fn tag_font_size(&self) -> LogicalPx {
        self.font_size_micro
    }

    /// `component.tag-gap` → `{semantic.space-xs}` = 4px
    #[inline]
    pub fn tag_gap(&self) -> LogicalPx {
        self.spacing_xs
    }

    /// `component.tag-padding-x` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn tag_padding_x(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.tag-radius` → `{semantic.radius-sm}` = 2px
    #[inline]
    pub fn tag_radius(&self) -> LogicalPx {
        self.corner_radius_sm
    }

    /// `component.tag-size` → `{primitive.size-16}` = 16px
    #[inline]
    pub fn tag_size(&self) -> LogicalPx {
        LogicalPx((16.0 * self.ui_zoom).round())
    }

    /// `component.titlebar-button-active-bg` → `{semantic.overlay-active}`
    #[inline]
    pub fn titlebar_button_active_bg(&self) -> HexColor {
        self.overlay_active()
    }

    /// `component.titlebar-button-fg` → `{semantic.text-muted}`
    #[inline]
    pub fn titlebar_button_fg(&self) -> HexColor {
        self.text_muted()
    }

    /// `component.titlebar-button-fg-hover` → `{semantic.text-primary}`
    #[inline]
    pub fn titlebar_button_fg_hover(&self) -> HexColor {
        self.text_primary()
    }

    /// `component.titlebar-button-hover-bg` → `{semantic.overlay-hover}`
    #[inline]
    pub fn titlebar_button_hover_bg(&self) -> HexColor {
        self.overlay_hover()
    }

    /// `component.titlebar-caption-width` → `{primitive.size-46}` = 46px
    #[inline]
    pub fn titlebar_caption_width(&self) -> LogicalPx {
        self.caption_width
    }

    /// `component.titlebar-close-hover-bg` → `{semantic.accent-window-close}`
    #[inline]
    pub fn titlebar_close_hover_bg(&self) -> HexColor {
        self.accent_window_close()
    }

    /// `component.titlebar-close-hover-fg` → `{semantic.text-on-window-close}`
    #[inline]
    pub fn titlebar_close_hover_fg(&self) -> HexColor {
        self.text_on_window_close()
    }

    /// `component.titlebar-csd-border` → `{semantic.border-strong}`
    #[inline]
    pub fn titlebar_csd_border(&self) -> HexColor {
        self.border_strong()
    }

    /// `component.titlebar-csd-radius` → `{primitive.radius-8}` = 8px
    #[inline]
    pub fn titlebar_csd_radius(&self) -> LogicalPx {
        LogicalPx((8.0 * self.ui_zoom).round())
    }

    /// `component.titlebar-csd-shadow-margin` → `{primitive.size-8}` = 8px
    #[inline]
    pub fn titlebar_csd_shadow_margin(&self) -> LogicalPx {
        LogicalPx((8.0 * self.ui_zoom).round())
    }

    /// `component.titlebar-traffic-close` → `{semantic.accent-macos-close}`
    #[inline]
    pub fn titlebar_traffic_close(&self) -> HexColor {
        self.accent_macos_close()
    }

    /// `component.titlebar-traffic-inactive` → `{semantic.surface-active}`
    #[inline]
    pub fn titlebar_traffic_inactive(&self) -> HexColor {
        self.surface_active()
    }

    /// `component.titlebar-traffic-min` → `{semantic.accent-macos-min}`
    #[inline]
    pub fn titlebar_traffic_min(&self) -> HexColor {
        self.accent_macos_min()
    }

    /// `component.titlebar-traffic-size` → `{primitive.size-12}` = 12px
    #[inline]
    pub fn titlebar_traffic_size(&self) -> LogicalPx {
        self.traffic_size
    }

    /// `component.titlebar-traffic-zoom` → `{semantic.accent-macos-zoom}`
    #[inline]
    pub fn titlebar_traffic_zoom(&self) -> HexColor {
        self.accent_macos_zoom()
    }

    /// `component.titlebar-window-button-size` → `{primitive.size-24}` = 24px
    #[inline]
    pub fn titlebar_window_button_size(&self) -> LogicalPx {
        self.window_button_size
    }

    /// `component.toast-accent-agent` → `{semantic.accent-agent}`
    #[inline]
    pub fn toast_accent_agent(&self) -> HexColor {
        self.accent_agent()
    }

    /// `component.toast-accent-danger` → `{semantic.accent-danger}`
    #[inline]
    pub fn toast_accent_danger(&self) -> HexColor {
        self.accent_danger()
    }

    /// `component.toast-accent-info` → `{semantic.accent-info}`
    #[inline]
    pub fn toast_accent_info(&self) -> HexColor {
        self.accent_info()
    }

    /// `component.toast-accent-success` → `{semantic.accent-success}`
    #[inline]
    pub fn toast_accent_success(&self) -> HexColor {
        self.accent_success()
    }

    /// `component.toast-accent-warning` → `{semantic.accent-warning}`
    #[inline]
    pub fn toast_accent_warning(&self) -> HexColor {
        self.accent_warning()
    }

    /// `component.toast-accent-width` → `{primitive.size-3}` = 3px
    #[inline]
    pub fn toast_accent_width(&self) -> LogicalPx {
        self.toast_accent_width
    }

    /// `component.toast-bg` → `{semantic.surface-raised}`
    #[inline]
    pub fn toast_bg(&self) -> HexColor {
        self.surface_raised()
    }

    /// `component.toast-border` → `{semantic.border-default}`
    #[inline]
    pub fn toast_border(&self) -> HexColor {
        self.border_default()
    }

    /// `component.toast-fg` → `{semantic.text-primary}`
    #[inline]
    pub fn toast_fg(&self) -> HexColor {
        self.text_primary()
    }

    /// `component.toast-gap` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn toast_gap(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.toast-hint-font-size` → `{semantic.font-size-micro}` = 10px
    #[inline]
    pub fn toast_hint_font_size(&self) -> LogicalPx {
        self.font_size_micro
    }

    /// `component.toast-max-width` → `{primitive.size-320}` = 320px
    #[inline]
    pub fn toast_max_width(&self) -> LogicalPx {
        self.toast_max_width
    }

    /// `component.toast-min-height` → `{semantic.control-height}` = 28px
    #[inline]
    pub fn toast_min_height(&self) -> LogicalPx {
        self.item_height_interactive
    }

    /// `component.toast-padding-x` → `{semantic.space-md}` = 12px
    #[inline]
    pub fn toast_padding_x(&self) -> LogicalPx {
        self.spacing_md
    }

    /// `component.toast-padding-y` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn toast_padding_y(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.toast-radius` → `{semantic.radius}` = 4px
    #[inline]
    pub fn toast_radius(&self) -> LogicalPx {
        self.corner_radius
    }

    /// `component.tooltip-bg` → `{semantic.surface-raised}`
    #[inline]
    pub fn tooltip_bg(&self) -> HexColor {
        self.surface_raised()
    }

    /// `component.tooltip-border` → `{semantic.border-strong}`
    #[inline]
    pub fn tooltip_border(&self) -> HexColor {
        self.border_strong()
    }

    /// `component.tooltip-fg` → `{semantic.text-secondary}`
    #[inline]
    pub fn tooltip_fg(&self) -> HexColor {
        self.text_secondary()
    }

    /// `component.tooltip-font-size` → `{semantic.font-size-caption}` = 11px
    #[inline]
    pub fn tooltip_font_size(&self) -> LogicalPx {
        self.font_size_caption
    }

    /// `component.tooltip-max-width` → `{primitive.size-240}` = 240px
    #[inline]
    pub fn tooltip_max_width(&self) -> LogicalPx {
        LogicalPx((240.0 * self.ui_zoom).round())
    }

    /// `component.tooltip-offset` → `{semantic.space-xs}` = 4px
    #[inline]
    pub fn tooltip_offset(&self) -> LogicalPx {
        self.spacing_xs
    }

    /// `component.tooltip-padding-x` → `{semantic.space-sm}` = 8px
    #[inline]
    pub fn tooltip_padding_x(&self) -> LogicalPx {
        self.spacing_sm
    }

    /// `component.tooltip-padding-y` → `{semantic.space-xs}` = 4px
    #[inline]
    pub fn tooltip_padding_y(&self) -> LogicalPx {
        self.spacing_xs
    }

    /// `component.tooltip-radius` → `{semantic.radius}` = 4px
    #[inline]
    pub fn tooltip_radius(&self) -> LogicalPx {
        self.corner_radius
    }

    /// `component.tree-row-bg-active` → `{semantic.surface-active}`
    #[inline]
    pub fn tree_row_bg_active(&self) -> HexColor {
        self.surface_active()
    }

    /// `component.tree-row-bg-hover` → `{semantic.overlay-hover}`
    #[inline]
    pub fn tree_row_bg_hover(&self) -> HexColor {
        self.overlay_hover()
    }

    /// `component.tree-row-fg` → `{semantic.text-secondary}`
    #[inline]
    pub fn tree_row_fg(&self) -> HexColor {
        self.text_secondary()
    }

    /// `component.tree-row-fg-active` → `{semantic.text-primary}`
    #[inline]
    pub fn tree_row_fg_active(&self) -> HexColor {
        self.text_primary()
    }

    /// `component.tree-row-font-size` → `{semantic.font-size-body}` = 13px
    #[inline]
    pub fn tree_row_font_size(&self) -> LogicalPx {
        self.font_size_body
    }

    /// `component.tree-row-gap` → `{semantic.space-xs}` = 4px
    #[inline]
    pub fn tree_row_gap(&self) -> LogicalPx {
        self.spacing_xs
    }

    /// `component.tree-row-height` → `{semantic.control-height-tree}` = 22px
    #[inline]
    pub fn tree_row_height(&self) -> LogicalPx {
        self.item_height_tree
    }

    /// `component.tree-row-indent` → `{semantic.space-md}` = 12px
    #[inline]
    pub fn tree_row_indent(&self) -> LogicalPx {
        self.spacing_md
    }

    /// `component.tree-row-meta-font-size` → `{semantic.font-size-micro}` = 10px
    #[inline]
    pub fn tree_row_meta_font_size(&self) -> LogicalPx {
        self.font_size_micro
    }
}
