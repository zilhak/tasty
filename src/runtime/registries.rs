//! Process-owned registry instances. Engines and PluginManager receive the same shared handles.
use std::{path::Path, sync::Arc};
#[derive(Clone)]
pub(crate) struct RuntimeRegistries {
    pub(crate) surface_registry: Arc<super::surface_registry::SurfaceKindRegistry>,
    pub(crate) file_format: Arc<crate::file::format::FileFormatRegistry>,
    pub(crate) file_handler: Arc<crate::file::handler::FileHandlerRegistry>,
    pub(crate) plugin_hook_events: Arc<crate::core::hook_event_registry::PluginHookEventRegistry>,
    #[cfg(feature = "gui")]
    pub(crate) explorer_favorites: Arc<crate::core::explorer_favorites::SharedExplorerFavorites>,
}
impl RuntimeRegistries {
    pub(crate) fn new(user_config: Option<&Path>) -> Self {
        let surface_registry = super::surface_registry::SurfaceKindRegistry::new();
        super::surface_registry::register_builtin_kinds(&surface_registry);
        let file_format = Arc::new(crate::file::format::FileFormatRegistry::new());
        file_format.install_host_defaults(crate::file::format::HOST_DEFAULTS_TOML);
        let file_handler = Arc::new(crate::file::handler::FileHandlerRegistry::new());
        file_handler.install_host_defaults(crate::file::handler::HOST_DEFAULTS_TOML);
        if let Some(path) = user_config {
            file_format.install_user_config(path);
            file_handler.install_user_config(path);
        }
        file_handler.attach_detector_info(file_format.clone());
        Self {
            surface_registry: Arc::new(surface_registry),
            file_format,
            file_handler,
            plugin_hook_events: Arc::new(
                crate::core::hook_event_registry::PluginHookEventRegistry::new(),
            ),
            #[cfg(feature = "gui")]
            explorer_favorites: Arc::new(
                crate::core::explorer_favorites::SharedExplorerFavorites::load(),
            ),
        }
    }
}
