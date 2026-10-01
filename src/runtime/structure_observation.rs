//! Execution obligations derived from a committed local batch, never polled from a View baseline.
use super::engine_access::EngineMut;
impl EngineMut<'_> {
    /// Project transport obligations only after the entire structural batch has been installed.
    /// Sending remains in the ordinary observation pump, behind its publication/result barrier.
    pub(crate) fn observe_committed_structure(
        &mut self,
        before: &tasty_core::JournalModel,
        events: &[tasty_core::RecordedEvent],
    ) {
        use tasty_core::DomainEvent as E;
        let structural = events.iter().any(|record| {
            matches!(
                record.event,
                E::StructureReplaced { .. }
                    | E::WorkspaceCreated { .. }
                    | E::WorkspaceRenamed { .. }
                    | E::WorkspaceDetailsSet { .. }
                    | E::WorkspaceMoved { .. }
                    | E::WorkspaceClosed { .. }
                    | E::PaneSplit { .. }
                    | E::PaneMoved { .. }
                    | E::PaneClosed { .. }
                    | E::TabCreated { .. }
                    | E::TabRenamed { .. }
                    | E::TabExplicitNameSet { .. }
                    | E::TabMoved { .. }
                    | E::TabClosed { .. }
                    | E::SurfaceSplit { .. }
                    | E::SurfaceMoved { .. }
                    | E::SurfaceClosed { .. }
                    | E::SurfaceConverted { .. }
                    | E::SurfaceActivationChanged { .. }
                    | E::PaneRatioSet { .. }
                    | E::SurfaceRatioSet { .. }
            )
        });
        if !structural {
            return;
        }
        for workspace in &before.workspace_order {
            if !self
                .core
                .local_workspaces()
                .iter()
                .any(|current| current.id == *workspace)
            {
                // The registry retains stop ownership even after this workspace leaves the tree.
                let _ = self.task_scope.request_stop_workspace(*workspace);
            }
        }
        for (tab, old) in &before.tabs {
            if let Some(pane) = self.core.find_pane_for_tab(*tab)
                && pane != old.pane
            {
                self.runtime.pending_host_events.push(
                    crate::core::host_event::PendingHostEvent::TabMoved {
                        tab_id: *tab,
                        from_pane: old.pane,
                        to_pane: pane,
                    },
                );
            }
        }
        let workspaces: std::collections::BTreeSet<_> = before
            .workspace_order
            .iter()
            .copied()
            .chain(
                self.core
                    .local_workspaces()
                    .iter()
                    .map(|workspace| workspace.id),
            )
            .collect();
        for workspace in workspaces {
            if self.live.occupancy.workspace_holder(workspace).is_some() {
                self.remote.mark_structure_changed(workspace);
            }
        }
        for record in events {
            let E::SurfaceActivationChanged { id, activation, .. } = &record.event else {
                continue;
            };
            if activation.phase != tasty_core::ActivationPhase::Ready {
                continue;
            }
            let Some(workspace) = self
                .find_workspace_index_for_surface(*id)
                .and_then(|(index, _)| self.workspace_at(index))
                .map(|workspace| workspace.id)
            else {
                continue;
            };
            if self.live.occupancy.workspace_holder(workspace).is_none() {
                continue;
            }
            let generation = self.runtime.terminals.generation(*id);
            self.live
                .occupancy
                .add_workspace_member(workspace, *id, generation.is_some());
            if let Some(generation) = generation {
                self.remote
                    .pending_workspace_taps
                    .insert(*id, (workspace, generation));
            }
        }
    }
}

impl EngineMut<'_> {
    /// Remote replacement preserves IDs when the server keeps a tab. Initial snapshot insertion
    /// has no predecessor and therefore emits no historical movement event.
    #[cfg(feature = "gui")]
    pub(crate) fn replace_mirror_workspace(
        &mut self,
        workspace: crate::model::Workspace,
    ) -> Result<(), crate::model::Workspace> {
        let old = self
            .core
            .mirror_workspaces()
            .iter()
            .find(|old| old.id == workspace.id)
            .map(|old| {
                old.pane_layout()
                    .all_pane_ids()
                    .into_iter()
                    .filter_map(|pane| old.pane_layout().find_pane(pane))
                    .flat_map(|pane| pane.tabs.iter().map(move |tab| (tab.id, pane.id)))
                    .collect::<std::collections::HashMap<_, _>>()
            })
            .unwrap_or_default();
        let moved: Vec<_> = workspace
            .pane_layout()
            .all_pane_ids()
            .into_iter()
            .filter_map(|pane| workspace.pane_layout().find_pane(pane))
            .flat_map(|pane| {
                pane.tabs.iter().filter_map(|tab| {
                    old.get(&tab.id)
                        .filter(|from| **from != pane.id)
                        .map(|from| crate::core::host_event::PendingHostEvent::TabMoved {
                            tab_id: tab.id,
                            from_pane: *from,
                            to_pane: pane.id,
                        })
                })
            })
            .collect();
        self.core.replace_mirror_workspace(workspace)?;
        self.runtime.pending_host_events.extend(moved);
        Ok(())
    }
}
