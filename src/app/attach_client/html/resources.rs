//! HTML plugin materialization carries only a remote marker; it never supplies a local URL.
use crate::{
    model::{DeferredPlugin, EmptySurface, Surface},
    runtime::surface_registry::{KindSource, SurfaceKindRegistry},
};
use serde_json::Value;
pub(in crate::app::attach_client) fn available(registry: &SurfaceKindRegistry) -> bool {
    super::viewer_available()
        && registry.get_live("html").is_some_and(
            |def| matches!(&def.source, KindSource::Plugin(id) if id == "com.tasty.html"),
        )
}
fn params(descriptor: &Value) -> Value {
    serde_json::json!({"display_name":descriptor.get("display_name").and_then(Value::as_str).unwrap_or("HTML"),"remote":{"html_entry":true,"source":descriptor.get("source")}})
}
pub(in crate::app::attach_client) fn create(
    descriptor: &Value,
    local: u32,
    registry: &SurfaceKindRegistry,
) -> Option<Box<dyn Surface>> {
    if !available(registry) {
        return None;
    }
    let definition = registry.get_live("html")?;
    match (definition.create)(local, None, &params(descriptor))
        .and_then(|prepared| prepared.publish())
    {
        Ok(surface) => Some(surface),
        Err(error) => {
            tracing::warn!(surface = local, %error, "remote HTML surface creation failed");
            None
        }
    }
}
pub(in crate::app::attach_client) fn deferred(descriptor: &Value, local: u32) -> Box<dyn Surface> {
    Box::new(EmptySurface::new_deferred_plugin(
        local,
        DeferredPlugin {
            kind: "html".into(),
            snapshot: params(descriptor),
        },
    ))
}
pub(in crate::app::attach_client) fn destroy(
    manager: &mut Option<crate::plugin::PluginManager>,
    ids: impl IntoIterator<Item = u32>,
) {
    if let Some(manager) = manager {
        for id in ids {
            manager.destroy_remote_surface(id, Some("html"));
        }
    }
}
