//! Application inputs for kind materialization. Surface IDs are targets, never focus aliases.
use serde::{Deserialize,Serialize};
use crate::app::command::{ConvertSurfaceTarget,DomainIntent};

#[derive(Clone,Debug,Serialize,Deserialize)]
pub(super) enum Destination {
    Workspace { name:Option<String>,subtitle:Option<String>,description:Option<String>,category:Option<u32> },
    Tab { pane:u32,name:Option<String>,activate:bool },
    Pane { target:u32,direction:crate::model::SplitDirection },
    Surface { target:u32,direction:crate::model::SplitDirection },
    Convert { surface:u32,respawn:bool },
}

#[derive(Clone,Debug,Serialize,Deserialize)]
pub(super) struct Spec {
    pub destination:Destination,
    pub kind:String,
    pub cwd:Option<std::path::PathBuf>,
    pub params:serde_json::Value,
}

impl Spec {
    pub fn from_intent(intent:&DomainIntent)->Option<Self> {
        let (destination,kind,cwd,params)=match intent {
            DomainIntent::CreateWorkspace {cwd,kind,surface_params,name,subtitle,description,category}=>(Destination::Workspace {name:name.clone(),subtitle:subtitle.clone(),description:description.clone(),category:*category},kind.clone(),cwd.clone(),surface_params.clone()),
            DomainIntent::CreateTab {pane_id,cwd,kind,name,surface_params,activate}=>(Destination::Tab {pane:*pane_id,name:name.clone(),activate:*activate},kind.clone(),cwd.clone(),surface_params.clone()),
            DomainIntent::SplitPane {target_pane_id,direction,cwd,kind,surface_params}=>(Destination::Pane {target:*target_pane_id,direction:*direction},kind.clone(),cwd.clone(),surface_params.clone()),
            DomainIntent::SplitSurface {target_surface_id,direction,cwd,kind,surface_params}=>(Destination::Surface {target:*target_surface_id,direction:*direction},kind.clone(),cwd.clone(),surface_params.clone()),
            DomainIntent::ConvertSurface {surface_id,target}=> {
                let (kind,cwd,params)=match target {
                    ConvertSurfaceTarget::Terminal {cwd}=>("terminal".into(),cwd.clone(),serde_json::json!({})),
                    ConvertSurfaceTarget::Kind {cwd,kind,params}=>(kind.clone(),cwd.clone(),params.clone()),
                };
                (Destination::Convert {surface:*surface_id,respawn:false},kind,cwd,params)
            },
            DomainIntent::RespawnTerminal {surface_id,cwd}=>(Destination::Convert {surface:*surface_id,respawn:true},"terminal".into(),cwd.clone(),serde_json::json!({})),
            _=>return None,
        };
        Some(Self {destination,kind,cwd,params})
    }
}
