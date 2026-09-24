//! 매니페스트의 detector·handler JSON을 파일 도메인 타입으로 읽고 검증한다.
//! manifest 크레이트가 본체의 파일 도메인에 의존하지 않도록 검증을 여기서 보완한다.

use anyhow::Result;

/// Manifest::load 뒤에 detector·handler와 surface kind·IPC namespace 참조를 검증한다.
pub fn validate_bin_extras(manifest: &crate::plugin::Manifest) -> Result<()> {
    validate_detector_actual(&manifest.contributes.detector)?;
    let surface_kinds: Vec<String> = manifest
        .surface_kinds
        .iter()
        .map(|k| k.kind.clone())
        .collect();
    let ipc_prefixes: Vec<String> = manifest
        .contributes
        .ipc_namespace
        .iter()
        .map(|n| n.prefix.clone())
        .collect();
    validate_handler_actual(&manifest.contributes.handler, &surface_kinds, &ipc_prefixes)?;
    Ok(())
}

pub fn validate_detector_actual(raw: &[serde_json::Value]) -> Result<()> {
    for v in raw {
        let decl: crate::file::format::config::DetectorDecl = serde_json::from_value(v.clone())
            .map_err(|e| anyhow::anyhow!("invalid detector entry shape: {e}"))?;
        crate::file::format::config::validate_detector_decl(&decl, true)
            .map_err(|e| anyhow::anyhow!("contributes.detector '{}': {e}", decl.id))?;
    }
    Ok(())
}

pub fn validate_handler_actual(
    raw: &[serde_json::Value],
    surface_kinds: &[String],
    ipc_prefixes: &[String],
) -> Result<()> {
    for v in raw {
        let decl: crate::file::handler::config::HandlerDecl<
            crate::file::handler::config::PluginHandlerActionDecl,
        > = serde_json::from_value(v.clone())
            .map_err(|e| anyhow::anyhow!("invalid handler entry shape: {e}"))?;
        crate::file::handler::config::validate_plugin_handler_decl(&decl)
            .map_err(|e| anyhow::anyhow!("contributes.handler: {e}"))?;
        crate::file::handler::config::validate_plugin_handler_refs(
            &decl,
            surface_kinds,
            ipc_prefixes,
        )
        .map_err(|e| anyhow::anyhow!("contributes.handler: {e}"))?;
    }
    Ok(())
}
