//! Resolve View input once into explicit application targets. No structure or resource is changed.
use crate::runtime::engine_access::EngineRef;
use crate::app::command::{ConvertSurfaceTarget,DomainIntent};
use crate::intent::{ConvertTarget,Intent,IntentOrigin};
use crate::state::RequestContext;

pub(crate) fn resolve(state:&RequestContext,engine:&EngineRef<'_>,intent:&Intent,origin:&IntentOrigin)->anyhow::Result<Option<DomainIntent>> {
    let params_or_empty=|params:&serde_json::Value|if params.is_null(){serde_json::json!({})}else{params.clone()};
    Ok(Some(match intent {
        Intent::NewWorkspace {kind,params,category}=> {
            let kind=kind.as_deref().unwrap_or("terminal");
            DomainIntent::CreateWorkspace {
                cwd:if kind=="terminal" && params.is_null(){state.resolve_inherit_cwd(engine)}else{None},
                kind:kind.into(),surface_params:params_or_empty(params),name:None,subtitle:None,description:None,category:*category,
            }
        },
        Intent::NewTab {kind,params}=> {
            let kind=kind.as_deref().unwrap_or("terminal");
            DomainIntent::CreateTab {pane_id:state.focused_pane_id(engine.core),cwd:if kind=="terminal" {state.resolve_inherit_cwd(engine)}else{None},kind:kind.into(),name:None,surface_params:params_or_empty(params),activate:origin.is_user()}
        },
        Intent::SplitPane {direction}=>DomainIntent::SplitPane {target_pane_id:state.focused_pane_id(engine.core),direction:*direction,cwd:state.resolve_inherit_cwd(engine),kind:"terminal".into(),surface_params:serde_json::json!({})},
        Intent::SplitSurface {direction}=> {
            let target=state.focused_surface_id(engine.core).ok_or_else(||anyhow::anyhow!("SplitSurface: no focused surface"))?;
            DomainIntent::SplitSurface {target_surface_id:target,direction:*direction,cwd:state.resolve_inherit_cwd_from_surface(engine,target),kind:"terminal".into(),surface_params:serde_json::json!({})}
        },
        Intent::ConvertSurface {surface_id,target}=> {
            let target=match target {
                ConvertTarget::Terminal=>ConvertSurfaceTarget::Terminal {cwd:state.resolve_inherit_cwd(engine)},
                ConvertTarget::Kind {kind,cwd,params}=> {
                    let mut params=params.clone();
                    if let Some(definition)=engine.core.surface_registry.get(kind) {definition.normalize_param_aliases(&mut params);}
                    ConvertSurfaceTarget::Kind {cwd:cwd.clone().or_else(||state.resolve_inherit_cwd_from_surface(engine,*surface_id)),kind:kind.clone(),params}
                },
            };
            DomainIntent::ConvertSurface {surface_id:*surface_id,target}
        },
        _=>return Ok(None),
    }))
}
