#![forbid(unsafe_code)]

//! 본체와 갤러리가 공유하는 egui 위젯과 레이아웃.
//! Theme는 인자로 전달하고 본체의 전역 상태에는 의존하지 않는다.
//! 공용 위젯은 사용처 수와 무관하게 여기 두며 갤러리를 먼저 만드는 과정에서
//! 본체 호출자가 아직 없는 경우도 허용한다. 같은 UI를 다시 구현하지 않고 공용 함수를 사용한다.
//! 기준은 docs/architecture/ui-widgets-crate.md와 docs/dev-guide/gallery-first.md를 따른다.

mod attach_refusal;
mod attach_size_sync;
mod attention;
mod autocomplete;
mod banner;
mod banner_more_row;
pub mod brand;
mod button;
mod center_state;
mod chip;
mod chrome_slot;
mod clipboard_viewer;
mod code_area;
mod compact_state;
mod control;
pub mod crumb_alloc;
mod dashed_edge;
mod drilldown;
mod explorer_columns;
mod explorer_commands;
mod explorer_find_bar;
mod explorer_name_edit;
mod explorer_ops;
pub mod file_handler;
mod filter_readout;
mod help_hint;
mod horizontal_tab_bar;
mod html_script_banner;
mod icon_button;
mod info_modal;
mod input;
mod kb_plugins;
mod keyboard_cursor;
mod language_select;
mod listctrl;
mod mac_permissions;
mod menu_item;
mod move_source;
mod multi_select;
mod override_row;
mod path_field;
mod plugin_add;
mod plugin_avatar;
mod plugin_detail;
mod plugin_paths;
mod popup_title;
mod ports_table;
mod remote_tool;
mod resize_grip;
mod script_confirm;
mod script_trigger;
mod segmented;
mod select;
mod sequence_editor;
mod settings_row;
mod shell_setup;
mod spacing;
mod spinner;
mod state_screen;
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
pub use attach_refusal::{
    AttachRefusalBannerClicks, AttachRefusalBannerView, attach_refusal_avatar_tooltip,
    attach_refusal_banner, attach_refusal_banner_content, attach_refusal_mark,
    attach_refusal_mark_size, paint_attach_refusal_chip,
};
pub use attach_size_sync::{
    AttachSizeSyncBannerClicks, AttachSizeSyncBannerView, attach_size_sync_banner,
    attach_size_sync_banner_content,
};
pub use attention::{
    Attention, RailDot, attention_count_label, attention_edge_stroke, occupancy_edge_shows,
    occupancy_edge_stroke, paint_rail_dot, surface_edge_attention, tab_title_color,
    workspace_attention_badges,
};
pub use autocomplete::{
    AutoComplete, AutoCompleteAction, AutoCompleteResponse, MatchMode, autocomplete_dropdown,
};
pub use banner::{banner_is_narrow, banner_shell, inset_banner_zone, inset_content_rect};
pub use banner_more_row::{
    BannerMoreLabel, banner_more_row, banner_more_row_height, banner_more_row_natural_width,
};
pub use button::{Button, ButtonVariant, banner_surface, banner_surface_ctx, in_banner_surface};
pub use center_state::{
    CENTER_STATE_ERROR_GLYPH, CenterState, CenterStateOutput, CenterStateVariant,
};
pub use chip::{
    BadgeVariant, KbdKey, TagVariant, badge, badge_disabled, badge_dot, disabled_chip_scope,
    in_disabled_chip_scope, kbd, kbd_parts, kbd_parts_at, kbd_parts_width, kbd_text_parts_painted,
    kbd_width, num_keycap, paint_badge_dot, paint_num_keycap, tag, tag_caps, tag_disabled,
    tag_width,
};
pub use chrome_slot::top_right_inset_square;
pub use clipboard_viewer::{
    SEG_COMPACT_AT, SegmentIconPainter, TypeSegment, draw_type_segments, seg_shows_label,
};
pub use code_area::{CodeArea, CodeAreaKeys, CodeAreaOutput};
pub use compact_state::{CompactStateGlyph, CompactStateRow, compact_state_row};
pub use control::ControlSize;
pub use dashed_edge::paint_dashed_outline;
pub use drilldown::{DrillDown, DrillDownActions, DrillDownOutput, DrillDownView};
pub use explorer_columns::{
    ExplorerDetailHeads, explorer_detail_columns, explorer_detail_tail_width,
};
pub use explorer_commands::{
    ExplorerCommand, ExplorerCommandClick, ExplorerCommandLabels, ExplorerCommandsView,
    ExplorerToggle, explorer_commands, explorer_commands_compact, explorer_commands_width,
};
pub use explorer_find_bar::{
    ExplorerFindBar, ExplorerFindEvents, ExplorerFindLabels, ExplorerFindStatus, explorer_find_bar,
    explorer_find_bar_height, explorer_match_job, match_range,
};
pub use explorer_name_edit::{
    ExplorerNameEdit, ExplorerNameEvent, ExplorerNameLayout, explorer_name_error, explorer_name_row,
};
pub use explorer_ops::{
    ConflictPick, ConflictProps, DragChipProps, DragOp, OpStatusProps, OpStatusResponse,
    QueueRowProps, ResultAction, ResultCardProps, ResultCardResponse, ResultLine, conflict_card,
    drag_chip, op_queue_frame, op_queue_popover, op_queue_rows, op_queue_width, op_status_line,
    paint_drop_target, result_card,
};
pub use filter_readout::{filter_readout, filter_readout_label, filter_readout_width};
pub use help_hint::HelpHint;
pub use horizontal_tab_bar::{
    TabScrollArrowInk, TabScrollArrowSide, horizontal_tab_bar_with_arrows, paint_tab_scroll_arrow,
};
pub use html_script_banner::{
    HtmlScriptBannerOutput, HtmlScriptBannerState, HtmlScriptBannerView, HtmlScriptMarkerKind,
    html_script_banner, html_script_banner_is_narrow, html_script_marker,
};
pub use icon_button::{
    IconButton, IconButtonState, IconButtonVariant, IconPainter, paint_icon_button_state,
};
pub use info_modal::{
    InfoModalButton, InfoModalOutput, InfoModalParagraph, InfoModalParagraphKind, InfoModalSpan,
    InfoModalSpanKind, InfoModalView, footer_height as info_modal_footer_height, info_modal,
    parse_emphasis as info_modal_parse, shell_height as info_modal_shell_height,
};
pub use input::Input;
pub use kb_plugins::{
    KB_PLUGIN_MODE_CUSTOM, KB_PLUGIN_MODE_INHERIT, KB_PLUGIN_MODE_NONE, KbPluginLabels,
    KbPluginRowOutput, KbPluginRowRects, KbPluginRowView, KbPluginSlot, KbPluginsOutput,
    KbPluginsView, kb_plugins_subtab,
};
pub use language_select::{LanguageOption, LanguageSelectLabels, language_select};
pub use listctrl::{ListCtrl, ListCtrlItem, ListCtrlOutput, ListCtrlTrailing};
pub use mac_permissions::{
    MacPermissionsOutput, MacPermissionsView, PermRow, PermState, mac_permissions,
};
pub use menu_item::{
    MenuItemVariant, fit_menu_width, menu_item, menu_item_kbd, menu_item_with_hover,
    menu_label_galley, menu_option, menu_option_icon, menu_option_value, menu_separator,
};
pub use move_source::{
    move_source_glyph_size, paint_move_source_chip, paint_move_source_glyph, paint_move_source_ring,
};
pub use multi_select::{
    MultiSelectAllToggle, MultiSelectLabels, multi_select, multi_select_popup_id,
    multi_select_summary, popup_chrome_width,
};
pub use override_row::{OverrideCell, override_row, override_row_fits};
pub use path_field::{PathField, PathFieldOutcome};
pub use plugin_add::{
    PLUGIN_ADD_INSET, PluginAddBarClicks, PluginAddBarView, PluginAddPickerOutput,
    PluginAddPickerView, PluginFingerprintLineView, PluginManifestCardOutput,
    PluginManifestCardView, PluginTrustKind, mono_header as plugin_mono_header, plugin_add_bar,
    plugin_add_empty_hint, plugin_add_path_picker, plugin_add_read_error, plugin_fingerprint_line,
    plugin_manifest_card, plugin_signature_invalid_detail, plugin_trust_box, short_fingerprint,
};
pub use plugin_avatar::{PluginAvatarSize, paint_plugin_avatar, plugin_avatar};
pub use plugin_detail::{
    PluginAttentionBarAction, PluginAttentionBarView, PluginDetailBarClicks, PluginDetailBarView,
    PluginIdentityView, PluginMetaView, PluginUninstallConfirmClicks, PluginUninstallConfirmView,
    homepage_display, is_web_homepage, plugin_attention_bar, plugin_command_row, plugin_detail_bar,
    plugin_detail_bar_frame, plugin_detail_bar_height, plugin_detail_description,
    plugin_detail_identity, plugin_detail_meta, plugin_detail_name_row, plugin_keycaps,
    plugin_uninstall_confirm_bar, plugin_uninstall_confirm_bar_height,
};
pub use plugin_paths::{
    PluginInstallPathsView, plugin_detail_section, plugin_detail_section_gap, plugin_install_paths,
};
pub use popup_title::{
    elide_popup_title, paint_popup_title_glyph, popup_title_font, popup_title_text_rect,
    show_popup_title_tooltip,
};
pub use ports_table::{
    PORTS_PANEL_PAD_X, PortsColumn, ports_favorite_detail_and_state,
    ports_favorite_state_min_width, ports_process_cell, ports_star_column_width, ports_table,
};
pub use remote_tool::{
    FILTER_DROPDOWN_MAX_HEIGHT, LocalSshHost, LocalSshSectionData, ProtocolFilterItem,
    ProtocolFilterLabels, RemoteRowChip, TabStripData, TextWrap, draw_local_ssh_section,
    draw_protocol_filter_body, draw_protocol_filter_button, draw_tab_strip,
    filter_dropdown_content_width, ghost_button, primary_button, remote_list_row,
    remote_row_actions_width, remote_row_title, secondary_button, selectable_label,
    selectable_text, warn_badge, warn_badge_width,
};
pub use resize_grip::{modhint_resize_grip, resize_grip_segments};
pub use script_confirm::{
    SCRIPT_CONFIRM_PAD_X, ScriptConfirmOutput, ScriptConfirmView, script_confirm,
};
pub use script_trigger::{
    script_trigger_add_control, script_trigger_menu, script_trigger_menu_frame,
};
pub use segmented::segmented;
pub use select::{select, select_or_placeholder};
pub use sequence_editor::{
    SequenceEditorAction, SequenceEditorError, SequenceEditorView, sequence_editor,
};
pub use settings_row::{
    SettingsRow, settings_label_cell, settings_label_column, settings_label_gap,
    settings_row_caption,
};
pub use shell_setup::{
    SHELL_SETUP_FORM_WIDTH, ShellSetupCheck, ShellSetupOutput, ShellSetupView, shell_setup_screen,
};
pub use spacing::{hspace, margin_all, margin_sym, vspace};
pub use spinner::Spinner;
pub use state_screen::{GlyphPainter, StateGlyph, StateScreenView, state_screen};
pub use status_bar::{StatusBarAction, StatusBarData, StatusBarDrawResult, draw_status_bar_view};
pub use status_dot::{StatusKind, status_dot};
pub use tab_content_frame::{settings_content_column, tab_content_frame};
pub use table::{
    Table, TableAlign, TableColumn, TableColumnWidth, TableOutput, TableSortDir, fixed_total_width,
};
pub use toast::{
    CardColors as ToastCardColors, FADE_IN_MS as TOAST_FADE_IN_MS,
    FADE_OUT_MS as TOAST_FADE_OUT_MS, ToastEntryView, ToastScopeView, ToastStackBottom,
    ToastViewProps, accent_color as toast_accent_color, card_colors as toast_card_colors,
    draw_card as draw_toast_card, draw_single_card as draw_toast_single_card, draw_toast_scopes,
    fade_alpha as toast_fade_alpha, layout_card as toast_layout_card,
    stack_anchor as toast_stack_anchor,
};
pub use toggle::{checkbox, checkbox_width, switch, switch_with_label_color};
pub use tooltip::{Tooltip, TooltipPlacement, tooltip_hover_delay_elapsed};
pub use tree_row::{tree_row, tree_row_icon_center, tree_row_matching};
pub use two_depth::{two_depth_layout, two_depth_layout_filtered};
pub use warning_callout::warning_callout;
