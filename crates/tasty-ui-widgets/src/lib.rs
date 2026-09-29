#![forbid(unsafe_code)]

//! 본체와 갤러리가 공유하는 egui 위젯과 레이아웃.
//! Theme는 인자로 전달하고 본체의 전역 상태에는 의존하지 않는다.
//! 공용 위젯은 사용처 수와 무관하게 여기 두며 갤러리를 먼저 만드는 과정에서
//! 본체 호출자가 아직 없는 경우도 허용한다. 같은 UI를 다시 구현하지 않고 공용 함수를 사용한다.
//! 기준은 docs/architecture/ui-widgets-crate.md와 docs/dev-guide/gallery-first.md를 따른다.

mod autocomplete;
mod banner;
pub mod brand;
mod button;
mod center_state;
mod chip;
mod chrome_slot;
mod control;
pub mod crumb_alloc;
mod drilldown;
pub mod file_handler;
mod help_hint;
mod horizontal_tab_bar;
mod html_script_banner;
mod icon_button;
mod info_modal;
mod input;
mod keyboard_cursor;
mod language_select;
mod listctrl;
mod mac_permissions;
mod menu_item;
mod move_source;
mod multi_select;
mod path_field;
mod plugin_avatar;
mod popup_title;
mod remote_tool;
mod segmented;
mod select;
mod spacing;
mod spinner;
mod status_bar;
mod status_dot;
mod tab_content_frame;
mod table;
mod toast;
mod toggle;
pub mod tokens;
mod tooltip;
mod tree_row;
mod two_depth;
mod warning_callout;
pub use autocomplete::{
    AutoComplete, AutoCompleteAction, AutoCompleteResponse, MatchMode, autocomplete_dropdown,
};
pub use banner::{banner_shell, inset_banner_zone, inset_content_rect};
pub use button::{Button, ButtonVariant, banner_surface};
pub use center_state::{
    CENTER_STATE_ERROR_GLYPH, CenterState, CenterStateOutput, CenterStateVariant,
};
pub use chip::{
    BadgeVariant, KbdKey, TagVariant, badge, badge_dot, kbd, kbd_parts, kbd_parts_at,
    kbd_parts_width, kbd_width, num_keycap, paint_badge_dot, paint_num_keycap, tag, tag_width,
};
pub use chrome_slot::top_right_inset_square;
pub use control::ControlSize;
pub use drilldown::{DrillDown, DrillDownActions, DrillDownOutput, DrillDownView};
pub use help_hint::HelpHint;
pub use horizontal_tab_bar::horizontal_tab_bar_with_arrows;
pub use html_script_banner::{
    HtmlScriptBannerOutput, HtmlScriptBannerState, HtmlScriptBannerView, HtmlScriptMarkerKind,
    html_script_banner, html_script_banner_is_narrow, html_script_marker,
};
pub use icon_button::{
    IconButton, IconButtonState, IconButtonVariant, IconPainter, paint_icon_button_state,
};
pub use info_modal::{
    InfoModalButton, InfoModalOutput, InfoModalSpan, InfoModalSpanKind, InfoModalView,
    footer_height as info_modal_footer_height, info_modal, parse_emphasis as info_modal_parse,
    shell_height as info_modal_shell_height,
};
pub use input::Input;
pub use language_select::{LanguageOption, LanguageSelectLabels, language_select};
pub use listctrl::{ListCtrl, ListCtrlItem, ListCtrlOutput, ListCtrlTrailing};
pub use mac_permissions::{
    MacPermissionsOutput, MacPermissionsView, PermRow, PermState, mac_permissions,
};
pub use menu_item::{MenuItemVariant, menu_item, menu_item_kbd, menu_separator};
pub use move_source::{
    move_source_glyph_size, paint_move_source_chip, paint_move_source_glyph, paint_move_source_ring,
};
pub use multi_select::{
    MultiSelectAllToggle, MultiSelectLabels, multi_select, multi_select_popup_id,
    multi_select_summary, popup_chrome_width,
};
pub use path_field::{PathField, PathFieldOutcome};
pub use plugin_avatar::{PluginAvatarSize, paint_plugin_avatar, plugin_avatar};
pub use popup_title::{
    elide_popup_title, paint_popup_title_glyph, popup_title_font, popup_title_text_rect,
    show_popup_title_tooltip,
};
pub use remote_tool::{
    FILTER_DROPDOWN_MAX_HEIGHT, FILTER_DROPDOWN_MIN_WIDTH, LocalSshHost, LocalSshSectionData,
    ProtocolFilterItem, ProtocolFilterLabels, TabStripData, TextWrap, draw_local_ssh_section,
    draw_protocol_filter_body, draw_protocol_filter_button, draw_tab_strip, ghost_button, hsep,
    primary_button, secondary_button, selectable_label, selectable_text, warn_badge,
};
pub use segmented::segmented;
pub use select::{select, select_or_placeholder};
pub use spacing::{hspace, margin_all, margin_sym, vspace};
pub use spinner::Spinner;
pub use status_bar::{StatusBarAction, StatusBarData, StatusBarDrawResult, draw_status_bar_view};
pub use status_dot::{StatusKind, status_dot};
pub use tab_content_frame::{settings_content_column, tab_content_frame};
pub use table::{Table, TableAlign, TableColumn, TableColumnWidth, TableOutput, TableSortDir};
pub use toast::{
    CardColors as ToastCardColors, FADE_IN_MS as TOAST_FADE_IN_MS,
    FADE_OUT_MS as TOAST_FADE_OUT_MS, ToastEntryView, ToastScopeView, ToastViewProps,
    accent_color as toast_accent_color, card_colors as toast_card_colors,
    draw_card as draw_toast_card, draw_single_card as draw_toast_single_card, draw_toast_scopes,
    fade_alpha as toast_fade_alpha, layout_card as toast_layout_card,
};
pub use toggle::{checkbox, checkbox_width, switch};
pub use tooltip::{Tooltip, TooltipPlacement, tooltip_hover_delay_elapsed};
pub use tree_row::tree_row;
pub use two_depth::{two_depth_layout, two_depth_layout_filtered};
pub use warning_callout::warning_callout;
