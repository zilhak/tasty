//! host 내장 surface의 생성·복원·snapshot 동작을 등록한다.

use std::sync::Arc;

use serde_json::{Value, json};

use crate::model::{
    DagDirection, DagGraphSurface, EmptySurface, ExplorerPanel, ExplorerTab, ExplorerViewMode,
    SortColumn, SortDir, Surface, resolve_root,
};

use super::{
    KindSource, PresetFieldInput, PresetFieldSpec, PresetFieldTarget, RegisteredRendering,
    SurfaceKindDef, SurfaceKindRegistry,
};

pub fn register_builtin_kinds(registry: &SurfaceKindRegistry) {
    register_terminal(registry);
    register_empty(registry);
    register_explorer(registry);
    register_dag_graph(registry);
}

/// plugin 등록 경로가 host 내장 종류를 덮어쓰지 않도록 확인하는 목록.
pub fn is_host_builtin_kind(kind: &str) -> bool {
    matches!(kind, "terminal" | "empty" | "explorer" | "dag_graph")
}

// Terminal의 PTY 생성·복원은 host가 별도로 처리하므로 이 등록의 create/restore는 오류를 반환한다.

fn register_terminal(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "terminal",
        rendering: RegisteredRendering::HostEgui,
        source: KindSource::HostBuiltin,
        display_name_i18n_key: "surface.kind.terminal",
        icon: Some("terminal".to_string()),
        create: Arc::new(|_sid, _cwd, _params| {
            anyhow::bail!("terminal surfaces require host-managed PTY spawn; use split_pane_targeted/add_terminal_tab")
        }),
        restore: Arc::new(|_sid, _data| {
            anyhow::bail!("terminal surfaces are restored via SavedSurface::Terminal, not Generic")
        }),
        snapshot: Arc::new(|_| None),
        // terminal의 cwd·startup은 params가 아닌 PresetSurface 전용 필드에 저장한다.
        preset_fields: vec![
            PresetFieldSpec {
                id: "cwd".to_string(),
                label_key: "preset.edit.cwd".to_string(),
                target: PresetFieldTarget::Cwd,
                input: PresetFieldInput::Dir,
                required: false,
                placeholder_key: None,
                default: None,
                derive_cwd: false,
            },
            PresetFieldSpec {
                id: "startup".to_string(),
                label_key: "preset.edit.startup".to_string(),
                target: PresetFieldTarget::Startup,
                input: PresetFieldInput::Text,
                required: false,
                placeholder_key: Some("preset.edit.startup_hint".to_string()),
                default: None,
                derive_cwd: false,
            },
        ],
        param_aliases: std::collections::HashMap::new(),
        default_params: std::collections::HashMap::new(),
        // terminal 입력·줌·복사는 이 기능 플래그 대신 별도 PTY/GPU 경로를 쓴다.
        consumes_egui_input: false,
        zoomable: false,
        egui_copy: false,
        copy_path: false,
        egui_paste: false,
        name_from_param: None,
        records_recent: false,
        convert_requires_input: false,
        convert_input_popup: None,
    });
}

// Explorer는 내부 탭과 보기·정렬을 저장한다. 앞뒤 이동 이력은 저장하지 않는다.

fn register_explorer(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "explorer",
        rendering: RegisteredRendering::HostEgui,
        source: KindSource::HostBuiltin,
        display_name_i18n_key: "surface.kind.explorer",
        icon: Some("folder".to_string()),
        create: Arc::new(|sid, cwd, params| {
            // 명시 path가 있으면 cwd보다 우선한다. 상대 path는 resolve_root가 기본 경로로 바꾸며 cwd로 재시도하지 않는다.
            let root = resolve_root(
                params
                    .get("path")
                    .and_then(|v| v.as_str())
                    .map(std::path::PathBuf::from)
                    .or_else(|| cwd.map(std::path::PathBuf::from)),
            );
            let view_mode = params
                .get("view_mode")
                .and_then(|v| v.as_str())
                .map(ExplorerViewMode::from_str)
                .unwrap_or(ExplorerViewMode::Detail);
            Ok(Box::new(ExplorerPanel::new_with_mode(sid, root, view_mode)) as Box<dyn Surface>)
        }),
        restore: Arc::new(|sid, data| {
            let active = data.get("active").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
            let tabs: Vec<ExplorerTab> = data
                .get("tabs")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().map(explorer_tab_from_json).collect())
                .unwrap_or_default();
            Ok(Box::new(ExplorerPanel::from_tabs(sid, tabs, active)) as Box<dyn Surface>)
        }),
        snapshot: Arc::new(|s: &dyn Surface| {
            let ex = s.as_any().downcast_ref::<ExplorerPanel>()?;
            let tabs: Vec<Value> = ex
                .tabs
                .iter()
                .map(|t| {
                    json!({
                        "cwd": t.cwd.to_string_lossy(),
                        "root": t.root.to_string_lossy(),
                        "view_mode": t.view_mode.as_str(),
                        "sort_column": t.sort_column.as_str(),
                        "sort_dir": t.sort_dir.as_str(),
                    })
                })
                .collect();
            Some(json!({ "tabs": tabs, "active": ex.active }))
        }),
        // params.path가 없으면 cwd를 쓰므로 편집기는 전용 cwd 필드에 저장한다.
        preset_fields: vec![PresetFieldSpec {
            id: "cwd".to_string(),
            label_key: "preset.edit.cwd".to_string(),
            target: PresetFieldTarget::Cwd,
            input: PresetFieldInput::Dir,
            required: false,
            placeholder_key: None,
            default: None,
            derive_cwd: false,
        }],
        param_aliases: std::collections::HashMap::new(),
        // @home은 호출자가 홈을 제공한 새 탭 경로에서 해석한다. 상속 cwd 경로에서는 해석하지 않는다.
        default_params: std::collections::HashMap::from([
            (
                "view_mode".to_string(),
                "@settings.explorer_view_mode".to_string(),
            ),
            ("path".to_string(), "@home".to_string()),
        ]),
        consumes_egui_input: true,
        zoomable: true,
        egui_copy: false,
        copy_path: true,
        egui_paste: false,
        name_from_param: Some("path".to_string()),
        records_recent: false,
        convert_requires_input: false,
        convert_input_popup: None,
    });
}

fn explorer_tab_from_json(v: &Value) -> ExplorerTab {
    // root가 없거나 상대 경로이면 기본 경로로 보정한다.
    let current = resolve_root(
        v.get("root")
            .and_then(|x| x.as_str())
            .map(std::path::PathBuf::from),
    );
    // cwd가 없는 옛 snapshot은 current를 사용하며 상대 cwd도 current로 바꾼다.
    let cwd = v
        .get("cwd")
        .and_then(|x| x.as_str())
        .map(std::path::PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| current.clone());
    let mut tab = ExplorerTab::with_cwd(cwd, current);
    if let Some(m) = v.get("view_mode").and_then(|x| x.as_str()) {
        tab.view_mode = ExplorerViewMode::from_str(m);
    }
    if let Some(c) = v.get("sort_column").and_then(|x| x.as_str()) {
        tab.sort_column = SortColumn::from_str(c);
    }
    if let Some(d) = v.get("sort_dir").and_then(|x| x.as_str()) {
        tab.sort_dir = SortDir::from_str(d);
    }
    tab
}

fn register_empty(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "empty",
        rendering: RegisteredRendering::HostEgui,
        source: KindSource::HostBuiltin,
        display_name_i18n_key: "surface.kind.empty",
        icon: None,
        create: Arc::new(|sid, cwd, _params| {
            Ok(
                Box::new(EmptySurface::new(sid).with_cwd(cwd.map(std::path::PathBuf::from)))
                    as Box<dyn Surface>,
            )
        }),
        restore: Arc::new(|sid, _data| Ok(Box::new(EmptySurface::new(sid)) as Box<dyn Surface>)),
        snapshot: Arc::new(|_| Some(Value::Object(Default::default()))),
        preset_fields: Vec::new(),
        param_aliases: std::collections::HashMap::new(),
        default_params: std::collections::HashMap::new(),
        consumes_egui_input: false,
        zoomable: false,
        egui_copy: false,
        copy_path: false,
        egui_paste: false,
        name_from_param: None,
        records_recent: false,
        convert_requires_input: false,
        convert_input_popup: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::abs_path;

    fn registry_with_builtins() -> SurfaceKindRegistry {
        let r = SurfaceKindRegistry::new();
        register_builtin_kinds(&r);
        r
    }

    #[test]
    fn builtin_kinds_no_longer_include_image() {
        let reg = registry_with_builtins();
        assert!(
            reg.get("image").is_none(),
            "image kind must be registered via com.tasty.image plugin, not register_builtin_kinds"
        );
    }

    #[test]
    fn builtin_kinds_no_longer_include_markdown() {
        let reg = registry_with_builtins();
        assert!(
            reg.get("markdown").is_none(),
            "markdown kind must be registered via com.tasty.markdown plugin, not register_builtin_kinds"
        );
    }

    #[test]
    fn empty_snapshot_returns_object() {
        let reg = registry_with_builtins();
        let def = reg.get("empty").unwrap();
        let s = (def.create)(1, None, &json!({})).unwrap();
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert!(snap.is_object());
    }

    #[test]
    fn empty_carries_cwd_from_create() {
        let reg = registry_with_builtins();
        let def = reg.get("empty").unwrap();
        let cwd = std::path::PathBuf::from("/tmp/carry-test");
        let s = (def.create)(1, Some(cwd.as_path()), &json!({})).unwrap();
        assert_eq!(s.source_cwd().as_deref(), Some(cwd.as_path()));
    }

    #[test]
    fn explorer_is_builtin_and_round_trips() {
        let reg = registry_with_builtins();
        let def = reg
            .get("explorer")
            .expect("explorer is a host builtin kind");
        let root = abs_path("tmp/exp");
        let root_str = root.to_string_lossy().into_owned();
        let s = (def.create)(5, None, &json!({ "path": &root_str })).unwrap();
        assert_eq!(s.kind(), "explorer");
        assert_eq!(s.surface_id(), Some(5));
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert_eq!(snap["tabs"][0]["cwd"], root_str);
        assert_eq!(snap["tabs"][0]["root"], root_str);
        let restored = (def.restore)(5, &snap).unwrap();
        assert_eq!(restored.kind(), "explorer");
        let ex = restored
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert_eq!(ex.current_root(), root.as_path());
        assert_eq!(ex.cwd(), root.as_path());
    }

    #[test]
    fn explorer_restore_old_snapshot_without_cwd() {
        let root = abs_path("x");
        let old = json!({
            "tabs": [{"root": root.to_string_lossy(), "view_mode": "detail", "sort_column": "name", "sort_dir": "asc"}],
            "active": 0,
        });
        let reg = registry_with_builtins();
        let def = reg.get("explorer").unwrap();
        let restored = (def.restore)(9, &old).unwrap();
        let ex = restored
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert_eq!(ex.cwd(), root.as_path());
        assert_eq!(ex.current_root(), root.as_path());
    }

    #[test]
    fn explorer_create_defaults_to_cwd() {
        let reg = registry_with_builtins();
        let def = reg.get("explorer").unwrap();
        let cwd = abs_path("tmp/cwd-default");
        let s = (def.create)(1, Some(cwd.as_path()), &json!({})).unwrap();
        let ex = s
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert_eq!(ex.current_root(), cwd.as_path());
    }

    #[test]
    fn explorer_create_without_path_or_cwd_falls_back_to_absolute_root() {
        let reg = registry_with_builtins();
        let def = reg.get("explorer").unwrap();
        let s = (def.create)(1, None, &json!({})).unwrap();
        let ex = s
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        let root = ex.cwd();
        assert!(
            root.is_absolute(),
            "explorer root must never be relative: {root:?}"
        );
        assert_ne!(root, std::path::Path::new("."));
        assert_eq!(root, crate::model::default_root());
    }

    #[test]
    fn explorer_create_rejects_relative_path_and_cwd() {
        let reg = registry_with_builtins();
        let def = reg.get("explorer").unwrap();
        for s in [
            (def.create)(1, None, &json!({"path": "."})).unwrap(),
            (def.create)(2, None, &json!({"path": "sub/dir"})).unwrap(),
            (def.create)(3, Some(std::path::Path::new(".")), &json!({})).unwrap(),
        ] {
            let root = s
                .as_any()
                .downcast_ref::<crate::model::ExplorerPanel>()
                .unwrap()
                .cwd()
                .to_path_buf();
            assert_eq!(
                root,
                crate::model::default_root(),
                "relative root must fall back"
            );
        }
    }

    #[test]
    fn explorer_create_preserves_explicit_absolute_priority() {
        let reg = registry_with_builtins();
        let def = reg.get("explorer").unwrap();
        let carry = abs_path("tmp/carry");
        let explicit = abs_path("tmp/explicit");
        let s = (def.create)(
            1,
            Some(carry.as_path()),
            &json!({ "path": explicit.to_string_lossy() }),
        )
        .unwrap();
        let ex = s
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert_eq!(ex.cwd(), explicit.as_path());

        let s = (def.create)(2, Some(carry.as_path()), &json!({})).unwrap();
        let ex = s
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert_eq!(ex.cwd(), carry.as_path());
    }

    #[test]
    fn explorer_restore_normalizes_relative_snapshot_root() {
        let reg = registry_with_builtins();
        let def = reg.get("explorer").unwrap();
        let home = crate::model::default_root();

        let dotted = json!({"tabs": [{"root": ".", "cwd": "."}], "active": 0});
        let ex = (def.restore)(1, &dotted).unwrap();
        let ex = ex
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert_eq!(ex.cwd(), home.as_path());
        assert_eq!(ex.current_root(), home.as_path());

        let missing = json!({"tabs": [{"view_mode": "detail"}], "active": 0});
        let ex = (def.restore)(2, &missing).unwrap();
        let ex = ex
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert!(ex.cwd().is_absolute());
        assert_eq!(ex.cwd(), home.as_path());

        let empty = json!({"tabs": [], "active": 0});
        let ex = (def.restore)(3, &empty).unwrap();
        let ex = ex
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert!(ex.cwd().is_absolute());

        let xy = abs_path("x/y");
        let mixed = json!({"tabs": [{"root": xy.to_string_lossy(), "cwd": "."}], "active": 0});
        let ex = (def.restore)(4, &mixed).unwrap();
        let ex = ex
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert_eq!(ex.cwd(), xy.as_path());
        assert_eq!(ex.current_root(), xy.as_path());
    }

    #[test]
    fn terminal_create_errors() {
        let reg = registry_with_builtins();
        let def = reg.get("terminal").unwrap();
        assert!((def.create)(1, None, &json!({})).is_err());
    }

    #[test]
    fn explorer_capability_flags() {
        let reg = registry_with_builtins();
        let ex = reg.get("explorer").unwrap();
        assert!(ex.consumes_egui_input);
        assert!(ex.zoomable);
        assert!(ex.copy_path);
        assert!(!ex.egui_copy);
        assert!(!ex.egui_paste);
        let term = reg.get("terminal").unwrap();
        assert!(!term.consumes_egui_input);
        assert!(!term.zoomable);
        assert!(!term.copy_path);
    }
}

// DAG 데이터는 host가 보유하므로 내장 surface가 직접 조회한다.
// 대상·방향은 저장하지만 달라진 그래프에 낡은 화면 위치를 적용하지 않도록 줌·팬·선택은 저장하지 않는다.

fn register_dag_graph(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "dag_graph",
        rendering: RegisteredRendering::HostEgui,
        source: KindSource::HostBuiltin,
        display_name_i18n_key: "surface.kind.dag_graph",
        icon: Some("git_tree".to_string()),
        create: Arc::new(|sid, _cwd, params| {
            Ok(Box::new(DagGraphSurface::with_target(
                sid,
                dag_id_param(params),
                params.get("workspace_id").and_then(parse_workspace_id),
                params
                    .get("direction")
                    .and_then(|v| v.as_str())
                    .map(DagDirection::from_str)
                    .unwrap_or_default(),
            )) as Box<dyn Surface>)
        }),
        restore: Arc::new(|sid, data| {
            Ok(Box::new(DagGraphSurface::with_target(
                sid,
                dag_id_param(data),
                data.get("workspace_id").and_then(parse_workspace_id),
                data.get("direction")
                    .and_then(|v| v.as_str())
                    .map(DagDirection::from_str)
                    .unwrap_or_default(),
            )) as Box<dyn Surface>)
        }),
        snapshot: Arc::new(|s: &dyn Surface| {
            let dag = s.as_any().downcast_ref::<DagGraphSurface>()?;
            let mut obj = json!({ "direction": dag.direction.as_str() });
            if let Some(id) = &dag.dag_id {
                obj["dag_id"] = json!(id);
            }
            if let Some(ws) = dag.workspace_id {
                obj["workspace_id"] = json!(ws);
            }
            Some(obj)
        }),
        // 프리셋은 관찰 대상만 받는다. 방향은 화면에서 바꾸는 설정이다.
        preset_fields: vec![PresetFieldSpec {
            id: "dag_id".to_string(),
            label_key: "preset.edit.dag_id".to_string(),
            target: PresetFieldTarget::Params("dag_id".to_string()),
            input: PresetFieldInput::Text,
            required: false,
            placeholder_key: Some("preset.edit.dag_id_hint".to_string()),
            default: None,
            derive_cwd: false,
        }],
        param_aliases: std::collections::HashMap::from([("dag".to_string(), "dag_id".to_string())]),
        default_params: std::collections::HashMap::new(),
        consumes_egui_input: true,
        // UI 폰트 줌과 그래프 줌은 다르므로 같은 입력으로 둘을 함께 바꾸지 않는다.
        zoomable: false,
        egui_copy: false,
        copy_path: false,
        egui_paste: false,
        name_from_param: None,
        records_recent: false,
        convert_requires_input: false,
        convert_input_popup: None,
    });
}

fn dag_id_param(v: &Value) -> Option<String> {
    v.get("dag_id")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_workspace_id(v: &Value) -> Option<u32> {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
        .and_then(|n| u32::try_from(n).ok())
}
