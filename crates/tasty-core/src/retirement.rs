//! Exact logical owners removed by a committed close. Cleanup cannot resolve a current focus.
use crate::{DataRef, EntityId};
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseTarget {
    Workspace(u32),
    Pane(u32),
    Tab(u32),
    Surface(u32),
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetiredSurface {
    pub id: u32,
    pub kind: String,
    pub activation_generation: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetirementPlan {
    #[serde(default)]
    pub replacement: Option<crate::Replacement>,
    pub target: CloseTarget,
    pub removed: Vec<EntityId>,
    pub surfaces: Vec<RetiredSurface>,
    pub tab_parents: Vec<(u32, u32)>,
    pub is_user_close: bool,
    /// Immutable user undo capture. Agent closes do not add an undo entry.
    pub undo: Option<crate::UndoCapture>,
}
