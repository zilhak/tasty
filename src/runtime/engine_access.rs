//! 실행 경로가 필요한 원본과 현재 journal binding을 빌린다. EngineSession의 소유권·View 관계는 노출하지 않는다.
//! 구조만 읽거나 바꾸는 함수는 계속 CoreState를 받는다. 이 대여는 effect 분리를 대신하지 않는다.
use super::engine_runtime::EngineRuntime;
use crate::core::CoreState;
use crate::hook_runtime::HookRuntimeState;
use crate::output_observer::ObserverRouter;
use std::ops::{Deref, DerefMut};
use tasty_task_runtime::TaskScope;

pub(crate) struct EngineMut<'a> {
    pub(crate) journal_binding: Option<&'a super::journal_product::EngineBinding>,
    pub(crate) core: &'a mut CoreState,
    pub(crate) persistence: &'a mut super::engine_session::EnginePersistence,
    pub(crate) remote: &'a mut crate::remote::state::RemoteState,
    pub(crate) live: &'a mut crate::core::live::LiveDomainState,
    pub(crate) runtime: &'a mut EngineRuntime,
    pub(crate) hooks: &'a mut HookRuntimeState,
    pub(crate) task_scope: &'a mut TaskScope,
    pub(crate) observer_router: &'a mut ObserverRouter,
}

#[derive(Clone, Copy)]
pub(crate) struct EngineRef<'a> {
    pub(crate) journal_binding: Option<&'a super::journal_product::EngineBinding>,
    pub(crate) core: &'a CoreState,
    pub(crate) persistence: &'a super::engine_session::EnginePersistence,
    pub(crate) remote: &'a crate::remote::state::RemoteState,
    pub(crate) live: &'a crate::core::live::LiveDomainState,
    pub(crate) runtime: &'a EngineRuntime,
    pub(crate) hooks: &'a HookRuntimeState,
    pub(crate) task_scope: &'a TaskScope,
    pub(crate) observer_router: &'a ObserverRouter,
}

impl EngineMut<'_> {
    pub(crate) fn read(&self) -> super::engine_read::EngineRead<'_> {
        self.as_ref().read()
    }

    pub(crate) fn as_ref(&self) -> EngineRef<'_> {
        EngineRef {
            journal_binding: self.journal_binding,
            core: self.core,
            persistence: self.persistence,
            remote: self.remote,
            live: self.live,
            runtime: self.runtime,
            hooks: self.hooks,
            task_scope: self.task_scope,
            observer_router: self.observer_router,
        }
    }
}
impl Deref for EngineMut<'_> {
    type Target = CoreState;
    fn deref(&self) -> &CoreState {
        self.core
    }
}
impl DerefMut for EngineMut<'_> {
    fn deref_mut(&mut self) -> &mut CoreState {
        self.core
    }
}
impl Deref for EngineRef<'_> {
    type Target = CoreState;
    fn deref(&self) -> &CoreState {
        self.core
    }
}

impl<'a> EngineRef<'a> {
    pub(crate) fn find_surface_by_id(&self, id: u32) -> Option<&'a dyn crate::model::Surface> {
        self.runtime
            .surfaces
            .get(&id)
            .map(|surface| surface.as_ref())
    }
}
impl EngineMut<'_> {
    pub(crate) fn find_surface_by_id(&self, id: u32) -> Option<&dyn crate::model::Surface> {
        self.runtime
            .surfaces
            .get(&id)
            .map(|surface| surface.as_ref())
    }
    pub(crate) fn find_surface_by_id_mut(
        &mut self,
        id: u32,
    ) -> Option<&mut (dyn crate::model::Surface + 'static)> {
        self.runtime
            .surfaces
            .get_mut(&id)
            .map(|surface| surface.as_mut())
    }
}

impl EngineRef<'_> {
    /// leaf를 조회해 attach 후보를 분류한다. Deferred::Terminal만 터미널로 취급하며
    /// Deferred::Plugin은 PTY 생성 대상이 아닌 플러그인 placeholder다.
    pub(crate) fn classify_attach_surfaces(
        &self,
        workspace_id: u32,
    ) -> crate::model::AttachSurfaceClass {
        let mut class = crate::model::AttachSurfaceClass::default();
        let Some(workspace) = self
            .core
            .workspaces()
            .into_iter()
            .find(|workspace| workspace.id == workspace_id)
        else {
            return class;
        };
        for id in workspace.all_surface_ids() {
            let Some(s) = self.find_surface_by_id(id) else {
                continue;
            };
            if let Some((kind, plugin_id)) = s.attach_mesh_info() {
                class
                    .mesh_candidates
                    .push((id, kind.to_string(), plugin_id.to_string()));
                continue;
            }
            // 두 신호를 모두 내면 기존 mesh 경로를 우선한다.
            if let Some((kind, plugin_id, file)) = s.attach_content_info() {
                class
                    .content_candidates
                    .push((id, kind.to_string(), plugin_id.to_string(), file));
                continue;
            }
            if let Some(explorer) = s.as_any().downcast_ref::<crate::model::ExplorerPanel>() {
                class
                    .explorers
                    .push((id, explorer.current_root().to_path_buf()));
                continue;
            }
            let is_terminal = s
                .as_any()
                .downcast_ref::<crate::runtime::surface_restorer::JournalPlaceholder>()
                .is_some_and(|placeholder| placeholder.kind == "terminal")
                || s.kind() == "terminal"
                || s.as_any()
                    .downcast_ref::<crate::model::EmptySurface>()
                    .map(|e| e.deferred_spawn().is_some())
                    .unwrap_or(false);
            if is_terminal {
                class.terminals.push(id);
            } else {
                class.non_terminals.push(id);
            }
        }
        class
    }
}
impl EngineMut<'_> {
    pub(crate) fn classify_attach_surfaces(
        &self,
        workspace_id: u32,
    ) -> crate::model::AttachSurfaceClass {
        self.as_ref().classify_attach_surfaces(workspace_id)
    }
}

impl EngineMut<'_> {
    pub(crate) fn force_detach(&mut self, id: u32) -> Option<crate::core::attach::AttachClientId> {
        let holder = self.live.occupancy.force_detach(id)?;
        self.remote.notify_detached(holder, "force_detach");
        Some(holder)
    }
    pub(crate) fn force_detach_workspace(
        &mut self,
        id: u32,
    ) -> Option<crate::core::attach::AttachClientId> {
        let holder = self.live.occupancy.force_detach_workspace(id)?;
        self.remote
            .notify_detached(holder, "force_detach_workspace");
        Some(holder)
    }
    pub(crate) fn forget_closed_surface(&mut self, id: u32) -> bool {
        let workspace = self.live.occupancy.workspace_of_surface(id);
        let changed = self.live.occupancy.forget_closed_surface(id);
        if let Some(workspace) = workspace
            && self.live.occupancy.workspace_holder(workspace).is_some()
        {
            self.remote.mark_structure_changed(workspace);
        }
        changed
    }
}

impl EngineRef<'_> {
    /// 설정과 factory가 있으면 surface별 waker, 아니면 공용 waker를 반환한다.
    pub fn make_waker(&self, surface_id: u32) -> tasty_terminal::Waker {
        if self.runtime.settings.performance.targeted_pty_polling
            && let Some(factory) = &self.runtime.waker_factory
        {
            return factory.make_targeted_waker(surface_id);
        }
        self.runtime.waker.clone()
    }
}
impl EngineMut<'_> {
    pub(crate) fn make_waker(&self, id: u32) -> tasty_terminal::Waker {
        self.as_ref().make_waker(id)
    }
}

/// Borrowed presentation joins immutable View choices and live observations for one query.
pub(crate) struct ObservedPresentation<'a> {
    selection: &'a dyn crate::model::StructurePresentation,
    titles: &'a std::collections::HashMap<u32, tasty_model::SurfaceTitle>,
}
impl<'a> ObservedPresentation<'a> {
    pub(crate) fn new(
        selection: &'a dyn crate::model::StructurePresentation,
        titles: &'a std::collections::HashMap<u32, tasty_model::SurfaceTitle>,
    ) -> Self {
        Self { selection, titles }
    }
}
impl crate::model::StructurePresentation for ObservedPresentation<'_> {
    fn surface_title(&self, id: u32) -> Option<&tasty_model::SurfaceTitle> {
        self.titles.get(&id)
    }
    fn pane_id(&self, workspace: &crate::model::Workspace) -> Option<u32> {
        self.selection.pane_id(workspace)
    }
    fn tab_index(&self, pane: &crate::model::Pane) -> usize {
        self.selection.tab_index(pane)
    }
    fn surface_id(&self, tab: &crate::model::Tab) -> Option<u32> {
        self.selection.surface_id(tab)
    }
    fn category_collapsed(&self, id: u32) -> bool {
        self.selection.category_collapsed(id)
    }
    fn split_focus_second(&self, id: crate::model::SplitNodeId) -> bool {
        self.selection.split_focus_second(id)
    }
}
impl EngineRef<'_> {
    pub(crate) fn tab_display_name(&self, tab: &crate::model::Tab, surface: Option<u32>) -> String {
        tab.display_name(surface.and_then(|id| self.live.surface_titles.get(&id)))
    }
    pub(crate) fn observed_presentation<'a>(
        &'a self,
        selection: &'a dyn crate::model::StructurePresentation,
    ) -> ObservedPresentation<'a> {
        ObservedPresentation {
            selection,
            titles: &self.live.surface_titles,
        }
    }
}
impl EngineMut<'_> {
    pub(crate) fn tab_display_name(&self, tab: &crate::model::Tab, surface: Option<u32>) -> String {
        self.as_ref().tab_display_name(tab, surface)
    }
}

impl EngineRef<'_> {
    pub(crate) fn surface_display_path(
        &self,
        surface: u32,
        selection: &dyn crate::model::StructurePresentation,
    ) -> Option<tasty_core::SurfaceDisplayPath> {
        self.core
            .surface_display_path(surface, &self.observed_presentation(selection))
    }
}
