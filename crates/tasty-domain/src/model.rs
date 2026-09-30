//! 저널 전용 구조 모델. 이벤트를 적용한 결과이며 GUI·PTY·선택 상태를 담지 않는다.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use tasty_event_store::{BatchId, Revision};

use crate::ids::{CategoryId, PaneId, SurfaceId, TabId, WorkspaceId};

/// 분할 비율. f32의 비트를 그대로 보관해 인코딩 왕복에서 값이 바뀌지 않게 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Ratio(u32);

impl Ratio {
    pub fn from_f32(value: f32) -> Self {
        Self(value.to_bits())
    }

    pub fn to_f32(self) -> f32 {
        f32::from_bits(self.0)
    }

    pub fn bits(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

/// 새 노드를 기존 노드의 어느 쪽에 둘지.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Placement {
    Before,
    After,
}

/// 분할 트리. pane 배치와 tab 안의 surface 배치가 같은 형태를 쓴다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SplitTree<Id> {
    Leaf(Id),
    Split {
        direction: SplitDirection,
        ratio: Ratio,
        first: Box<SplitTree<Id>>,
        second: Box<SplitTree<Id>>,
    },
}

impl<Id: Copy + PartialEq> SplitTree<Id> {
    /// 왼쪽부터 깊이 우선 순서의 leaf.
    pub fn leaves(&self) -> Vec<Id> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out
    }

    fn collect(&self, out: &mut Vec<Id>) {
        match self {
            Self::Leaf(id) => out.push(*id),
            Self::Split { first, second, .. } => {
                first.collect(out);
                second.collect(out);
            }
        }
    }

    /// `target` leaf를 `target`과 `new` 두 leaf의 분할로 바꾼다. 없으면 false.
    pub(crate) fn split_leaf(
        &mut self,
        target: Id,
        new: Id,
        direction: SplitDirection,
        ratio: Ratio,
        placement: Placement,
    ) -> bool {
        match self {
            Self::Leaf(id) if *id == target => {
                let (first, second) = match placement {
                    Placement::Before => (new, target),
                    Placement::After => (target, new),
                };
                *self = Self::Split {
                    direction,
                    ratio,
                    first: Box::new(Self::Leaf(first)),
                    second: Box::new(Self::Leaf(second)),
                };
                true
            }
            Self::Leaf(_) => false,
            Self::Split { first, second, .. } => {
                first.split_leaf(target, new, direction, ratio, placement)
                    || second.split_leaf(target, new, direction, ratio, placement)
            }
        }
    }

    /// `target` leaf를 빼고 부모 분할을 형제로 접는다. 마지막 leaf는 뺄 수 없다.
    pub(crate) fn remove_leaf(&mut self, target: Id) -> RemoveLeaf {
        match self {
            Self::Leaf(id) if *id == target => RemoveLeaf::LastLeaf,
            Self::Leaf(_) => RemoveLeaf::NotFound,
            Self::Split { first, second, .. } => {
                let keep = if matches!(**first, Self::Leaf(id) if id == target) {
                    Some(second.as_ref().clone())
                } else if matches!(**second, Self::Leaf(id) if id == target) {
                    Some(first.as_ref().clone())
                } else {
                    None
                };
                if let Some(keep) = keep {
                    *self = keep;
                    return RemoveLeaf::Removed;
                }
                match first.remove_leaf(target) {
                    RemoveLeaf::NotFound => second.remove_leaf(target),
                    other => other,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RemoveLeaf {
    Removed,
    NotFound,
    LastLeaf,
}

/// surface kind 고유 자료의 참조. 내용은 journal의 불변 payload에 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DataRef(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub category: CategoryId,
    pub layout: SplitTree<PaneId>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pane {
    pub workspace: WorkspaceId,
    pub tabs: Vec<TabId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tab {
    pub pane: PaneId,
    pub name: String,
    pub layout: SplitTree<SurfaceId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Surface {
    pub tab: TabId,
    /// terminal·markdown·plugin kind 등. 해석은 kind 소유자가 한다.
    pub kind: String,
    pub data: Option<DataRef>,
    pub metadata: BTreeMap<String, String>,
}

/// 모델에 마지막으로 적용한 journal 위치.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Applied {
    /// 마지막으로 적용한 batch. 구조 이벤트가 없는 batch도 포함한다.
    pub batch: Option<BatchId>,
    /// 구조 stream의 마지막 적용 revision.
    pub revision: Option<Revision>,
}

/// 구조 stream을 적용한 결과. 순서는 `*_order`와 각 부모의 목록이 정한다.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct JournalModel {
    pub applied: Applied,
    pub categories: BTreeMap<CategoryId, Category>,
    pub category_order: Vec<CategoryId>,
    pub workspaces: BTreeMap<WorkspaceId, Workspace>,
    pub workspace_order: Vec<WorkspaceId>,
    pub panes: BTreeMap<PaneId, Pane>,
    pub tabs: BTreeMap<TabId, Tab>,
    pub surfaces: BTreeMap<SurfaceId, Surface>,
}
