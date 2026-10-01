//! Borrowed plugin presentation data and instance-bound render outputs.
use crate::plugin::PluginManager;
use crate::plugin::manager::{BannerInstance, EguiMeshFrame, PopupInstance};
use tasty_plugin_protocol::{BannerSetContextParams, PopupSetContextParams, SharedBufferId};

#[derive(Clone, Copy)]
pub(crate) struct PluginDisplay<'a> {
    manager: &'a PluginManager,
}

impl<'a> PluginDisplay<'a> {
    pub(crate) fn new(manager: &'a PluginManager) -> Self { Self { manager } }
    pub(crate) fn attention_count(self) -> usize { self.manager.attention_count() }
    pub(crate) fn popup_instances(self) -> impl Iterator<Item = (u64, &'a PopupInstance)> {
        self.manager.popup_instances()
    }
    pub(crate) fn banner_instances(self) -> impl Iterator<Item = (u64, &'a BannerInstance)> {
        self.manager.banner_instances()
    }
    pub(crate) fn egui_mesh_frame(self, surface: u32) -> Option<&'a EguiMeshFrame> {
        self.manager.egui_mesh_frame(surface)
    }
    pub(crate) fn popup_mesh_frame(self, instance: u64) -> Option<&'a EguiMeshFrame> {
        self.manager.popup_mesh_frame(instance)
    }
    pub(crate) fn banner_mesh_frame(self, instance: u64) -> Option<&'a EguiMeshFrame> {
        self.manager.banner_mesh_frame(instance)
    }
    pub(crate) fn mesh_bytes(self, plugin: &str, buffer: SharedBufferId) -> Option<&'a [u8]> {
        let mapping = self.manager.plugin_buffer(plugin, buffer)?;
        // SAFETY: The manager borrow retains the mapping throughout rendering. The existing
        // mesh protocol's generation check does not exclude concurrent peer payload writes.
        Some(unsafe { mapping.as_slice() })
    }
    pub(crate) fn shortcut_epoch(self) -> (u64, u64) {
        (self.manager.command_registry.revision(), self.manager.config.shortcut_revision())
    }
    pub(crate) fn command_bindings(self, keys: &tasty_settings::KeybindingSettings) -> Vec<String> {
        crate::plugin_bridge::key_dispatch::all_command_bindings(self.manager, keys)
    }
}

/// The App rechecks the instance's plugin before delivering a frame's input or geometry.
/// Instance counters survive plugin reload; App never replaces an installed manager. A future
/// manager replacement must fence these queued outputs before resetting instance counters.
#[derive(Clone, Debug)]
pub(crate) enum PluginDisplayRequest {
    Popup { plugin: String, params: PopupSetContextParams },
    Banner { plugin: String, params: BannerSetContextParams },
}

impl PluginDisplayRequest {
    pub(crate) fn apply(&self, manager: &PluginManager) {
        match self {
            Self::Popup { plugin, params } => {
                if manager.popup_instances().any(|(id, instance)| {
                    id == params.instance_id && instance.plugin_id == *plugin
                }) {
                    manager.send_popup_set_context(plugin, params);
                }
            }
            Self::Banner { plugin, params } => {
                if manager.banner_instances().any(|(id, instance)| {
                    id == params.instance_id && instance.plugin_id == *plugin
                }) {
                    manager.send_banner_set_context(plugin, params);
                }
            }
        }
    }
}
