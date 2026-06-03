//! Manifest opaque payload (Vec<serde_json::Value>) 를 본 바이너리 file 도메인
//! concrete 타입으로 deserialize 후 추가 검증.
//!
//! Phase F.B.2 에서 `ContributesDecl::detector` / `.handler` 가 opaque 화되어
//! manifest crate 가 file 도메인을 의존하지 않게 되었다. 본 모듈은 manifest 가
//! 검증할 수 없는 *file::format::config* / *file::handler::config* 차원의
//! schema 를 추가로 검증한다 — `Manifest::validate()` 직후 호출.

use anyhow::Result;

/// Plugin manifest 의 raw detector contribute 들을 concrete `DetectorDecl` 로
/// deserialize 한 뒤 본 바이너리 `validate_detector_decl` 로 schema 검증.
pub fn validate_detector_actual(raw: &[serde_json::Value]) -> Result<()> {
    for v in raw {
        let decl: crate::file::format::config::DetectorDecl = serde_json::from_value(v.clone())
            .map_err(|e| anyhow::anyhow!("invalid detector entry shape: {e}"))?;
        crate::file::format::config::validate_detector_decl(&decl, true)
            .map_err(|e| anyhow::anyhow!("contributes.detector '{}': {e}", decl.id))?;
    }
    Ok(())
}

/// Plugin manifest 의 raw handler contribute 들을 concrete
/// `HandlerDecl<PluginHandlerActionDecl>` 로 deserialize 한 뒤 본 바이너리 schema
/// + cross-ref (surface_kinds / ipc_prefixes) 검증.
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
