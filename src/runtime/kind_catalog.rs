//! Kind metadata and opaque registration identity for presentation. Factories stay in the registry.
use super::surface_registry::{
    KindSource, PresetFieldSpec, RegisteredRendering, SurfaceKindDef, SurfaceKindRegistry,
};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};
#[derive(Clone)]
pub(crate) struct KindCatalog(Arc<SurfaceKindRegistry>);
#[derive(Clone, Debug)]
pub(crate) struct Registration(Weak<SurfaceKindDef>);
impl Registration {
    pub(crate) fn matches(&self, definition: &Arc<SurfaceKindDef>) -> bool {
        self.0.ptr_eq(&Arc::downgrade(definition))
    }
}
#[derive(Clone)]
pub(crate) struct KindMetadata {
    pub(crate) kind: &'static str,
    pub(crate) rendering: RegisteredRendering,
    pub(crate) source: KindSource,
    pub(crate) display_name_i18n_key: &'static str,
    pub(crate) icon: Option<String>,
    pub(crate) preset_fields: Vec<PresetFieldSpec>,
    pub(crate) param_aliases: HashMap<String, String>,
    pub(crate) default_params: HashMap<String, String>,
    pub(crate) consumes_egui_input: bool,
    pub(crate) zoomable: bool,
    pub(crate) egui_copy: bool,
    pub(crate) copy_path: bool,
    pub(crate) egui_paste: bool,
    pub(crate) name_from_param: Option<String>,
    pub(crate) records_recent: bool,
    pub(crate) convert_requires_input: bool,
    pub(crate) convert_input_popup: Option<String>,
    registration: Registration,
}
impl KindMetadata {
    pub(crate) fn registration(&self) -> Registration {
        self.registration.clone()
    }
    pub(crate) fn required_params(&self) -> impl Iterator<Item = &str> {
        self.preset_fields
            .iter()
            .filter_map(|field| match &field.target {
                super::surface_registry::PresetFieldTarget::Params(key) if field.required => {
                    Some(key.as_str())
                }
                _ => None,
            })
    }
    pub(crate) fn first_missing_required_param(&self, params: &serde_json::Value) -> Option<&str> {
        self.required_params().find(|key| {
            params
                .get(*key)
                .and_then(|value| value.as_str())
                .is_none_or(str::is_empty)
        })
    }
}

impl KindCatalog {
    pub(crate) fn new(registry: Arc<SurfaceKindRegistry>) -> Self {
        Self(registry)
    }
    pub(crate) fn get(&self, kind: &str) -> Option<Arc<KindMetadata>> {
        self.0.metadata(kind, false)
    }
    pub(crate) fn get_live(&self, kind: &str) -> Option<Arc<KindMetadata>> {
        self.0.metadata(kind, true)
    }
    pub(crate) fn contains(&self, kind: &str) -> bool {
        self.0.contains(kind)
    }
    pub(crate) fn withdrawn_by(&self, kind: &str) -> Option<String> {
        self.0.withdrawn_by(kind)
    }
    pub(crate) fn kinds_snapshot(&self) -> Vec<(&'static str, Arc<KindMetadata>)> {
        self.0.metadata_snapshot()
    }
    pub(super) fn metadata(def: Arc<SurfaceKindDef>) -> KindMetadata {
        KindMetadata {
            kind: def.kind,
            rendering: def.rendering.clone(),
            source: def.source.clone(),
            display_name_i18n_key: def.display_name_i18n_key,
            icon: def.icon.clone(),
            preset_fields: def.preset_fields.clone(),
            param_aliases: def.param_aliases.clone(),
            default_params: def.default_params.clone(),
            consumes_egui_input: def.consumes_egui_input,
            zoomable: def.zoomable,
            egui_copy: def.egui_copy,
            copy_path: def.copy_path,
            egui_paste: def.egui_paste,
            name_from_param: def.name_from_param.clone(),
            records_recent: def.records_recent,
            convert_requires_input: def.convert_requires_input,
            convert_input_popup: def.convert_input_popup.clone(),
            registration: Registration(Arc::downgrade(&def)),
        }
    }
}
