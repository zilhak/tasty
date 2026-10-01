//! Borrowed display queries. No runtime owner, transport sender, writer, waker or task control escapes.
use std::{collections::{HashMap,HashSet},ops::Deref};
use crate::{core::{CoreState,live::LiveDomainState},model::{Surface,Tab,StructurePresentation}};
use super::{engine_access::{EngineRef,ObservedPresentation},terminal_store::TerminalStore};

#[derive(Clone)]
pub(crate) struct EngineRead<'a> {
    pub(crate) core:&'a CoreState,
    pub(crate) layout_slot:Option<crate::core::layout_persistence::LayoutSlotId>,
    pub(crate) live:&'a LiveDomainState,
    pub(crate) settings:&'a crate::settings::Settings,
    pub(crate) terminals:TerminalRead<'a>,
    pub(crate) surface_registry:super::kind_catalog::KindCatalog,
    pub(crate) file_handler:HandlerCatalog<'a>,
    pub(crate) default_cols:usize,
    pub(crate) default_rows:usize,
    #[cfg(feature="gui")]
    pub(crate) explorer_favorites:&'a crate::core::explorer_favorites::ExplorerFavorites,
    #[cfg(feature="gui")]
    pub(crate) port_favorites:&'a crate::core::port_favorites::PortFavorites,
    #[cfg(feature="gui")]
    pub(crate) attach_mesh_frames:&'a crate::remote::mesh_frames::AttachMeshFrameStore,
    surfaces:&'a HashMap<u32,Box<dyn Surface>>,
    mirror_cwd:&'a HashMap<u32,crate::core::state::RemoteCwd>,
    mirror_busy:&'a HashSet<u32>,
    #[cfg(feature="gui")]
    readonly:&'a HashMap<u32,tasty_terminal::Terminal>,
    memory:&'a std::sync::Mutex<dyn tasty_memory::MemoryStorage>,
    tasks:&'a tasty_task_runtime::TaskScope,
}
#[derive(Clone,Copy)]
pub(crate) struct TerminalRead<'a>(&'a TerminalStore);
impl<'a> TerminalRead<'a> {
    pub(crate) fn get(&self,id:u32)->Option<&'a tasty_terminal::Terminal> {self.0.get(id)}
    pub(crate) fn contains(&self,id:u32)->bool {self.0.contains(id)}
    pub(crate) fn iter(&self)->impl Iterator<Item=(u32,&'a tasty_terminal::Terminal)> {self.0.iter()}
    pub(crate) fn generation(&self,id:u32)->Option<tasty_terminal::ResourceGeneration> {self.0.generation(id)}
    pub(crate) fn matches_generation(&self,id:u32,generation:tasty_terminal::ResourceGeneration)->bool {self.0.matches_generation(id,generation)}
    pub(crate) fn has_pty(&self,id:u32)->bool {self.0.pty(id).is_some()}
    pub(crate) fn foreground_process_name(&self,id:u32)->Option<String> {self.0.pty(id)?.foreground_process_info().map(|info|info.name)}
    pub(crate) fn process_id(&self,id:u32)->Option<u32> {self.0.pty(id)?.process_id()}
    pub(crate) fn cwd(&self,id:u32)->Option<std::path::PathBuf> {self.0.cwd(id)}
}
impl<'a> EngineRef<'a> {
    pub(crate) fn read(&self)->EngineRead<'a> {
        EngineRead {core:self.core,layout_slot:self.persistence.slot,live:self.live,settings:&self.runtime.settings,terminals:TerminalRead(&self.runtime.terminals),
            surface_registry:super::kind_catalog::KindCatalog::new(self.runtime.surface_registry.clone()),file_handler:HandlerCatalog(&self.runtime.file_handler),
            default_cols:self.runtime.default_cols,default_rows:self.runtime.default_rows,
            #[cfg(feature="gui")] explorer_favorites:&self.runtime.explorer_favorites,
            #[cfg(feature="gui")] port_favorites:&self.runtime.port_favorites,
            #[cfg(feature="gui")] attach_mesh_frames:&self.remote.attach_mesh_frames,
            surfaces:&self.runtime.surfaces,mirror_cwd:&self.remote.mirror_surface_cwd,mirror_busy:&self.remote.mirror_busy_surfaces,
            #[cfg(feature="gui")] readonly:&self.runtime.readonly_views,
            memory:self.runtime.memory.as_ref(),tasks:self.task_scope}
    }
}
impl Deref for EngineRead<'_> {type Target=CoreState;fn deref(&self)->&CoreState {self.core}}
impl<'a> EngineRead<'a> {
    #[cfg(feature="gui")]
    pub(crate) fn html_script(&self,sid:u32)->Option<crate::app::html_runtime::HtmlSnapshot> {
        let remote=self.surfaces.get(&sid)?.as_any().downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()?;
        if remote.kind_static!="html" {return None;}
        let target=crate::app::engine_action::SurfaceBinding::capture(self,sid)?;
        Some(crate::app::html_runtime::snapshot(remote,target))
    }
    pub(crate) fn as_ref(&self)->Self {self.clone()}
    pub(crate) fn find_surface_by_id(&self,id:u32)->Option<SurfaceRead<'a>> {self.surfaces.get(&id).map(|surface|SurfaceRead {inner:surface.as_ref()})}
    pub(crate) fn find_terminal_by_id(&self,id:u32)->Option<&'a tasty_terminal::Terminal> {self.terminals.get(id)}
    #[cfg(feature="gui")]
    pub(crate) fn visible_terminal(&self,id:u32)->Option<&'a tasty_terminal::Terminal> {if self.live.occupancy.is_hard_occupied(id) {self.readonly.get(&id)} else {self.terminals.get(id)}}
    pub(crate) fn tab_display_name(&self,tab:&Tab,surface:Option<u32>)->String {tab.display_name(surface.and_then(|id|self.live.surface_titles.get(&id)))}
    pub(crate) fn observed_presentation<'b>(&'b self,selection:&'b dyn StructurePresentation)->ObservedPresentation<'b> {ObservedPresentation::new(selection,&self.live.surface_titles)}
    pub(crate) fn surface_display_path(&self,id:u32,selection:&dyn StructurePresentation)->Option<tasty_core::SurfaceDisplayPath> {self.core.surface_display_path(id,&self.observed_presentation(selection))}
    pub(crate) fn surface_cwd(&self,id:u32)->Option<crate::core::state::SurfaceCwd> {
        use crate::core::state::{SurfaceCwd,RemoteCwd};
        if let Some(cwd)=self.mirror_cwd.get(&id) {return Some(SurfaceCwd::Remote(cwd.clone()));}
        let surface=self.find_surface_by_id(id)?;
        let cwd=if surface.kind()=="terminal" {self.terminals.cwd(id)} else {surface.source_cwd()}?;
        Some(if self.is_mirror_surface(id) {SurfaceCwd::Remote(RemoteCwd::new(cwd.to_string_lossy().into_owned()))} else {SurfaceCwd::Local(cwd)})
    }
    pub(crate) fn local_surface_cwd(&self,id:u32)->Option<std::path::PathBuf> {self.surface_cwd(id).and_then(crate::core::state::SurfaceCwd::into_local)}
    pub(crate) fn is_surface_busy(&self,id:u32)->bool {self.live.busy_surfaces.contains(&id)||self.mirror_busy.contains(&id)}
    pub(crate) fn busy_count(&self,ids:&[u32])->usize {ids.iter().filter(|id|self.is_surface_busy(**id)).count()}
    pub(crate) fn foreground_name(&self,id:u32)->Option<&str> {self.live.foreground_names.get(&id).map(String::as_str)}
    pub(crate) fn foreground_generation(&self,id:u32)->u64 {self.live.foreground_generation.get(&id).copied().unwrap_or(0)}
    pub(crate) fn is_surface_mouse_capture_disabled(&self,id:u32)->bool {self.live.mouse_capture_disabled_surfaces.contains(&id)}
    pub(crate) fn task_list(&self,workspace:u32)->Result<Vec<tasty_agent::Task>,tasty_agent::AgentError> {tasty_task_runtime::task::task_list_from_state(self.memory,self.tasks,workspace)}
    pub(crate) fn dag_list(&self,workspaces:&[u32])->Result<Vec<tasty_agent::DagSummary>,tasty_agent::AgentError> {tasty_task_runtime::task::dag_list_from_state(self.memory,self.tasks,workspaces)}
    pub(crate) fn runner_liveness(&self,workspace:u32)->(bool,bool) {self.tasks.runner_liveness(workspace)}
}

#[derive(Clone,Copy)]
pub(crate) struct HandlerCatalog<'a>(&'a crate::file::handler::FileHandlerRegistry);
impl HandlerCatalog<'_> {
    pub(crate) fn all_handlers(&self)->Vec<tasty_file_handler::FileHandler> {self.0.all_handlers()}
}

/// Borrowed display capability. No Any cast or runtime object reference escapes this wrapper.
#[derive(Clone,Copy)]
pub(crate) struct SurfaceRead<'a> {inner:&'a dyn Surface}
impl<'a> SurfaceRead<'a> {
    pub(crate) fn kind(&self)->&'static str {self.inner.kind()}
    pub(crate) fn type_name(&self)->&'static str {self.inner.type_name()}
    pub(crate) fn surface_id(&self)->Option<u32> {self.inner.surface_id()}
    pub(crate) fn display_name(&self)->String {self.inner.display_name()}
    pub(crate) fn source_cwd(&self)->Option<std::path::PathBuf> {self.inner.source_cwd()}
    pub(crate) fn webview_url(&self)->Option<String> {self.inner.webview_url()}
    pub(crate) fn to_tree_json(&self)->serde_json::Value {self.inner.to_tree_json()}
    // These builtin models contain ordinary values; their mutation requires &mut ownership.
    pub(crate) fn empty(&self)->Option<&'a crate::model::EmptySurface> {self.inner.as_any().downcast_ref()}
    pub(crate) fn explorer(&self)->Option<&'a crate::model::ExplorerPanel> {self.inner.as_any().downcast_ref()}
    pub(crate) fn dag(&self)->Option<&'a crate::model::DagGraphSurface> {self.inner.as_any().downcast_ref()}
    pub(crate) fn mesh(&self)->Option<&'a super::egui_mesh_surface::EguiMeshSurface> {self.inner.as_any().downcast_ref()}
    pub(crate) fn attach_mesh(&self)->Option<&'a crate::model::AttachMeshSurface> {self.inner.as_any().downcast_ref()}
    #[cfg(feature="gui")]
    pub(crate) fn remote_webview(&self)->Option<WebviewRead<'a>> {self.inner.as_any().downcast_ref().map(|inner|WebviewRead {inner})}
}
#[cfg(feature="gui")]
#[derive(Clone,Copy)]
pub(crate) struct WebviewRead<'a> {inner:&'a crate::plugin_bridge::remote_surface::RemoteSurface}
#[cfg(feature="gui")]
impl WebviewRead<'_> {
    pub(crate) fn kind(&self)->&'static str {self.inner.kind_static}
    pub(crate) fn url(&self)->Option<String> {self.inner.webview_url()}
    pub(crate) fn nav_state(&self)->crate::model::NavState {self.inner.nav_state()}
}
