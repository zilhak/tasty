//! 상세 맨 위 정체 블록. Installed 와 Attention 이 같은 함수를 쓴다.

use tasty_type_appearance::theme::Theme;

use super::{PluginMetaView, plugin_detail_meta, plugin_detail_name_row};
use crate::plugin_avatar::{PluginAvatarSize, plugin_avatar};

/// 정체 블록 입력.
pub struct PluginIdentityView<'a> {
    pub name: &'a str,
    pub version: &'a str,
    /// built-in 플러그인이면 Tag 문구. 아니면 None.
    pub builtin_tag: Option<&'a str>,
    pub meta: PluginMetaView<'a>,
}

/// 아바타(lg) 오른쪽에 이름 줄과 메타 줄을 `spacing_xs` 간격으로 쌓는다. 아바타와 글 열 사이는
/// `spacing_md` 다. 메타 줄의 homepage 링크를 눌렀으면 true.
pub fn plugin_detail_identity(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &PluginIdentityView<'_>,
) -> bool {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
        plugin_avatar(ui, theme, view.name, PluginAvatarSize::Detail);
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            plugin_detail_name_row(ui, theme, view.name, view.version, view.builtin_tag);
            plugin_detail_meta(ui, theme, &view.meta)
        })
        .inner
    })
    .inner
}
