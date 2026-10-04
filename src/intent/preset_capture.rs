//! Freeze preset structure and live content; resolve lazy immutable data on the journal worker.
//! Neither phase activates a terminal/plugin or copies runtime IDs into the preset wire format.
use crate::intent::ClonedPreset;
use crate::model::{Deferred, EmptySurface, Pane, PaneNode, SplitDirection, SurfaceLayout, Tab};
use crate::runtime::engine_access::EngineRef;
use crate::runtime::journal_payload::SavedSurfaceSource;
use crate::runtime::surface_registry::SurfaceKindRegistry;
use tasty_presets::{
    PanePreset, PresetKind, PresetPane, PresetPaneNode, PresetSplitDirection, PresetSurface,
    PresetSurfaceLayout, PresetTab, TabPreset, WorkspacePreset,
};

#[derive(Debug, Clone)]
pub(crate) struct PresetCaptureDraft {
    preset: ClonedPreset,
    pending: Vec<(usize, SavedSurfaceSource)>,
    pub(crate) base_name: String,
}
impl PresetCaptureDraft {
    pub(crate) fn references(&self) -> Vec<tasty_core::DataRef> {
        self.pending
            .iter()
            .flat_map(|(_, source)| source.data.into_iter().chain(source.creation_seed))
            .collect()
    }
    pub(crate) fn weight(&self) -> usize {
        let bytes = match &self.preset {
            ClonedPreset::Workspace(value) => serde_json::to_vec(value),
            ClonedPreset::Pane(value) => serde_json::to_vec(value),
            ClonedPreset::Tab(value) => serde_json::to_vec(value),
        };
        bytes.map_or(usize::MAX, |bytes| {
            bytes
                .len()
                .saturating_add(self.base_name.len())
                .saturating_add(
                    self.pending
                        .iter()
                        .map(|(_, source)| source.kind.len().saturating_add(64))
                        .sum::<usize>(),
                )
        })
    }

    pub(crate) fn finish_live(self) -> Result<(ClonedPreset, String), String> {
        if !self.pending.is_empty() {
            return Err("preset capture requires journal payload resolution".into());
        }
        Ok((self.preset, self.base_name))
    }

    /// Resolves the frozen refs rather than looking up a possibly replaced live surface.
    pub(crate) fn resolve(
        mut self,
        store: &tasty_event_store::EventStore,
    ) -> Result<(ClonedPreset, String), String> {
        let mut resolved = std::collections::BTreeMap::new();
        let mut remaining = crate::runtime::journal_product::MAX_PRESET_CAPTURE_BYTES;
        for (index, source) in &self.pending {
            resolved.insert(
                *index,
                crate::runtime::journal_payload::preset_surface(store, source, &mut remaining)?,
            );
        }
        let mut index = 0;
        visit_preset(&mut self.preset, &mut |surface| {
            if let Some(value) = resolved.remove(&index) {
                *surface = value;
            }
            index += 1;
        });
        if !resolved.is_empty() {
            return Err("preset payload positions differ from the frozen layout".into());
        }
        Ok((self.preset, self.base_name))
    }
}

pub(crate) fn capture_draft(
    presentation: &dyn crate::model::StructurePresentation,
    engine: &EngineRef<'_>,
    kind: PresetKind,
    source: u32,
) -> Result<PresetCaptureDraft, String> {
    let mut builder = Builder {
        engine,
        registry: &engine.runtime.surface_registry,
        presentation,
        pending: Vec::new(),
        index: 0,
    };
    let (preset, base_name) = match kind {
        PresetKind::Workspace => {
            let workspace = engine
                .workspaces()
                .into_iter()
                .find(|workspace| workspace.id == source)
                .ok_or_else(|| format!("Workspace id {source} not found"))?;
            (
                ClonedPreset::Workspace(WorkspacePreset {
                    name: String::new(),
                    subtitle: workspace.subtitle.clone(),
                    description: workspace.description.clone(),
                    layout: builder.panes(workspace.pane_layout())?,
                }),
                if workspace.name.is_empty() {
                    "workspace".into()
                } else {
                    workspace.name.clone()
                },
            )
        }
        PresetKind::Pane => {
            let pane = engine
                .find_pane_by_id(source)
                .ok_or_else(|| format!("Pane id {source} not found"))?;
            (
                ClonedPreset::Pane(PanePreset {
                    name: String::new(),
                    pane: builder.pane(pane)?,
                }),
                "pane".into(),
            )
        }
        PresetKind::Tab => {
            let pane = engine
                .find_pane_for_tab(source)
                .and_then(|id| engine.find_pane_by_id(id))
                .ok_or_else(|| format!("Tab id {source} not found"))?;
            let tab = pane
                .tabs
                .iter()
                .find(|tab| tab.id == source)
                .ok_or_else(|| format!("Tab id {source} not found"))?;
            let name = tab
                .explicit_name
                .clone()
                .unwrap_or_else(|| tab.name.clone());
            (
                ClonedPreset::Tab(TabPreset {
                    name: String::new(),
                    tab: builder.tab(tab)?,
                }),
                if name.is_empty() { "tab".into() } else { name },
            )
        }
    };
    Ok(PresetCaptureDraft {
        preset,
        pending: builder.pending,
        base_name,
    })
}

struct Builder<'a, 'engine> {
    engine: &'a EngineRef<'engine>,
    registry: &'a SurfaceKindRegistry,
    presentation: &'a dyn crate::model::StructurePresentation,
    pending: Vec<(usize, SavedSurfaceSource)>,
    index: usize,
}
impl Builder<'_, '_> {
    fn panes(&mut self, node: &PaneNode) -> Result<PresetPaneNode, String> {
        Ok(match node {
            PaneNode::Leaf(pane) => PresetPaneNode::Leaf {
                pane: self.pane(pane)?,
            },
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => PresetPaneNode::Split {
                direction: split(*direction),
                ratio: *ratio,
                first: Box::new(self.panes(first)?),
                second: Box::new(self.panes(second)?),
            },
        })
    }
    fn pane(&mut self, pane: &Pane) -> Result<PresetPane, String> {
        if pane.tabs.is_empty() {
            return Err("preset pane has no tabs".into());
        }
        Ok(PresetPane {
            tabs: pane
                .tabs
                .iter()
                .map(|tab| self.tab(tab))
                .collect::<Result<_, _>>()?,
            active_tab: self.presentation.tab_index(pane).min(pane.tabs.len() - 1),
        })
    }
    fn tab(&mut self, tab: &Tab) -> Result<PresetTab, String> {
        Ok(PresetTab {
            explicit_name: tab.explicit_name.clone(),
            layout: self.layout(tab.layout())?,
        })
    }
    fn layout(&mut self, layout: &SurfaceLayout) -> Result<PresetSurfaceLayout, String> {
        Ok(match layout {
            SurfaceLayout::Leaf(surface) => PresetSurfaceLayout::Leaf {
                surface: self.surface(surface.id)?,
            },
            SurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
                ..
            } => PresetSurfaceLayout::Split {
                direction: split(*direction),
                ratio: *ratio,
                first: Box::new(self.layout(first)?),
                second: Box::new(self.layout(second)?),
            },
        })
    }
    fn surface(&mut self, id: u32) -> Result<PresetSurface, String> {
        let index = self.index;
        self.index += 1;
        let owner = self
            .engine
            .runtime
            .surfaces
            .get(&id)
            .ok_or("preset surface instance missing")?;
        if let Some(lazy) = owner
            .as_any()
            .downcast_ref::<crate::runtime::surface_restorer::JournalPlaceholder>()
        {
            self.pending.push((
                index,
                SavedSurfaceSource {
                    kind: lazy.kind.clone(),
                    data: lazy.data,
                    creation_seed: lazy.creation_seed,
                },
            ));
            return Ok(blank(&lazy.kind));
        }
        if let Some(empty) = owner.as_any().downcast_ref::<EmptySurface>() {
            match &empty.deferred {
                Some(Deferred::Plugin(saved)) => {
                    return Ok(PresetSurface {
                        params: saved.snapshot.clone(),
                        ..blank(&saved.kind)
                    });
                }
                Some(Deferred::Terminal(spawn)) => {
                    return Ok(PresetSurface {
                        cwd: spawn
                            .working_dir
                            .as_ref()
                            .map(|cwd| cwd.to_string_lossy().into_owned()),
                        ..blank("terminal")
                    });
                }
                None => {}
            }
        }
        if owner.kind() == "terminal" {
            return Ok(PresetSurface {
                cwd: self
                    .engine
                    .local_surface_cwd(id)
                    .map(|cwd| cwd.to_string_lossy().into_owned()),
                ..blank("terminal")
            });
        }
        Ok(match self.registry.get(owner.kind()) {
            Some(definition) => PresetSurface {
                params: (definition.snapshot)(owner.as_ref())
                    .unwrap_or_else(|| serde_json::json!({})),
                ..blank(owner.kind())
            },
            None => blank("empty"),
        })
    }
}
fn blank(kind: &str) -> PresetSurface {
    PresetSurface {
        id: None,
        kind: kind.into(),
        cwd: None,
        startup_command: None,
        params: serde_json::json!({}),
    }
}
fn split(direction: SplitDirection) -> PresetSplitDirection {
    match direction {
        SplitDirection::Horizontal => PresetSplitDirection::Horizontal,
        SplitDirection::Vertical => PresetSplitDirection::Vertical,
    }
}
fn visit_layout(layout: &mut PresetSurfaceLayout, visit: &mut impl FnMut(&mut PresetSurface)) {
    match layout {
        PresetSurfaceLayout::Leaf { surface } => visit(surface),
        PresetSurfaceLayout::Split { first, second, .. } => {
            visit_layout(first, visit);
            visit_layout(second, visit);
        }
    }
}
fn visit_pane(pane: &mut PresetPane, visit: &mut impl FnMut(&mut PresetSurface)) {
    for tab in &mut pane.tabs {
        visit_layout(&mut tab.layout, visit);
    }
}
fn visit_panes(node: &mut PresetPaneNode, visit: &mut impl FnMut(&mut PresetSurface)) {
    match node {
        PresetPaneNode::Leaf { pane } => visit_pane(pane, visit),
        PresetPaneNode::Split { first, second, .. } => {
            visit_panes(first, visit);
            visit_panes(second, visit);
        }
    }
}
fn visit_preset(preset: &mut ClonedPreset, visit: &mut impl FnMut(&mut PresetSurface)) {
    match preset {
        ClonedPreset::Workspace(value) => visit_panes(&mut value.layout, visit),
        ClonedPreset::Pane(value) => visit_pane(&mut value.pane, visit),
        ClonedPreset::Tab(value) => visit_layout(&mut value.tab.layout, visit),
    }
}
