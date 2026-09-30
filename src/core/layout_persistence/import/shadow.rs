//! CoreState 쪽 구조 digest 입력. CoreState를 직접 읽은 정규 표현과, capture → importer → journal
//! 모델을 거친 정규 표현을 만든다. 정규 표현과 비교 범위는 [`crate::runtime::shadow_digest`]가 정한다.
//!
//! 시험 전용이며 제품 경로에 연결하지 않는다.

mod tests;

use tasty_domain::{DataRef, JournalModel};
use tasty_event_store::{EventStore, PayloadRef, WriterEpoch};

use super::surface_data::SurfaceData;
use super::{ImportError, ImportOutcome, ScrollbackSource, import_slot};
use crate::core::CoreState;
use crate::core::layout_persistence::LayoutSlotId;
use crate::core::layout_persistence::schema::SavedLayout;
use crate::model::{Deferred, EmptySurface, PaneNode, Surface, SurfaceLayout, TerminalSurface};
use crate::runtime::journal;
use crate::runtime::shadow_digest::{
    CanonCategory, CanonData, CanonPane, CanonSurface, CanonTab, CanonTree, CanonWorkspace,
    Canonical, IdMode, ResolveData, direction_name, fnv_hex, sorted_value,
};

/// CoreState의 정규 표현. ID는 원래 값이고 surface 저장 자료는 비교하지 않는다.
/// mirror workspace와 선택·파생 값은 뺀다.
pub(super) fn core_canonical(engine: &CoreState) -> Canonical {
    let mut defects = Vec::new();
    let categories = engine
        .categories
        .iter()
        .map(|c| CanonCategory {
            id: c.id,
            name: c.name.clone(),
        })
        .collect();
    let workspaces = engine
        .workspaces
        .iter()
        .filter(|ws| !ws.mirror)
        .map(|ws| {
            let mut leaves = Vec::new();
            pane_leaves(ws.pane_layout(), &mut leaves);
            let panes = leaves
                .into_iter()
                .map(|pane| CanonPane {
                    id: pane.id,
                    tabs: pane
                        .tabs
                        .iter()
                        .map(|tab| {
                            let mut surfaces = Vec::new();
                            surface_leaves(tab.layout(), &mut surfaces);
                            CanonTab {
                                id: tab.id,
                                name: tab.name.clone(),
                                explicit_name: tab.explicit_name.clone(),
                                layout: surface_tree(tab.layout(), &mut defects),
                                surfaces: surfaces
                                    .into_iter()
                                    .map(|s| CanonSurface {
                                        id: surface_id(s, &mut defects),
                                        kind: core_kind(s),
                                        data: CanonData::NotCompared,
                                        metadata: Default::default(),
                                    })
                                    .collect(),
                            }
                        })
                        .collect(),
                })
                .collect();
            CanonWorkspace {
                id: ws.id,
                name: ws.name.clone(),
                category: ws.category,
                subtitle: ws.subtitle.clone(),
                description: ws.description.clone(),
                attach_mapping: ws.attach_mapping.as_ref().map(|m| {
                    sorted_value(&serde_json::to_value(m).unwrap_or(serde_json::Value::Null))
                }),
                metadata: Default::default(),
                layout: pane_tree(ws.pane_layout()),
                panes,
            }
        })
        .collect();
    Canonical {
        ids: IdMode::Exact,
        categories,
        workspaces,
        defects,
    }
}

/// 저장 형식이 정하는 kind. 대기 중인 terminal은 terminal, plugin 대기는 기다리는 kind다.
fn core_kind(surface: &dyn Surface) -> String {
    if surface.as_any().is::<TerminalSurface>() {
        return super::TERMINAL_KIND.to_owned();
    }
    if let Some(empty) = surface.as_any().downcast_ref::<EmptySurface>() {
        match &empty.deferred {
            Some(Deferred::Terminal(_)) => return super::TERMINAL_KIND.to_owned(),
            Some(Deferred::Plugin(plugin)) => return plugin.kind.clone(),
            None => {}
        }
    }
    surface.kind().to_owned()
}

/// capture가 이 kind를 그대로 저장하는지. terminal은 registry를 보지 않고 저장하며, 그 밖의 surface는
/// capture와 같은 registry 조회(철회된 정의 포함)로 정한다. 대기 plugin은 등록 여부와 관계없이
/// kind를 유지하지만 kind 문자열만으로는 구별하지 못해 registry 판정을 따른다.
pub(super) fn capture_keeps_kind(engine: &CoreState, kind: &str) -> bool {
    kind == super::TERMINAL_KIND || engine.surface_registry.get(kind).is_some()
}

fn surface_id(surface: &dyn Surface, defects: &mut Vec<String>) -> u32 {
    surface.surface_id().unwrap_or_else(|| {
        defects.push(format!("a {} surface has no id", surface.kind()));
        u32::MAX
    })
}

fn pane_leaves<'a>(node: &'a PaneNode, out: &mut Vec<&'a crate::model::Pane>) {
    match node {
        PaneNode::Leaf(pane) => out.push(pane),
        PaneNode::Split { first, second, .. } => {
            pane_leaves(first, out);
            pane_leaves(second, out);
        }
    }
}

fn surface_leaves<'a>(node: &'a SurfaceLayout, out: &mut Vec<&'a dyn Surface>) {
    match node {
        SurfaceLayout::Leaf(surface) => out.push(surface.as_ref()),
        SurfaceLayout::Split { first, second, .. } => {
            surface_leaves(first, out);
            surface_leaves(second, out);
        }
    }
}

fn pane_tree(node: &PaneNode) -> CanonTree {
    match node {
        PaneNode::Leaf(pane) => CanonTree::Leaf(pane.id),
        PaneNode::Split {
            direction,
            ratio,
            first,
            second,
        } => CanonTree::Split {
            direction: direction_name(*direction),
            ratio: ratio.to_bits(),
            first: Box::new(pane_tree(first)),
            second: Box::new(pane_tree(second)),
        },
    }
}

/// `focus_second`는 포커스 hint라 담지 않는다.
fn surface_tree(node: &SurfaceLayout, defects: &mut Vec<String>) -> CanonTree {
    match node {
        SurfaceLayout::Leaf(surface) => CanonTree::Leaf(surface_id(surface.as_ref(), defects)),
        SurfaceLayout::Split {
            direction,
            ratio,
            first,
            second,
            ..
        } => CanonTree::Split {
            direction: direction_name(*direction),
            ratio: ratio.to_bits(),
            first: Box::new(surface_tree(first, defects)),
            second: Box::new(surface_tree(second, defects)),
        },
    }
}

/// CoreState를 슬롯으로 capture해 journal의 그 슬롯 엔진 stream에 가져온 뒤 그 모델을 읽는다.
/// capture는 scrollback 저장 ID를 새로 정할 수 있어 engine을 바꿀 수 있다.
pub(super) fn capture_and_import(
    engine: &mut CoreState,
    store: &mut EventStore,
    epoch: WriterEpoch,
    slot: LayoutSlotId,
    scrollback: &dyn ScrollbackSource,
) -> Result<(ImportOutcome, JournalModel), ImportError> {
    let layout = SavedLayout::capture(engine, 0);
    let json = serde_json::to_string(&layout).map_err(ImportError::Mapping)?;
    let outcome = import_slot(store, epoch, slot, &json, scrollback)?;
    let model = journal::load(store, &journal::engine_stream(slot))?;
    Ok((outcome, model))
}

/// surface 저장 자료를 kind 소유자의 형식으로 해석한다. scrollback은 길이와 해시로 줄인다.
pub(super) struct DecodedData<'a>(pub(super) &'a EventStore);

impl ResolveData for DecodedData<'_> {
    fn resolve(&self, data: DataRef) -> CanonData {
        let bytes = match self.0.read_payload(PayloadRef(data.0)) {
            Ok(bytes) => bytes,
            Err(error) => return CanonData::Unreadable(error.to_string()),
        };
        match SurfaceData::decode(&bytes) {
            Ok(SurfaceData::Terminal {
                cwd,
                restore_command,
                scrollback_ref,
                scrollback,
            }) => CanonData::Decoded(serde_json::json!({
                "terminal": {
                    "cwd": cwd,
                    "restore_command": restore_command,
                    "scrollback_ref": scrollback_ref,
                    "scrollback": scrollback.map(|b| serde_json::json!({
                        "len": b.len(),
                        "hash": fnv_hex(&b),
                    })),
                }
            })),
            Ok(SurfaceData::Generic { data }) => {
                CanonData::Decoded(serde_json::json!({ "generic": sorted_value(&data) }))
            }
            Err(error) => CanonData::Unreadable(error.to_string()),
        }
    }
}
