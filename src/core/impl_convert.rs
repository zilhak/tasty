//! surface 종류를 바꾸고 탭 제목을 갱신한다.

use super::*;

impl Core {
    /// split 탭은 해당 leaf만 교체하고 단일 surface 탭은 전체 surface를 교체한다.
    pub(super) fn apply_convert_surface(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        target: crate::core::intent::ConvertSurfaceTarget,
    ) -> CoreEvent {
        use crate::core::intent::ConvertSurfaceTarget;

        let is_terminal = matches!(target, ConvertSurfaceTarget::Terminal { .. });

        let (new_surface, new_name) =
            match Self::create_surface_for_convert(engine, surface_id, target) {
                Ok(v) => v,
                Err(ev) => return ev,
            };

        let (ws_idx, pane_id, tab_idx) = match Self::find_surface_location(engine, surface_id) {
            Some(loc) => loc,
            None => {
                return CoreEvent::SurfaceConverted {
                    surface_id,
                    replaced: false,
                    failure: Some(format!("surface {surface_id} not found")),
                };
            }
        };

        let replaced = match Self::replace_surface_in_tab(
            engine,
            ws_idx,
            pane_id,
            tab_idx,
            surface_id,
            new_surface,
            new_name,
        ) {
            Some(r) => r,
            None => {
                return CoreEvent::SurfaceConverted {
                    surface_id,
                    replaced: false,
                    failure: Some(format!("pane {pane_id} not found")),
                };
            }
        };

        if replaced {
            engine.mark_layout_dirty();
            if is_terminal {
                engine.send_fast_init(surface_id);
            }
            // 종류·제목·명시 이름 해제가 반영되도록 자동 제목을 다시 계산한다.
            engine.refresh_tab_osc_title(surface_id);
        }

        CoreEvent::SurfaceConverted {
            surface_id,
            replaced,
            // 위치 검색에서 찾은 탭의 트리에도 대상이 있어야 한다.
            failure: (!replaced)
                .then(|| format!("surface {surface_id} not found in its tab layout")),
        }
    }

    /// 새 surface를 만든다. Terminal 생성 시 store도 바뀌므로 호출자가 이후 실패해도 되돌리지 않는다.
    fn create_surface_for_convert(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        target: crate::core::intent::ConvertSurfaceTarget,
    ) -> Result<(Box<dyn crate::model::Surface>, Option<Option<String>>), CoreEvent> {
        use crate::core::intent::ConvertSurfaceTarget;
        match target {
            ConvertSurfaceTarget::Terminal { cwd } => {
                let cols = engine.default_cols;
                let rows = engine.default_rows;
                let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
                let waker = engine.make_waker(surface_id);
                let terminal = match tasty_terminal::Terminal::new(
                    tasty_terminal::TerminalConfig {
                        cols,
                        rows,
                        shell: sh.shell_ref(),
                        args: &sh.args_ref(),
                        extra_env: &sh.envs_ref(),
                        surface_id,
                        working_dir: cwd.as_deref(),
                        initial_input: None,
                    },
                    waker,
                ) {
                    Ok(t) => t,
                    Err(e) => {
                        return Err(CoreEvent::SurfaceConverted {
                            surface_id,
                            replaced: false,
                            failure: Some(e.to_string()),
                        });
                    }
                };
                engine.terminals.insert(surface_id, terminal);
                let node = crate::model::TerminalSurface { id: surface_id };
                // 단일 surface 탭에서는 기존 명시 이름을 지우고 자동 제목을 사용한다.
                Ok((Box::new(node), Some(None)))
            }
            ConvertSurfaceTarget::Kind { cwd, kind, params } => {
                let new_surface = match engine.create_surface_via_registry(
                    &kind,
                    surface_id,
                    cwd.as_deref(),
                    &params,
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!("ConvertSurface kind='{}' failed: {}", kind, e);
                        return Err(CoreEvent::SurfaceConverted {
                            surface_id,
                            replaced: false,
                            failure: Some(e.to_string()),
                        });
                    }
                };
                let auto_name =
                    derive_auto_name(engine.surface_registry.get(&kind).as_deref(), &params);
                Ok((new_surface, Some(auto_name)))
            }
        }
    }

    fn find_surface_location(
        engine: &crate::core::CoreState,
        surface_id: u32,
    ) -> Option<(usize, u32, usize)> {
        for (ws_idx, workspace) in engine.workspaces.iter().enumerate() {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                        if tab.contains_surface(surface_id) {
                            return Some((ws_idx, pid, tab_idx));
                        }
                    }
                }
            }
        }
        None
    }

    /// split 탭은 이름을 유지한다. 단일 surface 탭은 new_name으로 명시 이름도 바꾼다.
    fn replace_surface_in_tab(
        engine: &mut crate::core::CoreState,
        ws_idx: usize,
        pane_id: u32,
        tab_idx: usize,
        surface_id: u32,
        new_surface: Box<dyn crate::model::Surface>,
        new_name: Option<Option<String>>,
    ) -> Option<bool> {
        let ws = &mut engine.workspaces[ws_idx];
        let pane = ws.pane_layout_mut().find_pane_mut(pane_id)?;
        let tab = &mut pane.tabs[tab_idx];
        if tab.is_split() {
            Some(tab.layout_mut().replace_surface(surface_id, new_surface))
        } else {
            tab.put_surface(new_surface);
            if let Some(name_opt) = new_name {
                tab.explicit_name = name_opt;
            }
            Some(true)
        }
    }
}

/// 등록부의 name_from_param이 가리키는 경로에서 파일명을 얻는다. 선언·값·파일명이 없으면 None이다.
fn derive_auto_name(
    def: Option<&crate::core::surface_registry::SurfaceKindDef>,
    params: &serde_json::Value,
) -> Option<String> {
    let key = def.and_then(|d| d.name_from_param.as_deref())?;
    let p = params.get(key).and_then(|v| v.as_str())?;
    std::path::Path::new(p)
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
}

#[cfg(test)]
mod derive_auto_name_tests {
    use super::derive_auto_name;
    use crate::core::surface_registry::{SurfaceKindRegistry, register_builtin_kinds};

    #[test]
    fn name_from_param_kind_yields_basename_else_none() {
        let reg = SurfaceKindRegistry::new();
        register_builtin_kinds(&reg);
        let explorer = reg.get("explorer").unwrap();
        assert_eq!(
            derive_auto_name(Some(&explorer), &serde_json::json!({"path": "/a/b/proj"})),
            Some("proj".to_string())
        );
        assert_eq!(
            derive_auto_name(Some(&explorer), &serde_json::json!({})),
            None
        );
        let empty = reg.get("empty").unwrap();
        assert_eq!(
            derive_auto_name(Some(&empty), &serde_json::json!({"path": "/a/b"})),
            None
        );
        assert_eq!(
            derive_auto_name(None, &serde_json::json!({"file": "/x.md"})),
            None
        );
    }
}
