//! Bind a path receipt to the exact preset, leaf, field and edit generation that requested it.
use super::{KindCatalog, PresetFieldInput, PresetFieldSpec, PresetFieldTarget, SurfaceCfg};
use crate::view::file_chooser::FileChooserMode;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);
pub(super) fn next_generation() -> u64 {
    NEXT_GENERATION.fetch_add(1, Ordering::Relaxed)
}

#[derive(Clone, Debug)]
pub(crate) struct FieldBrowseRequest {
    preset_key: String,
    leaf_id: usize,
    generation: u64,
    kind: String,
    field_id: String,
    target: PresetFieldTarget,
    input: PresetFieldInput,
    pub(crate) mode: FileChooserMode,
    pub(crate) start: String,
}
impl SurfaceCfg {
    pub(super) fn request_browse(&mut self, field: &PresetFieldSpec) {
        let mode = match field.input {
            PresetFieldInput::Dir => FileChooserMode::Folder,
            PresetFieldInput::FilePath => FileChooserMode::Open,
            _ => return,
        };
        self.generation = next_generation();
        self.browse_request = Some(FieldBrowseRequest {
            preset_key: self.preset_key.clone(),
            leaf_id: self.leaf_id,
            generation: self.generation,
            kind: self.draft.kind().to_string(),
            field_id: field.id.clone(),
            target: field.target.clone(),
            input: field.input,
            mode,
            start: self.draft.value(&field.target),
        });
    }
    pub(crate) fn take_browse_request(&mut self) -> Option<FieldBrowseRequest> {
        self.browse_request.take()
    }
    pub(crate) fn accepts_browse(
        &self,
        request: &FieldBrowseRequest,
        catalog: &KindCatalog,
    ) -> bool {
        self.preset_key == request.preset_key
            && self.leaf_id == request.leaf_id
            && self.generation == request.generation
            && self.draft.kind() == request.kind
            && catalog.fields(self.draft.kind()).iter().any(|field| {
                field.id == request.field_id
                    && field.target == request.target
                    && field.input == request.input
            })
    }
    pub(crate) fn apply_browse(
        &mut self,
        request: &FieldBrowseRequest,
        path: String,
        catalog: &KindCatalog,
    ) -> bool {
        if !self.accepts_browse(request, catalog) {
            return false;
        }
        self.draft.set_value(&request.target, path);
        self.generation = next_generation();
        true
    }
    pub(super) fn switch_kind(&mut self, kind: &str, catalog: &KindCatalog) {
        if kind != self.draft.kind() {
            self.generation = next_generation();
            self.browse_request = None;
            self.draft.switch_kind(kind, catalog);
        }
    }
}
