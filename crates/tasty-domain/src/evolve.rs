//! pure evolve. batch 하나를 모델 사본에 모두 적용한 뒤에만 교체하므로 일부만 반영되지 않는다.
//!
//! 이벤트는 decide가 확정한 사실이다. 참조가 맞지 않으면 추측해 고치지 않고 오류로 중단한다.

use tasty_event_store::{BatchId, Revision, StoredBatch};

use crate::codec::{CodecError, decode_event};
use crate::event::{DomainEvent, MetadataTarget, SplitSpec, SurfaceSpec};
use crate::ids::{CategoryId, PaneId, SurfaceId, TabId, WorkspaceId};
use crate::model::{Category, JournalModel, Pane, RemoveLeaf, SplitTree, Surface, Tab, Workspace};

/// 구조 이벤트를 담는 stream. 다른 stream의 이벤트는 이 모델이 적용하지 않는다.
pub const STRUCTURE_STREAM: &str = "structure";

#[derive(Debug, thiserror::Error)]
pub enum EvolveError {
    #[error(transparent)]
    Codec(#[from] CodecError),

    #[error("batch {got} is not after the last applied batch {last}")]
    StaleBatch { last: BatchId, got: BatchId },

    #[error("structure revision {got} does not follow {expected}")]
    RevisionGap { expected: Revision, got: Revision },

    #[error("{0} does not exist")]
    Missing(String),

    #[error("{0} already exists")]
    Duplicate(String),

    #[error("index {index} is out of range for {len} items")]
    IndexOutOfRange { index: usize, len: usize },

    #[error("{0} is the last node of its layout")]
    LastLeaf(String),

    #[error("{0} still has workspaces")]
    CategoryNotEmpty(CategoryId),

    #[error("{0} cannot be moved next to itself")]
    SelfTarget(String),

    #[error("metadata key {key} is not set on {target}")]
    MissingKey { target: String, key: String },
}

type Result<T> = std::result::Result<T, EvolveError>;

/// batch의 구조 이벤트를 순서대로 적용한다. 오류이면 모델은 호출 전 그대로다.
pub fn evolve(model: &mut JournalModel, batch: &StoredBatch) -> Result<()> {
    let batch_id = batch.cut.batch_id;
    if let Some(last) = model.applied.batch
        && batch_id <= last
    {
        return Err(EvolveError::StaleBatch {
            last,
            got: batch_id,
        });
    }
    let mut next = model.clone();
    for stored in &batch.events {
        if stored.stream_id.as_str() != STRUCTURE_STREAM {
            continue;
        }
        let expected = next.applied.revision.unwrap_or(0) + 1;
        if stored.stream_revision != expected {
            return Err(EvolveError::RevisionGap {
                expected,
                got: stored.stream_revision,
            });
        }
        apply(&mut next, decode_event(&stored.payload)?)?;
        next.applied.revision = Some(stored.stream_revision);
    }
    next.applied.batch = Some(batch_id);
    *model = next;
    Ok(())
}

fn apply(m: &mut JournalModel, event: DomainEvent) -> Result<()> {
    match event {
        DomainEvent::CategoryCreated { id, name, index } => create_category(m, id, name, index),
        DomainEvent::CategoryRenamed { id, name } => {
            get_mut(&mut m.categories, id)?.name = name;
            Ok(())
        }
        DomainEvent::CategoryMoved { id, index } => move_in(&mut m.category_order, id, index),
        DomainEvent::CategoryClosed { id } => close_category(m, id),
        DomainEvent::WorkspaceCreated {
            id,
            name,
            category,
            index,
            pane,
        } => create_workspace(m, id, name, category, index, pane),
        DomainEvent::WorkspaceRenamed { id, name } => {
            get_mut(&mut m.workspaces, id)?.name = name;
            Ok(())
        }
        DomainEvent::WorkspaceMoved {
            id,
            category,
            index,
        } => move_workspace(m, id, category, index),
        DomainEvent::WorkspaceClosed { id } => close_workspace(m, id),
        DomainEvent::PaneSplit {
            target,
            pane,
            split,
        } => split_pane(m, target, pane, split),
        DomainEvent::PaneMoved { id, target, split } => move_pane(m, id, target, split),
        DomainEvent::PaneClosed { id } => close_pane(m, id),
        DomainEvent::TabCreated {
            id,
            pane,
            index,
            name,
            surface,
        } => create_tab(m, id, pane, index, name, surface),
        DomainEvent::TabRenamed { id, name } => {
            get_mut(&mut m.tabs, id)?.name = name;
            Ok(())
        }
        DomainEvent::TabMoved { id, pane, index } => move_tab(m, id, pane, index),
        DomainEvent::TabClosed { id } => close_tab(m, id),
        DomainEvent::SurfaceSplit {
            target,
            surface,
            split,
        } => split_surface(m, target, surface, split),
        DomainEvent::SurfaceMoved { id, target, split } => move_surface(m, id, target, split),
        DomainEvent::SurfaceClosed { id } => close_surface(m, id),
        DomainEvent::MetadataSet { target, key, value } => {
            metadata_mut(m, target)?.insert(key, value);
            Ok(())
        }
        DomainEvent::MetadataRemoved { target, key } => {
            match metadata_mut(m, target)?.remove(&key) {
                Some(_) => Ok(()),
                None => Err(EvolveError::MissingKey {
                    target: target_name(target),
                    key,
                }),
            }
        }
    }
}

fn create_category(m: &mut JournalModel, id: CategoryId, name: String, index: usize) -> Result<()> {
    ensure_absent(&m.categories, id)?;
    insert_at(&mut m.category_order, index, id)?;
    m.categories.insert(id, Category { name });
    Ok(())
}

fn close_category(m: &mut JournalModel, id: CategoryId) -> Result<()> {
    get(&m.categories, id)?;
    if m.workspaces.values().any(|w| w.category == id) {
        return Err(EvolveError::CategoryNotEmpty(id));
    }
    m.categories.remove(&id);
    m.category_order.retain(|c| *c != id);
    Ok(())
}

fn create_workspace(
    m: &mut JournalModel,
    id: WorkspaceId,
    name: String,
    category: CategoryId,
    index: usize,
    pane: PaneId,
) -> Result<()> {
    ensure_absent(&m.workspaces, id)?;
    ensure_absent(&m.panes, pane)?;
    get(&m.categories, category)?;
    insert_at(&mut m.workspace_order, index, id)?;
    m.workspaces.insert(
        id,
        Workspace {
            name,
            category,
            layout: SplitTree::Leaf(pane),
            metadata: Default::default(),
        },
    );
    m.panes.insert(
        pane,
        Pane {
            workspace: id,
            tabs: Vec::new(),
        },
    );
    Ok(())
}

fn move_workspace(
    m: &mut JournalModel,
    id: WorkspaceId,
    category: CategoryId,
    index: usize,
) -> Result<()> {
    get(&m.categories, category)?;
    get_mut(&mut m.workspaces, id)?.category = category;
    move_in(&mut m.workspace_order, id, index)
}

fn close_workspace(m: &mut JournalModel, id: WorkspaceId) -> Result<()> {
    let workspace = remove(&mut m.workspaces, id)?;
    m.workspace_order.retain(|w| *w != id);
    for pane in workspace.layout.leaves() {
        drop_pane(m, pane)?;
    }
    Ok(())
}

fn split_pane(m: &mut JournalModel, target: PaneId, pane: PaneId, split: SplitSpec) -> Result<()> {
    ensure_absent(&m.panes, pane)?;
    let workspace = get(&m.panes, target)?.workspace;
    attach_pane(m, workspace, target, pane, split)?;
    m.panes.insert(
        pane,
        Pane {
            workspace,
            tabs: Vec::new(),
        },
    );
    Ok(())
}

fn move_pane(m: &mut JournalModel, id: PaneId, target: PaneId, split: SplitSpec) -> Result<()> {
    if id == target {
        return Err(EvolveError::SelfTarget(id.to_string()));
    }
    let from = get(&m.panes, id)?.workspace;
    let to = get(&m.panes, target)?.workspace;
    detach_pane(m, from, id)?;
    attach_pane(m, to, target, id, split)?;
    get_mut(&mut m.panes, id)?.workspace = to;
    Ok(())
}

fn close_pane(m: &mut JournalModel, id: PaneId) -> Result<()> {
    let workspace = get(&m.panes, id)?.workspace;
    detach_pane(m, workspace, id)?;
    drop_pane(m, id)
}

fn attach_pane(
    m: &mut JournalModel,
    workspace: WorkspaceId,
    target: PaneId,
    pane: PaneId,
    split: SplitSpec,
) -> Result<()> {
    let layout = &mut get_mut(&mut m.workspaces, workspace)?.layout;
    if layout.split_leaf(target, pane, split.direction, split.ratio, split.placement) {
        Ok(())
    } else {
        Err(EvolveError::Missing(format!("{target} in {workspace}")))
    }
}

fn detach_pane(m: &mut JournalModel, workspace: WorkspaceId, pane: PaneId) -> Result<()> {
    let layout = &mut get_mut(&mut m.workspaces, workspace)?.layout;
    removed(layout.remove_leaf(pane), pane.to_string())
}

/// pane과 소속 tab·surface를 지운다. 배치에서는 호출자가 이미 뺐다.
fn drop_pane(m: &mut JournalModel, id: PaneId) -> Result<()> {
    let pane = remove(&mut m.panes, id)?;
    for tab in pane.tabs {
        drop_tab(m, tab)?;
    }
    Ok(())
}

fn create_tab(
    m: &mut JournalModel,
    id: TabId,
    pane: PaneId,
    index: usize,
    name: String,
    surface: SurfaceSpec,
) -> Result<()> {
    ensure_absent(&m.tabs, id)?;
    ensure_absent(&m.surfaces, surface.id)?;
    insert_at(&mut get_mut(&mut m.panes, pane)?.tabs, index, id)?;
    m.tabs.insert(
        id,
        Tab {
            pane,
            name,
            layout: SplitTree::Leaf(surface.id),
        },
    );
    insert_surface(m, id, surface);
    Ok(())
}

fn move_tab(m: &mut JournalModel, id: TabId, pane: PaneId, index: usize) -> Result<()> {
    let from = get(&m.tabs, id)?.pane;
    get(&m.panes, pane)?;
    get_mut(&mut m.panes, from)?.tabs.retain(|t| *t != id);
    insert_at(&mut get_mut(&mut m.panes, pane)?.tabs, index, id)?;
    get_mut(&mut m.tabs, id)?.pane = pane;
    Ok(())
}

fn close_tab(m: &mut JournalModel, id: TabId) -> Result<()> {
    let pane = get(&m.tabs, id)?.pane;
    get_mut(&mut m.panes, pane)?.tabs.retain(|t| *t != id);
    drop_tab(m, id)
}

fn drop_tab(m: &mut JournalModel, id: TabId) -> Result<()> {
    let tab = remove(&mut m.tabs, id)?;
    for surface in tab.layout.leaves() {
        remove(&mut m.surfaces, surface)?;
    }
    Ok(())
}

fn split_surface(
    m: &mut JournalModel,
    target: SurfaceId,
    surface: SurfaceSpec,
    split: SplitSpec,
) -> Result<()> {
    ensure_absent(&m.surfaces, surface.id)?;
    let tab = get(&m.surfaces, target)?.tab;
    attach_surface(m, tab, target, surface.id, split)?;
    insert_surface(m, tab, surface);
    Ok(())
}

fn move_surface(
    m: &mut JournalModel,
    id: SurfaceId,
    target: SurfaceId,
    split: SplitSpec,
) -> Result<()> {
    if id == target {
        return Err(EvolveError::SelfTarget(id.to_string()));
    }
    let from = get(&m.surfaces, id)?.tab;
    let to = get(&m.surfaces, target)?.tab;
    let layout = &mut get_mut(&mut m.tabs, from)?.layout;
    removed(layout.remove_leaf(id), id.to_string())?;
    attach_surface(m, to, target, id, split)?;
    get_mut(&mut m.surfaces, id)?.tab = to;
    Ok(())
}

fn close_surface(m: &mut JournalModel, id: SurfaceId) -> Result<()> {
    let tab = get(&m.surfaces, id)?.tab;
    let layout = &mut get_mut(&mut m.tabs, tab)?.layout;
    removed(layout.remove_leaf(id), id.to_string())?;
    m.surfaces.remove(&id);
    Ok(())
}

fn attach_surface(
    m: &mut JournalModel,
    tab: TabId,
    target: SurfaceId,
    surface: SurfaceId,
    split: SplitSpec,
) -> Result<()> {
    let layout = &mut get_mut(&mut m.tabs, tab)?.layout;
    if layout.split_leaf(
        target,
        surface,
        split.direction,
        split.ratio,
        split.placement,
    ) {
        Ok(())
    } else {
        Err(EvolveError::Missing(format!("{target} in {tab}")))
    }
}

fn insert_surface(m: &mut JournalModel, tab: TabId, spec: SurfaceSpec) {
    m.surfaces.insert(
        spec.id,
        Surface {
            tab,
            kind: spec.kind,
            data: spec.data,
            metadata: Default::default(),
        },
    );
}

fn metadata_mut(
    m: &mut JournalModel,
    target: MetadataTarget,
) -> Result<&mut std::collections::BTreeMap<String, String>> {
    Ok(match target {
        MetadataTarget::Workspace(id) => &mut get_mut(&mut m.workspaces, id)?.metadata,
        MetadataTarget::Surface(id) => &mut get_mut(&mut m.surfaces, id)?.metadata,
    })
}

fn target_name(target: MetadataTarget) -> String {
    match target {
        MetadataTarget::Workspace(id) => id.to_string(),
        MetadataTarget::Surface(id) => id.to_string(),
    }
}

fn removed(result: RemoveLeaf, name: String) -> Result<()> {
    match result {
        RemoveLeaf::Removed => Ok(()),
        RemoveLeaf::NotFound => Err(EvolveError::Missing(name)),
        RemoveLeaf::LastLeaf => Err(EvolveError::LastLeaf(name)),
    }
}

type Map<K, V> = std::collections::BTreeMap<K, V>;

fn get<K: Ord + Copy + std::fmt::Display, V>(map: &Map<K, V>, id: K) -> Result<&V> {
    map.get(&id)
        .ok_or_else(|| EvolveError::Missing(id.to_string()))
}

fn get_mut<K: Ord + Copy + std::fmt::Display, V>(map: &mut Map<K, V>, id: K) -> Result<&mut V> {
    map.get_mut(&id)
        .ok_or_else(|| EvolveError::Missing(id.to_string()))
}

fn remove<K: Ord + Copy + std::fmt::Display, V>(map: &mut Map<K, V>, id: K) -> Result<V> {
    map.remove(&id)
        .ok_or_else(|| EvolveError::Missing(id.to_string()))
}

fn ensure_absent<K: Ord + Copy + std::fmt::Display, V>(map: &Map<K, V>, id: K) -> Result<()> {
    if map.contains_key(&id) {
        Err(EvolveError::Duplicate(id.to_string()))
    } else {
        Ok(())
    }
}

fn insert_at<T>(list: &mut Vec<T>, index: usize, item: T) -> Result<()> {
    if index > list.len() {
        return Err(EvolveError::IndexOutOfRange {
            index,
            len: list.len(),
        });
    }
    list.insert(index, item);
    Ok(())
}

/// 목록에서 빼서 `index`에 다시 넣는다. `index`는 뺀 뒤 목록 기준이다.
fn move_in<T: PartialEq + Copy + std::fmt::Display>(
    list: &mut Vec<T>,
    item: T,
    index: usize,
) -> Result<()> {
    let Some(pos) = list.iter().position(|x| *x == item) else {
        return Err(EvolveError::Missing(item.to_string()));
    };
    list.remove(pos);
    insert_at(list, index, item)
}
