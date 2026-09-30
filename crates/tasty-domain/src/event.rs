//! 구조 stream의 도메인 이벤트. 이미 결정된 사실만 담으며 적용 중에 새 ID·시각을 만들지 않는다.

use serde::{Deserialize, Serialize};

use crate::ids::{CategoryId, PaneId, SurfaceId, TabId, WorkspaceId};
use crate::model::{DataRef, Placement, Ratio, SplitDirection};

/// metadata를 가진 대상.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MetadataTarget {
    Workspace(WorkspaceId),
    Surface(SurfaceId),
}

/// 새 surface의 kind와 자료 참조.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SurfaceSpec {
    pub id: SurfaceId,
    pub kind: String,
    pub data: Option<DataRef>,
}

/// 분할로 만들 새 노드의 위치.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitSpec {
    pub direction: SplitDirection,
    pub ratio: Ratio,
    pub placement: Placement,
}

/// 구조 이벤트. serde 태그는 codec의 type tag와 같다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum DomainEvent {
    #[serde(rename = "category.created")]
    CategoryCreated {
        id: CategoryId,
        name: String,
        index: usize,
    },
    #[serde(rename = "category.renamed")]
    CategoryRenamed { id: CategoryId, name: String },
    #[serde(rename = "category.moved")]
    CategoryMoved { id: CategoryId, index: usize },
    /// 소속 workspace가 없는 카테고리만 닫을 수 있다.
    #[serde(rename = "category.closed")]
    CategoryClosed { id: CategoryId },

    /// 첫 pane과 함께 만든다. tab은 뒤따르는 이벤트가 만든다.
    #[serde(rename = "workspace.created")]
    WorkspaceCreated {
        id: WorkspaceId,
        name: String,
        category: CategoryId,
        index: usize,
        pane: PaneId,
    },
    #[serde(rename = "workspace.renamed")]
    WorkspaceRenamed { id: WorkspaceId, name: String },
    /// 순서와 카테고리 소속을 함께 바꾼다.
    #[serde(rename = "workspace.moved")]
    WorkspaceMoved {
        id: WorkspaceId,
        category: CategoryId,
        index: usize,
    },
    /// 소속 pane·tab·surface를 함께 닫는다.
    #[serde(rename = "workspace.closed")]
    WorkspaceClosed { id: WorkspaceId },

    #[serde(rename = "pane.split")]
    PaneSplit {
        target: PaneId,
        pane: PaneId,
        split: SplitSpec,
    },
    /// pane을 원래 위치에서 빼서 `target` 옆에 다시 붙인다. 다른 workspace로도 옮길 수 있다.
    #[serde(rename = "pane.moved")]
    PaneMoved {
        id: PaneId,
        target: PaneId,
        split: SplitSpec,
    },
    /// 소속 tab·surface를 함께 닫는다. workspace의 마지막 pane은 닫을 수 없다.
    #[serde(rename = "pane.closed")]
    PaneClosed { id: PaneId },

    #[serde(rename = "tab.created")]
    TabCreated {
        id: TabId,
        pane: PaneId,
        index: usize,
        name: String,
        surface: SurfaceSpec,
    },
    #[serde(rename = "tab.renamed")]
    TabRenamed { id: TabId, name: String },
    #[serde(rename = "tab.moved")]
    TabMoved {
        id: TabId,
        pane: PaneId,
        index: usize,
    },
    #[serde(rename = "tab.closed")]
    TabClosed { id: TabId },

    #[serde(rename = "surface.split")]
    SurfaceSplit {
        target: SurfaceId,
        surface: SurfaceSpec,
        split: SplitSpec,
    },
    /// surface를 빼서 `target` 옆에 다시 붙인다. 다른 tab으로도 옮길 수 있다.
    #[serde(rename = "surface.moved")]
    SurfaceMoved {
        id: SurfaceId,
        target: SurfaceId,
        split: SplitSpec,
    },
    /// tab의 마지막 surface는 닫을 수 없다. 그 경우 tab을 닫는다.
    #[serde(rename = "surface.closed")]
    SurfaceClosed { id: SurfaceId },

    #[serde(rename = "metadata.set")]
    MetadataSet {
        target: MetadataTarget,
        key: String,
        value: String,
    },
    #[serde(rename = "metadata.removed")]
    MetadataRemoved { target: MetadataTarget, key: String },
}

impl DomainEvent {
    /// 이 빌드가 아는 모든 type tag. codec은 이 밖의 tag를 거절한다.
    pub const TAGS: &'static [&'static str] = &[
        "category.created",
        "category.renamed",
        "category.moved",
        "category.closed",
        "workspace.created",
        "workspace.renamed",
        "workspace.moved",
        "workspace.closed",
        "pane.split",
        "pane.moved",
        "pane.closed",
        "tab.created",
        "tab.renamed",
        "tab.moved",
        "tab.closed",
        "surface.split",
        "surface.moved",
        "surface.closed",
        "metadata.set",
        "metadata.removed",
    ];

    pub fn type_tag(&self) -> &'static str {
        match self {
            Self::CategoryCreated { .. } => "category.created",
            Self::CategoryRenamed { .. } => "category.renamed",
            Self::CategoryMoved { .. } => "category.moved",
            Self::CategoryClosed { .. } => "category.closed",
            Self::WorkspaceCreated { .. } => "workspace.created",
            Self::WorkspaceRenamed { .. } => "workspace.renamed",
            Self::WorkspaceMoved { .. } => "workspace.moved",
            Self::WorkspaceClosed { .. } => "workspace.closed",
            Self::PaneSplit { .. } => "pane.split",
            Self::PaneMoved { .. } => "pane.moved",
            Self::PaneClosed { .. } => "pane.closed",
            Self::TabCreated { .. } => "tab.created",
            Self::TabRenamed { .. } => "tab.renamed",
            Self::TabMoved { .. } => "tab.moved",
            Self::TabClosed { .. } => "tab.closed",
            Self::SurfaceSplit { .. } => "surface.split",
            Self::SurfaceMoved { .. } => "surface.moved",
            Self::SurfaceClosed { .. } => "surface.closed",
            Self::MetadataSet { .. } => "metadata.set",
            Self::MetadataRemoved { .. } => "metadata.removed",
        }
    }
}
