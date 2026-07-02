//! Generated from `dtcg/tasty.tokens.json` — DO NOT EDIT.
//! 재생성: `cargo run -p tasty-design-tokens --bin generate`.
//!
//! Tier 3 — component 치수 (테마 불변), 컴포넌트별 하위 모듈. semantic (일부는
//! primitive 직접 — 디자인 실물의 tier-skip alias) 참조로 정의된다.
//! 색 component 접근자는 시리즈 04 에서 결정.
//!
//! **zoom 주의**: 런타임 소비는 반드시 `&Theme` 경유 — `semantic.rs` 참조.

pub mod badge {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.badge-dot-size` → `{component.status-dot-size}` = 8px
    pub const DOT_SIZE: LogicalPx = super::status_dot::SIZE;

    /// `component.badge-font-size` → `{semantic.font-size-micro}` = 10px
    pub const FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_MICRO;

    /// `component.badge-font-weight` → `{semantic.font-weight-bold}` = 700
    pub const FONT_WEIGHT: u16 = crate::generated::semantic::FONT_WEIGHT_BOLD;

    /// `component.badge-padding-x` → `{semantic.space-xs}` = 4px
    pub const PADDING_X: LogicalPx = crate::generated::semantic::SPACE_XS;

    /// `component.badge-radius` → `{semantic.radius-sm}` = 2px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS_SM;

    /// `component.badge-size` → `{primitive.size-16}` = 16px
    pub const SIZE: LogicalPx = crate::generated::primitive::SIZE_16;
}

pub mod banner {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.banner-body-font-size` → `{semantic.font-size-caption}` = 11px
    pub const BODY_FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_CAPTION;

    /// `component.banner-countdown-font-size` → `{semantic.font-size-micro}` = 10px
    pub const COUNTDOWN_FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_MICRO;

    /// `component.banner-fade` → `{semantic.motion-ui}` = 120ms (ms)
    pub const FADE: f32 = crate::generated::semantic::MOTION_UI;

    /// `component.banner-gap` → `{semantic.space-md}` = 12px
    pub const GAP: LogicalPx = crate::generated::semantic::SPACE_MD;

    /// `component.banner-margin` → `{semantic.space-sm}` = 8px
    pub const MARGIN: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.banner-padding-x` → `{semantic.space-md}` = 12px
    pub const PADDING_X: LogicalPx = crate::generated::semantic::SPACE_MD;

    /// `component.banner-padding-y` → `{semantic.space-sm}` = 8px
    pub const PADDING_Y: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.banner-radius` → `{primitive.radius-8}` = 8px
    pub const RADIUS: LogicalPx = crate::generated::primitive::RADIUS_8;

    /// `component.banner-recessed-opacity` → `{primitive.opacity-recessed}` = 0.4
    pub const RECESSED_OPACITY: f32 = crate::generated::primitive::OPACITY_RECESSED;

    /// `component.banner-title-font-size` → `{semantic.font-size-body}` = 13px
    pub const TITLE_FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_BODY;
}

pub mod button {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.button-font-size` → `{semantic.font-size-body}` = 13px
    pub const FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_BODY;

    /// `component.button-font-weight` → `{semantic.font-weight-medium}` = 500
    pub const FONT_WEIGHT: u16 = crate::generated::semantic::FONT_WEIGHT_MEDIUM;

    /// `component.button-gap` → `{semantic.space-sm}` = 8px
    pub const GAP: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.button-height` → `{semantic.control-height}` = 28px
    pub const HEIGHT: LogicalPx = crate::generated::semantic::CONTROL_HEIGHT;

    /// `component.button-height-lg` → `{primitive.size-32}` = 32px
    pub const HEIGHT_LG: LogicalPx = crate::generated::primitive::SIZE_32;

    /// `component.button-height-sm` → `{semantic.control-height-tab}` = 24px
    pub const HEIGHT_SM: LogicalPx = crate::generated::semantic::CONTROL_HEIGHT_TAB;

    /// `component.button-padding-x` → `{semantic.space-md}` = 12px
    pub const PADDING_X: LogicalPx = crate::generated::semantic::SPACE_MD;

    /// `component.button-radius` → `{semantic.radius}` = 4px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS;
}

pub mod checkbox {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.checkbox-radius` → `{semantic.radius-sm}` = 2px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS_SM;

    /// `component.checkbox-size` → `{primitive.size-16}` = 16px
    pub const SIZE: LogicalPx = crate::generated::primitive::SIZE_16;
}

pub mod help_hint {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.help-hint-gap` → `{semantic.space-xs}` = 4px
    pub const GAP: LogicalPx = crate::generated::semantic::SPACE_XS;

    /// `component.help-hint-size` → `{semantic.icon-size-sm}` = 14px
    pub const SIZE: LogicalPx = crate::generated::semantic::ICON_SIZE_SM;
}

pub mod icon_button {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.icon-button-radius` → `{semantic.radius}` = 4px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS;

    /// `component.icon-button-size` → `{semantic.control-height}` = 28px
    pub const SIZE: LogicalPx = crate::generated::semantic::CONTROL_HEIGHT;

    /// `component.icon-button-size-sm` → `{primitive.size-24}` = 24px
    pub const SIZE_SM: LogicalPx = crate::generated::primitive::SIZE_24;
}

pub mod input {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.input-font-size` → `{semantic.font-size-body}` = 13px
    pub const FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_BODY;

    /// `component.input-gap` → `{semantic.space-sm}` = 8px
    pub const GAP: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.input-height` → `{semantic.control-height}` = 28px
    pub const HEIGHT: LogicalPx = crate::generated::semantic::CONTROL_HEIGHT;

    /// `component.input-padding-x` → `{semantic.space-md}` = 12px
    pub const PADDING_X: LogicalPx = crate::generated::semantic::SPACE_MD;

    /// `component.input-radius` → `{semantic.radius}` = 4px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS;
}

pub mod kbd {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.kbd-font-size` → `{semantic.font-size-micro}` = 10px
    pub const FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_MICRO;

    /// `component.kbd-gap` → `{primitive.size-3}` = 3px
    pub const GAP: LogicalPx = crate::generated::primitive::SIZE_3;

    /// `component.kbd-padding-x` → `{semantic.space-xs}` = 4px
    pub const PADDING_X: LogicalPx = crate::generated::semantic::SPACE_XS;

    /// `component.kbd-radius` → `{semantic.radius-sm}` = 2px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS_SM;

    /// `component.kbd-shadow-depth` → `{primitive.size-2}` = 2px
    pub const SHADOW_DEPTH: LogicalPx = crate::generated::primitive::SIZE_2;

    /// `component.kbd-size` → `{primitive.size-16}` = 16px
    pub const SIZE: LogicalPx = crate::generated::primitive::SIZE_16;
}

pub mod menu {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.menu-item-height` → `{semantic.control-height}` = 28px
    pub const ITEM_HEIGHT: LogicalPx = crate::generated::semantic::CONTROL_HEIGHT;

    /// `component.menu-item-padding-x` → `{semantic.space-md}` = 12px
    pub const ITEM_PADDING_X: LogicalPx = crate::generated::semantic::SPACE_MD;

    /// `component.menu-item-radius` → `{semantic.radius-sm}` = 2px
    pub const ITEM_RADIUS: LogicalPx = crate::generated::semantic::RADIUS_SM;

    /// `component.menu-item-shortcut-font-size` → `{semantic.font-size-micro}` = 10px
    pub const ITEM_SHORTCUT_FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_MICRO;

    /// `component.menu-radius` → `{semantic.radius}` = 4px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS;
}

pub mod plugins_list {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.plugins-list-width` → `{primitive.size-288}` = 288px
    pub const WIDTH: LogicalPx = crate::generated::primitive::SIZE_288;
}

pub mod remote {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.remote-label-col` → `{primitive.size-112}` = 112px
    pub const LABEL_COL: LogicalPx = crate::generated::primitive::SIZE_112;
}

pub mod select {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.select-chevron-offset` → `{semantic.space-sm}` = 8px
    pub const CHEVRON_OFFSET: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.select-chevron-room` → `{primitive.size-28}` = 28px
    pub const CHEVRON_ROOM: LogicalPx = crate::generated::primitive::SIZE_28;

    /// `component.select-font-size` → `{semantic.font-size-body}` = 13px
    pub const FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_BODY;

    /// `component.select-height` → `{semantic.control-height}` = 28px
    pub const HEIGHT: LogicalPx = crate::generated::semantic::CONTROL_HEIGHT;

    /// `component.select-padding-x` → `{semantic.space-md}` = 12px
    pub const PADDING_X: LogicalPx = crate::generated::semantic::SPACE_MD;

    /// `component.select-radius` → `{semantic.radius}` = 4px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS;
}

pub mod settings {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.settings-row-min-height` → `{primitive.size-32}` = 32px
    pub const ROW_MIN_HEIGHT: LogicalPx = crate::generated::primitive::SIZE_32;

    /// `component.settings-sidebar-width` → `{primitive.size-200}` = 200px
    pub const SIDEBAR_WIDTH: LogicalPx = crate::generated::primitive::SIZE_200;
}

pub mod sidebar {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.sidebar-button-label-font-size` → `{semantic.font-size-caption}` = 11px
    pub const BUTTON_LABEL_FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_CAPTION;

    /// `component.sidebar-collapsed-icon-height` → `{primitive.size-22}` = 22px
    pub const COLLAPSED_ICON_HEIGHT: LogicalPx = crate::generated::primitive::SIZE_22;

    /// `component.sidebar-collapsed-slot-width` → `{primitive.size-32}` = 32px
    pub const COLLAPSED_SLOT_WIDTH: LogicalPx = crate::generated::primitive::SIZE_32;

    /// `component.sidebar-collapsed-workspace-height` → `{primitive.size-28}` = 28px
    pub const COLLAPSED_WORKSPACE_HEIGHT: LogicalPx = crate::generated::primitive::SIZE_28;

    /// `component.sidebar-logo-collapsed-size` → `{primitive.size-24}` = 24px
    pub const LOGO_COLLAPSED_SIZE: LogicalPx = crate::generated::primitive::SIZE_24;

    /// `component.sidebar-logo-size` → `{primitive.size-22}` = 22px
    pub const LOGO_SIZE: LogicalPx = crate::generated::primitive::SIZE_22;

    /// `component.sidebar-section-heading-font-size` → `{semantic.font-size-micro}` = 10px
    pub const SECTION_HEADING_FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_MICRO;

    /// `component.sidebar-wordmark-font-size` → `{semantic.font-size-brand-wordmark}` = 17px
    pub const WORDMARK_FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_BRAND_WORDMARK;
}

pub mod spinner {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.spinner-duration` → `{primitive.duration-90}` = 90ms (ms)
    pub const DURATION: f32 = crate::generated::primitive::DURATION_90;

    /// `component.spinner-size` → `{primitive.size-16}` = 16px
    pub const SIZE: LogicalPx = crate::generated::primitive::SIZE_16;
}

pub mod status_dot {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.status-dot-attached-ring-offset` → `{primitive.size-2}` = 2px
    pub const ATTACHED_RING_OFFSET: LogicalPx = crate::generated::primitive::SIZE_2;

    /// `component.status-dot-attached-ring-width` → `{primitive.size-2}` = 2px
    pub const ATTACHED_RING_WIDTH: LogicalPx = crate::generated::primitive::SIZE_2;

    /// `component.status-dot-pulse-duration` → `{primitive.duration-1600}` = 1600ms (ms)
    pub const PULSE_DURATION: f32 = crate::generated::primitive::DURATION_1600;

    /// `component.status-dot-size` → `{primitive.size-8}` = 8px
    pub const SIZE: LogicalPx = crate::generated::primitive::SIZE_8;
}

pub mod swatch {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.swatch-radius` → `{semantic.radius-sm}` = 2px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS_SM;

    /// `component.swatch-size` → `{primitive.size-16}` = 16px
    pub const SIZE: LogicalPx = crate::generated::primitive::SIZE_16;
}

pub mod switch {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.switch-radius` → `{semantic.radius-pill}` = 9999px (sentinel — 완전 원형용 상한값)
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS_PILL;

    /// `component.switch-thumb-inset` → `{primitive.size-2}` = 2px
    pub const THUMB_INSET: LogicalPx = crate::generated::primitive::SIZE_2;

    /// `component.switch-thumb-size` → `{primitive.size-12}` = 12px
    pub const THUMB_SIZE: LogicalPx = crate::generated::primitive::SIZE_12;

    /// `component.switch-thumb-travel` → `{primitive.size-12}` = 12px
    pub const THUMB_TRAVEL: LogicalPx = crate::generated::primitive::SIZE_12;

    /// `component.switch-track-height` → `{primitive.size-16}` = 16px
    pub const TRACK_HEIGHT: LogicalPx = crate::generated::primitive::SIZE_16;

    /// `component.switch-track-width` → `{primitive.size-28}` = 28px
    pub const TRACK_WIDTH: LogicalPx = crate::generated::primitive::SIZE_28;
}

pub mod switch_overlay {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.switch-overlay-fade` → `{semantic.motion-ui-fast}` = 90ms (ms)
    pub const FADE: f32 = crate::generated::semantic::MOTION_UI_FAST;

    /// `component.switch-overlay-shadow-depth` → `{component.kbd-shadow-depth}` = 2px
    pub const SHADOW_DEPTH: LogicalPx = super::kbd::SHADOW_DEPTH;

    /// `component.switch-overlay-size` → `{component.kbd-size}` = 16px
    pub const SIZE: LogicalPx = super::kbd::SIZE;
}

pub mod tab {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.tab-close-radius` → `{semantic.radius-sm}` = 2px
    pub const CLOSE_RADIUS: LogicalPx = crate::generated::semantic::RADIUS_SM;

    /// `component.tab-close-size` → `{primitive.size-16}` = 16px
    pub const CLOSE_SIZE: LogicalPx = crate::generated::primitive::SIZE_16;

    /// `component.tab-dot-size` → `{component.status-dot-size}` = 8px
    pub const DOT_SIZE: LogicalPx = super::status_dot::SIZE;

    /// `component.tab-gap` → `{semantic.space-sm}` = 8px
    pub const GAP: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.tab-height` → `{semantic.control-height-tab}` = 24px
    pub const HEIGHT: LogicalPx = crate::generated::semantic::CONTROL_HEIGHT_TAB;

    /// `component.tab-icon-size` → `{semantic.icon-size-sm}` = 14px
    pub const ICON_SIZE: LogicalPx = crate::generated::semantic::ICON_SIZE_SM;

    /// `component.tab-indicator-width` → `{primitive.size-2}` = 2px
    pub const INDICATOR_WIDTH: LogicalPx = crate::generated::primitive::SIZE_2;

    /// `component.tab-padding-x` → `{semantic.space-sm}` = 8px
    pub const PADDING_X: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.tab-strip-width` → `{semantic.tab-width}` = 150px
    pub const STRIP_WIDTH: LogicalPx = crate::generated::semantic::TAB_WIDTH;
}

pub mod table {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.table-cell-height` → `{semantic.control-height}` = 28px
    pub const CELL_HEIGHT: LogicalPx = crate::generated::semantic::CONTROL_HEIGHT;

    /// `component.table-cell-padding-x` → `{semantic.space-md}` = 12px
    pub const CELL_PADDING_X: LogicalPx = crate::generated::semantic::SPACE_MD;

    /// `component.table-cell-padding-y` → `{semantic.space-sm}` = 8px
    pub const CELL_PADDING_Y: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.table-font-size` → `{semantic.font-size-body}` = 13px
    pub const FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_BODY;

    /// `component.table-header-font-size` → `{semantic.font-size-caption}` = 11px
    pub const HEADER_FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_CAPTION;

    /// `component.table-header-font-weight` → `{semantic.font-weight-medium}` = 500
    pub const HEADER_FONT_WEIGHT: u16 = crate::generated::semantic::FONT_WEIGHT_MEDIUM;
}

pub mod tag {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.tag-dot-size` → `{primitive.size-8}` = 8px
    pub const DOT_SIZE: LogicalPx = crate::generated::primitive::SIZE_8;

    /// `component.tag-font-size` → `{semantic.font-size-micro}` = 10px
    pub const FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_MICRO;

    /// `component.tag-gap` → `{semantic.space-xs}` = 4px
    pub const GAP: LogicalPx = crate::generated::semantic::SPACE_XS;

    /// `component.tag-padding-x` → `{semantic.space-sm}` = 8px
    pub const PADDING_X: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.tag-radius` → `{semantic.radius-sm}` = 2px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS_SM;

    /// `component.tag-size` → `{primitive.size-16}` = 16px
    pub const SIZE: LogicalPx = crate::generated::primitive::SIZE_16;
}

pub mod titlebar {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.titlebar-caption-width` → `{primitive.size-46}` = 46px
    pub const CAPTION_WIDTH: LogicalPx = crate::generated::primitive::SIZE_46;

    /// `component.titlebar-csd-radius` → `{primitive.radius-8}` = 8px
    pub const CSD_RADIUS: LogicalPx = crate::generated::primitive::RADIUS_8;

    /// `component.titlebar-csd-shadow-margin` → `{primitive.size-8}` = 8px
    pub const CSD_SHADOW_MARGIN: LogicalPx = crate::generated::primitive::SIZE_8;

    /// `component.titlebar-traffic-size` → `{primitive.size-12}` = 12px
    pub const TRAFFIC_SIZE: LogicalPx = crate::generated::primitive::SIZE_12;

    /// `component.titlebar-window-button-size` → `{primitive.size-24}` = 24px
    pub const WINDOW_BUTTON_SIZE: LogicalPx = crate::generated::primitive::SIZE_24;
}

pub mod toast {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.toast-accent-width` → `{primitive.size-3}` = 3px
    pub const ACCENT_WIDTH: LogicalPx = crate::generated::primitive::SIZE_3;

    /// `component.toast-gap` → `{semantic.space-sm}` = 8px
    pub const GAP: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.toast-hint-font-size` → `{semantic.font-size-micro}` = 10px
    pub const HINT_FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_MICRO;

    /// `component.toast-max-width` → `{primitive.size-320}` = 320px
    pub const MAX_WIDTH: LogicalPx = crate::generated::primitive::SIZE_320;

    /// `component.toast-min-height` → `{semantic.control-height}` = 28px
    pub const MIN_HEIGHT: LogicalPx = crate::generated::semantic::CONTROL_HEIGHT;

    /// `component.toast-padding-x` → `{semantic.space-md}` = 12px
    pub const PADDING_X: LogicalPx = crate::generated::semantic::SPACE_MD;

    /// `component.toast-padding-y` → `{semantic.space-sm}` = 8px
    pub const PADDING_Y: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.toast-radius` → `{semantic.radius}` = 4px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS;
}

pub mod tooltip {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.tooltip-delay` → `{semantic.motion-ui-med}` = 150ms (ms)
    pub const DELAY: f32 = crate::generated::semantic::MOTION_UI_MED;

    /// `component.tooltip-font-size` → `{semantic.font-size-caption}` = 11px
    pub const FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_CAPTION;

    /// `component.tooltip-line-height` → `{semantic.line-height-ui}` = 1.4
    pub const LINE_HEIGHT: f32 = crate::generated::semantic::LINE_HEIGHT_UI;

    /// `component.tooltip-max-width` → `{primitive.size-240}` = 240px
    pub const MAX_WIDTH: LogicalPx = crate::generated::primitive::SIZE_240;

    /// `component.tooltip-offset` → `{semantic.space-xs}` = 4px
    pub const OFFSET: LogicalPx = crate::generated::semantic::SPACE_XS;

    /// `component.tooltip-padding-x` → `{semantic.space-sm}` = 8px
    pub const PADDING_X: LogicalPx = crate::generated::semantic::SPACE_SM;

    /// `component.tooltip-padding-y` → `{semantic.space-xs}` = 4px
    pub const PADDING_Y: LogicalPx = crate::generated::semantic::SPACE_XS;

    /// `component.tooltip-radius` → `{semantic.radius}` = 4px
    pub const RADIUS: LogicalPx = crate::generated::semantic::RADIUS;
}

pub mod tree_row {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.tree-row-font-size` → `{semantic.font-size-body}` = 13px
    pub const FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_BODY;

    /// `component.tree-row-gap` → `{semantic.space-xs}` = 4px
    pub const GAP: LogicalPx = crate::generated::semantic::SPACE_XS;

    /// `component.tree-row-height` → `{semantic.control-height-tree}` = 22px
    pub const HEIGHT: LogicalPx = crate::generated::semantic::CONTROL_HEIGHT_TREE;

    /// `component.tree-row-indent` → `{semantic.space-md}` = 12px
    pub const INDENT: LogicalPx = crate::generated::semantic::SPACE_MD;

    /// `component.tree-row-meta-font-size` → `{semantic.font-size-micro}` = 10px
    pub const META_FONT_SIZE: LogicalPx = crate::generated::semantic::FONT_SIZE_MICRO;
}

pub mod workspace {
    use tasty_type_geometry::length::LogicalPx;

    /// `component.workspace-mirror-gap` → `{semantic.space-xs}` = 4px
    pub const MIRROR_GAP: LogicalPx = crate::generated::semantic::SPACE_XS;

    /// `component.workspace-mirror-icon-size` → `{semantic.icon-size-xs}` = 12px
    pub const MIRROR_ICON_SIZE: LogicalPx = crate::generated::semantic::ICON_SIZE_XS;
}
