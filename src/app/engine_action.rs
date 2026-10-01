//! Non-journal execution requests carry targets observed by a View, never a mutable Engine borrow.
use crate::runtime::engine_access::{EngineRef,EngineMut};
#[derive(Clone,Debug)]
pub(crate) struct SurfaceBinding {
    surface:u32,
    activation:Option<u64>,
    resource:Option<tasty_terminal::ResourceGeneration>,
    mirror:Option<(u32,std::sync::Weak<()>)>,
}
impl SurfaceBinding {
    pub(crate) fn capture(engine:&EngineRef<'_>,surface:u32)->Option<Self> {
        let descriptor=engine.core.find_surface_by_id(surface)?;
        let mirror=engine.find_workspace_index_for_surface(surface).and_then(|(index,_)|engine.workspace_at(index)).filter(|workspace|workspace.mirror).and_then(|workspace|engine.mirror_projection_token(workspace.id).map(|token|(workspace.id,token)));
        Some(Self {surface,activation:descriptor.activation_generation,resource:engine.runtime.terminals.generation(surface),mirror})
    }
    fn current(&self,engine:&EngineRef<'_>)->bool {
        engine.core.find_surface_by_id(self.surface).is_some_and(|descriptor|descriptor.activation_generation==self.activation)
            && self.resource.is_none_or(|generation|engine.runtime.terminals.matches_generation(self.surface,generation))
            && self.mirror.as_ref().is_none_or(|(workspace,token)|engine.matches_mirror_projection(*workspace,token))
    }
}
#[derive(Clone,Debug)]
pub(crate) enum EngineAction {
    RecordTyping {target:SurfaceBinding,at:std::time::Instant},
    ExplorerCwd {target:SurfaceBinding,folder:std::path::PathBuf},
    RemoveExplorerFavorite {path:std::path::PathBuf},
    RemoteMeshFull {targets:Vec<SurfaceBinding>},
    DefaultGrid {cols:usize,rows:usize},
    Resize {targets:Vec<(SurfaceBinding,usize,usize)>},
    #[cfg(feature="gui")]
    LocalMesh {target:SurfaceBinding,plugin:String,registration:std::sync::Weak<crate::runtime::surface_registry::SurfaceKindDef>,bootstrap:Option<(String,Option<String>,String)>,params:tasty_plugin_protocol::protocol::SurfaceSetContextParams},
    #[cfg(feature="gui")]
    RemoteMesh {target:SurfaceBinding,context:Option<crate::core::state::AttachMeshContextForward>,input:Option<tasty_plugin_protocol::protocol::RawInputWire>},
}
impl EngineAction {
    pub(crate) fn apply(&self,engine:&mut EngineMut<'_>,plugins:Option<&crate::plugin::PluginManager>) {
        match self {
            Self::ExplorerCwd {target,folder}=> {
                if !target.current(&engine.as_ref()) {return;}
                if let Some(explorer)=engine.runtime.surfaces.get_mut(&target.surface).and_then(|surface|surface.as_any_mut().downcast_mut::<crate::model::ExplorerPanel>()) {
                    explorer.active_tab_mut().set_cwd(folder.clone());
                    engine.mark_layout_dirty();
                }
            },
            Self::RemoveExplorerFavorite {path}=> {engine.runtime.explorer_favorites.remove(path);engine.runtime.explorer_favorites.save();},
            Self::RemoteMeshFull {targets}=> {
                for target in targets {if target.current(&engine.as_ref()) {engine.remote.pending_mesh_full_resend_forward.insert(target.surface);}}
            },
            #[cfg(feature="gui")]
            Self::LocalMesh {target,plugin,registration,bootstrap,params}=> {
                if !target.current(&engine.as_ref()) {return;}
                let Some(kind)=engine.core.find_surface_by_id(target.surface).map(|surface|surface.kind.as_str()) else {return;};
                let Some(current)=engine.runtime.surface_registry.get_live(kind) else {return;};
                if !registration.ptr_eq(&std::sync::Arc::downgrade(&current)) {return;}
                let Some(manager)=plugins else {return;};
                if let Some((kind,file,name))=bootstrap {manager.send_egui_mesh_surface_create(plugin,target.surface,kind,file.as_deref(),name);}
                manager.send_surface_set_context(plugin,params);
            },
            #[cfg(feature="gui")]
            Self::RemoteMesh {target,context,input}=> {
                if !target.current(&engine.as_ref()) {return;}
                if let Some(context)=context {engine.remote.pending_mesh_context_forward.insert(target.surface,context.clone());}
                if let Some(input)=input {engine.remote.pending_mesh_input_forward.entry(target.surface).and_modify(|pending| {pending.events.extend(input.events.clone());pending.focused=input.focused;pending.modifiers=input.modifiers;pending.time=input.time;}).or_insert_with(||input.clone());}
            },
            Self::RecordTyping {target,at} if target.current(&engine.as_ref())=> {engine.live.last_key_input.insert(target.surface,*at);},
            Self::RecordTyping {..}=>{},
            Self::DefaultGrid {cols,rows}=>{engine.runtime.default_cols=*cols;engine.runtime.default_rows=*rows;},
            Self::Resize {targets}=> {
                let targets=targets.iter().filter(|(target,_,_)|target.current(&engine.as_ref())).map(|(target,cols,rows)|(target.surface,*cols,*rows)).collect();
                #[cfg(feature="gui")]
                crate::app::services::AppServices::resize_terminals(engine,targets);
            },
        }
    }
}
