//! Composite reads borrow disjoint local and mirror projections. Neither tree is cloned.

use super::CoreState;
use crate::model::Workspace;

impl CoreState {
    /// Existing list indices refer to the combined display order. Journal input uses local only.
    pub(crate) fn workspaces(&self) -> WorkspaceRead<'_> {
        if self.mirror_workspaces.is_empty() {
            return WorkspaceRead::local(&self.local_workspaces);
        }
        WorkspaceRead {
            local: &self.local_workspaces,
            mirrors: &self.mirror_workspaces,
            order: &self.workspace_display_order,
        }
    }

    pub(crate) fn workspace_at(&self, index: usize) -> Option<&Workspace> {
        self.workspaces().get(index)
    }

    pub(crate) fn workspace_at_mut(&mut self, index: usize) -> Option<&mut Workspace> {
        let id = self.workspaces().get(index)?.id;
        self.local_workspaces
            .iter_mut()
            .chain(&mut self.mirror_workspaces)
            .find(|workspace| workspace.id == id)
    }

    pub(crate) fn workspaces_mut(&mut self) -> Vec<&mut Workspace> {
        let order = &self.workspace_display_order;
        let mut workspaces: Vec<_> = self
            .local_workspaces
            .iter_mut()
            .chain(&mut self.mirror_workspaces)
            .collect();
        workspaces.sort_by_key(|workspace| {
            order
                .iter()
                .position(|id| *id == workspace.id)
                .unwrap_or(usize::MAX)
        });
        workspaces
    }

    pub(crate) fn push_local_workspace(&mut self, workspace: Workspace) {
        self.insert_local_workspace(self.workspaces().len(), workspace);
    }

    #[cfg(any(feature = "gui", test))]
    pub(crate) fn push_mirror_workspace(&mut self, workspace: Workspace) {
        assert!(workspace.mirror, "remote projection requires a mirror");
        self.refresh_workspace_display_order();
        self.workspace_display_order.push(workspace.id);
        self.mirror_workspaces.push(workspace);
    }

    pub(crate) fn insert_local_workspace(&mut self, index: usize, workspace: Workspace) {
        assert!(!workspace.mirror, "local projection cannot own a mirror");
        self.refresh_workspace_display_order();
        let local_index = self.workspace_display_order[..index]
            .iter()
            .filter(|id| {
                self.local_workspaces
                    .iter()
                    .any(|workspace| workspace.id == **id)
            })
            .count();
        self.workspace_display_order.insert(index, workspace.id);
        self.local_workspaces.insert(local_index, workspace);
    }

    pub(crate) fn remove_workspace_at(&mut self, index: usize) -> Workspace {
        let id = self
            .workspace_at(index)
            .expect("workspace index is valid")
            .id;
        self.workspace_display_order
            .retain(|candidate| *candidate != id);
        if let Some(index) = self
            .local_workspaces
            .iter()
            .position(|workspace| workspace.id == id)
        {
            self.local_workspaces.remove(index)
        } else {
            let index = self
                .mirror_workspaces
                .iter()
                .position(|workspace| workspace.id == id)
                .expect("composite workspace exists");
            self.mirror_workspaces.remove(index)
        }
    }

    pub(crate) fn move_workspace_in_display(&mut self, from: usize, to: usize) -> bool {
        let len = self.workspaces().len();
        if from == to || from >= len || to >= len {
            return false;
        }
        self.refresh_workspace_display_order();
        let id = self.workspace_display_order.remove(from);
        self.workspace_display_order.insert(to, id);
        let order: Vec<_> = self
            .workspace_display_order
            .iter()
            .copied()
            .filter(|id| {
                self.local_workspaces
                    .iter()
                    .any(|workspace| workspace.id == *id)
            })
            .collect();
        self.reorder_local_workspaces(&order)
            .expect("display contains each local workspace once");
        true
    }

    #[cfg(any(feature = "gui", test))]
    pub(crate) fn replace_mirror_workspace(
        &mut self,
        workspace: Workspace,
    ) -> Result<(), Workspace> {
        assert!(workspace.mirror, "remote projection requires a mirror");
        if let Some(slot) = self
            .mirror_workspaces
            .iter_mut()
            .find(|candidate| candidate.id == workspace.id)
        {
            *slot = workspace;
            Ok(())
        } else {
            Err(workspace)
        }
    }

    pub(crate) fn reorder_local_workspaces(&mut self, order: &[u32]) -> Result<(), String> {
        if order.len() != self.local_workspaces.len()
            || order.iter().enumerate().any(|(index, id)| {
                order[..index].contains(id)
                    || !self
                        .local_workspaces
                        .iter()
                        .any(|workspace| workspace.id == *id)
            })
        {
            return Err("committed workspace order differs from the live local projection".into());
        }
        self.local_workspaces
            .sort_by_key(|workspace| order.iter().position(|id| *id == workspace.id));
        self.reconcile_local_display_order();
        Ok(())
    }

    pub(crate) fn replace_local_workspaces(&mut self, workspaces: Vec<Workspace>) {
        assert!(workspaces.iter().all(|workspace| !workspace.mirror));
        self.local_workspaces = workspaces;
        self.reconcile_local_display_order();
    }

    fn reconcile_local_display_order(&mut self) {
        let mut locals = self.local_workspaces.iter().map(|workspace| workspace.id);
        let mut order: Vec<_> = self
            .workspace_display_order
            .iter()
            .filter_map(|id| {
                if self
                    .mirror_workspaces
                    .iter()
                    .any(|workspace| workspace.id == *id)
                {
                    Some(*id)
                } else {
                    locals.next()
                }
            })
            .collect();
        order.extend(locals);
        self.workspace_display_order = order;
        self.refresh_workspace_display_order();
    }

    fn refresh_workspace_display_order(&mut self) {
        self.workspace_display_order = self
            .workspaces()
            .into_iter()
            .map(|workspace| workspace.id)
            .collect();
    }
}

/// Borrowed composite sequence. Iteration does not allocate, sort, or clone workspace objects.
#[derive(Clone, Copy)]
pub(crate) struct WorkspaceRead<'a> {
    local: &'a [Workspace],
    mirrors: &'a [Workspace],
    order: &'a [u32],
}

impl<'a> WorkspaceRead<'a> {
    pub(crate) fn local(local: &'a [Workspace]) -> Self {
        Self {
            local,
            mirrors: &[],
            order: &[],
        }
    }
    pub(crate) fn len(self) -> usize {
        self.local.len() + self.mirrors.len()
    }
    pub(crate) fn is_empty(self) -> bool {
        self.len() == 0
    }
    pub(crate) fn get(self, index: usize) -> Option<&'a Workspace> {
        self.iter().nth(index)
    }
    pub(crate) fn iter(self) -> WorkspaceIter<'a> {
        WorkspaceIter {
            read: self,
            ordered: 0,
            fallback: 0,
            yielded: 0,
        }
    }
}

pub(crate) struct WorkspaceIter<'a> {
    read: WorkspaceRead<'a>,
    ordered: usize,
    fallback: usize,
    yielded: usize,
}

impl<'a> Iterator for WorkspaceIter<'a> {
    type Item = &'a Workspace;
    fn next(&mut self) -> Option<Self::Item> {
        while let Some(id) = self.read.order.get(self.ordered) {
            self.ordered += 1;
            if let Some(workspace) = self
                .read
                .local
                .iter()
                .chain(self.read.mirrors)
                .find(|workspace| workspace.id == *id)
            {
                self.yielded += 1;
                return Some(workspace);
            }
        }
        while self.fallback < self.read.len() {
            let workspace = if self.fallback < self.read.local.len() {
                &self.read.local[self.fallback]
            } else {
                &self.read.mirrors[self.fallback - self.read.local.len()]
            };
            self.fallback += 1;
            if !self.read.order.contains(&workspace.id) {
                self.yielded += 1;
                return Some(workspace);
            }
        }
        None
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.read.len() - self.yielded;
        (remaining, Some(remaining))
    }
}
impl ExactSizeIterator for WorkspaceIter<'_> {}

impl<'a> IntoIterator for WorkspaceRead<'a> {
    type Item = &'a Workspace;
    type IntoIter = WorkspaceIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
impl<'a> IntoIterator for &WorkspaceRead<'a> {
    type Item = &'a Workspace;
    type IntoIter = WorkspaceIter<'a>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

#[cfg(test)]
impl CoreState {
    pub(crate) fn set_workspace_fixture(&mut self, workspaces: Vec<Workspace>) {
        self.workspace_display_order = workspaces.iter().map(|workspace| workspace.id).collect();
        (self.mirror_workspaces, self.local_workspaces) = workspaces
            .into_iter()
            .partition(|workspace| workspace.mirror);
    }
    pub(crate) fn make_local_fixture(&mut self, index: usize) {
        let mut workspace = self.remove_workspace_at(index);
        workspace.mirror = false;
        self.refresh_workspace_display_order();
        let local_index = self
            .workspaces()
            .iter()
            .take(index)
            .filter(|w| !w.mirror)
            .count();
        self.workspace_display_order.insert(index, workspace.id);
        self.local_workspaces.insert(local_index, workspace);
    }
    pub(crate) fn make_mirror_fixture(&mut self, index: usize) {
        let mut workspace = self.remove_workspace_at(index);
        workspace.mirror = true;
        self.refresh_workspace_display_order();
        self.workspace_display_order.insert(index, workspace.id);
        self.mirror_workspaces.push(workspace);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn workspace(id: u32, mirror: bool) -> Workspace {
        let mut workspace = Workspace::new_with_terminal_marker(
            id,
            format!("ws-{id}"),
            id * 10,
            id * 100,
            id * 1000,
        );
        workspace.mirror = mirror;
        workspace
    }
    #[test]
    fn committed_local_order_and_mirror_display_keep_existing_objects() {
        let (_view, mut session) = crate::state::tests::test_state();
        let engine = &mut session.core_state;
        engine.set_workspace_fixture(vec![
            workspace(1, false),
            workspace(9, true),
            workspace(2, false),
        ]);
        let surface = engine.find_surface_by_id(1000).unwrap() as *const dyn crate::model::Surface
            as *const ();
        engine.reorder_local_workspaces(&[2, 1]).unwrap();
        assert_eq!(
            engine
                .workspaces()
                .iter()
                .map(|workspace| workspace.id)
                .collect::<Vec<_>>(),
            [2, 9, 1]
        );
        assert_eq!(
            engine
                .local_workspaces
                .iter()
                .map(|workspace| workspace.id)
                .collect::<Vec<_>>(),
            [2, 1]
        );
        assert_eq!(engine.mirror_workspaces[0].id, 9);
        assert_eq!(
            engine.find_surface_by_id(1000).unwrap() as *const dyn crate::model::Surface
                as *const (),
            surface
        );
        assert!(engine.reorder_local_workspaces(&[9, 1]).is_err());
        assert_eq!(
            engine
                .workspaces()
                .iter()
                .map(|workspace| workspace.id)
                .collect::<Vec<_>>(),
            [2, 9, 1]
        );
    }
    #[test]
    fn dynamic_mirror_insert_rebuild_remove_preserves_local_selection() {
        let (mut view, mut session) = crate::state::tests::test_state();
        let engine = &mut session.core_state;
        engine.set_workspace_fixture(vec![workspace(1, false), workspace(2, false)]);
        view.reconcile_presentation(engine);
        view.navigation.select_workspace(&engine.workspaces(), 2);
        engine.push_mirror_workspace(workspace(9, true));
        assert!(engine.move_workspace_in_display(2, 0));
        engine
            .replace_mirror_workspace(workspace(9, true))
            .unwrap_or_else(|_| panic!("mirror disappeared"));
        view.reconcile_presentation(engine);
        assert_eq!(view.navigation.workspace_id(&engine.workspaces()), Some(2));
        assert_eq!(engine.workspace_at(0).unwrap().id, 9);
        engine.remove_workspace_at(0);
        assert!(engine.mirror_workspaces.is_empty());
        assert_eq!(
            engine
                .workspaces()
                .iter()
                .map(|workspace| workspace.id)
                .collect::<Vec<_>>(),
            [1, 2]
        );
        view.reconcile_presentation(engine);
        assert_eq!(view.navigation.workspace_id(&engine.workspaces()), Some(2));
    }
}
