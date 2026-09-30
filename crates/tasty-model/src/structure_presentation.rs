//! Read-only selection used when projecting a structure into legacy wire data.
//! The structure owns none of these choices. The caller supplies its View,
//! restore or command-default context explicitly.

use crate::{Pane, SurfaceId, Tab, Workspace};

pub trait StructurePresentation {
    fn pane_id(&self, workspace: &Workspace) -> Option<u32>;
    fn tab_index(&self, pane: &Pane) -> usize;
    fn surface_id(&self, tab: &Tab) -> Option<SurfaceId>;
}
