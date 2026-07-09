//! 본체 7종 surface의 SurfaceKindDef 등록.
//!
//! 03D-A에서는 create/restore/snapshot 함수만 채운다. render/on_close는 추후 단계에서
//! 추가될 예정 (egui_panels.rs dispatch 통합과 함께).

use std::sync::Arc;

use serde_json::{Value, json};

use crate::model::{
    EmptySurface, ExplorerPanel, ExplorerTab, ExplorerViewMode, SortColumn, SortDir, Surface,
};

use super::{
    PresetFieldInput, PresetFieldSpec, PresetFieldTarget, SurfaceKindDef, SurfaceKindRegistry,
};

/// 부팅 시 호출. CoreState 생성 직전에 빈 SurfaceKindRegistry에 호스트 내장 kind를 등록한다.
///
/// 부팅 시 등록: terminal / empty / attached / **explorer**(T11 host builtin).
///
/// 부팅 시 등록되지 *않는* kind (plugin hello 시 등록):
/// - `"image"` / `"markdown"`: 각 plugin 이 hello 시 `rendering = "egui-mesh"`
///   매니페스트로 egui-mesh 화이트리스트 매칭 후 등록 (`surface_registry/egui_mesh.rs`).
pub fn register_builtin_kinds(registry: &SurfaceKindRegistry) {
    register_terminal(registry);
    register_empty(registry);
    register_attached(registry);
    register_explorer(registry);
}

/// 부팅 시 호스트가 직접 소유·등록하는 내장 kind 인지 여부.
///
/// 이 목록의 kind 는 host 가 egui 로 직접 렌더하므로, 외부/잔존 plugin 이 같은 kind
/// 문자열을 remote kind 로 다시 선언해도 **덮어쓰지 못하게** 보호된다
/// ([`crate::plugin_bridge::remote_kind::register_remote_kind`] 의 가드). 특히
/// explorer 는 과거 `com.tasty.explorer` plugin 이 제공하던 remote kind 였으나 T11
/// 에서 host builtin 으로 승격됐다 — 사용자 `~/.tasty/plugins/` 에 옛 plugin 이
/// 남아 있어도 native explorer 가 항상 우선한다.
pub fn is_host_builtin_kind(kind: &str) -> bool {
    matches!(kind, "terminal" | "empty" | "attached" | "explorer")
}

// ── Terminal ────────────────────────────────────────────────────────────────
//
// Terminal은 PTY spawn이 호스트 책임이라 create/restore에서 자체적으로 surface를
// 생성하지 않는다. 호출자(IPC handler / split_pane_targeted / SavedSurface::Terminal
// 복원)가 별도 경로를 거치므로, 여기서는 안전한 sentinel만 반환한다.

fn register_terminal(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "terminal",
        display_name_i18n_key: "surface.kind.terminal",
        icon: Some("terminal".to_string()),
        create: Arc::new(|_sid, _cwd, _params| {
            anyhow::bail!("terminal surfaces require host-managed PTY spawn; use split_pane_targeted/add_terminal_tab")
        }),
        restore: Arc::new(|_sid, _data| {
            anyhow::bail!("terminal surfaces are restored via SavedSurface::Terminal, not Generic")
        }),
        snapshot: Arc::new(|_| None),
        // terminal 은 cwd/startup 이 params 가 아니라 PresetSurface 전용 컬럼이므로
        // target 을 Cwd/Startup 으로 라우팅한다 (편집기가 generic 하게 흡수).
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
        // terminal 은 GPU-PTY surface — 줌/복사/입력은 별도 경로. capability flags 없음.
        consumes_egui_input: false,
        zoomable: false,
        egui_copy: false,
        copy_path: false,
        egui_paste: false,
        // terminal 표시명은 surface 자체 display_name 으로 결정 — 파라미터 basename 명명 없음.
        name_from_param: None,
        // builtin kind 는 recent 기록 대상 아님(파일-open recent 는 plugin kind 소유).
        records_recent: false,
    });
}

// ── Attached ──────────────────────────────────────────────────────────────
//
// attached surface 는 attach 핸들러(단계 4)가 직접 생성하는 런타임 marker
// (배타 점유 lock 의 양쪽 표현 — placeholder/mirror). create/restore 경로 없음
// (sentinel bail). decision 2(휘발성): snapshot=None 으로 layout.json 에서 제외 —
// 재시작 시 내부 Terminal 이 일반 `SavedSurface::Terminal` 로 free 환원된다.

fn register_attached(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "attached",
        display_name_i18n_key: "surface.kind.attached",
        // attached 는 배타 점유 marker — 내부 Terminal 의 mirror 라 terminal 아이콘.
        icon: Some("terminal".to_string()),
        create: Arc::new(|_sid, _cwd, _params| {
            anyhow::bail!(
                "attached surfaces are created by the attach handler, not via registry create"
            )
        }),
        restore: Arc::new(|_sid, _data| {
            anyhow::bail!("attached surfaces are volatile (decision 2); not restored")
        }),
        snapshot: Arc::new(|_| None),
        // attached 는 사용자가 프리셋으로 만들 수 없는 런타임 marker — 편집 필드 없음.
        preset_fields: Vec::new(),
        param_aliases: std::collections::HashMap::new(),
        default_params: std::collections::HashMap::new(),
        consumes_egui_input: false,
        zoomable: false,
        egui_copy: false,
        copy_path: false,
        egui_paste: false,
        name_from_param: None,
        // builtin kind 는 recent 기록 대상 아님(파일-open recent 는 plugin kind 소유).
        records_recent: false,
    });
}

// ── Explorer ──────────────────────────────────────────────────────────────
//
// 본체 내장 파일 관리자 (T11). 과거엔 `com.tasty.explorer` plugin 의 remote kind
// 였으나 본체 surface 로 승격됐다. create 는 `path` param(없으면 cwd, 그래도 없으면
// ".")으로 단일 탭 생성. snapshot/restore 는 내부 탭 목록(root + view_mode + 정렬)과
// 활성 탭 인덱스를 직렬화한다(결정 3 — 내부 탭은 surface 와 함께 복원). 히스토리
// (back/forward)는 휘발성이라 직렬화하지 않는다.

fn register_explorer(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "explorer",
        display_name_i18n_key: "surface.kind.explorer",
        icon: Some("folder".to_string()),
        create: Arc::new(|sid, cwd, params| {
            let root = params
                .get("path")
                .and_then(|v| v.as_str())
                .map(std::path::PathBuf::from)
                .or_else(|| cwd.map(std::path::PathBuf::from))
                .unwrap_or_else(|| std::path::PathBuf::from("."));
            // host(`create_surface_via_registry`)가 마지막 view mode 를 주입한다.
            // 미지정 시 ExplorerViewMode::from_str 이 detail 로 fallback.
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
        // explorer create 는 params.path 미지정 시 cwd 를 루트로 쓴다 → 편집기가 cwd
        // 컬럼으로 루트 디렉토리를 입력하게 target=Cwd 로 둔다(기존 동작 보존).
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
        // kind별 기본값 주입(host 정책 토큰): view_mode 는 Settings 의 마지막 view mode,
        // path 는 새 탭 컨텍스트(cwd 상속 없음)에서만 home 으로 보정된다(`@home` 은
        // tab.create 만 해석 — split/preset/workspace 회귀 방지, 아래 해석기 참고).
        default_params: std::collections::HashMap::from([
            (
                "view_mode".to_string(),
                "@settings.explorer_view_mode".to_string(),
            ),
            ("path".to_string(), "@home".to_string()),
        ]),
        // explorer 는 host egui 위젯으로 렌더 → 키/IME 를 host egui 로 라우팅.
        // 줌(폰트 크기)·select-all/copy-path 단축키 소비. copy(egui Copy)/paste 는 아님.
        consumes_egui_input: true,
        zoomable: true,
        egui_copy: false,
        copy_path: true,
        egui_paste: false,
        // explorer 탭 표시명은 현재 폴더 `path` basename 으로 파생.
        name_from_param: Some("path".to_string()),
        // builtin kind 는 recent 기록 대상 아님(파일-open recent 는 plugin kind 소유).
        records_recent: false,
    });
}

/// snapshot JSON 한 항목 → `ExplorerTab` (히스토리 제외, cwd/current/view_mode/정렬 복원).
fn explorer_tab_from_json(v: &Value) -> ExplorerTab {
    // current(현재 폴더). 구 스냅샷은 `root` 만 있다.
    let current = v
        .get("root")
        .and_then(|x| x.as_str())
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    // cwd(고정 루트). 구 스냅샷 호환: 키 없으면 current 로 cwd·current 동일 설정.
    let cwd = v
        .get("cwd")
        .and_then(|x| x.as_str())
        .map(std::path::PathBuf::from)
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

// ── Empty ───────────────────────────────────────────────────────────────────

fn register_empty(registry: &SurfaceKindRegistry) {
    registry.register(SurfaceKindDef {
        kind: "empty",
        display_name_i18n_key: "surface.kind.empty",
        // empty 는 placeholder — 전용 아이콘 없이 UI fallback(FILE).
        icon: None,
        create: Arc::new(|sid, cwd, _params| {
            Ok(
                Box::new(EmptySurface::new(sid).with_cwd(cwd.map(std::path::PathBuf::from)))
                    as Box<dyn Surface>,
            )
        }),
        restore: Arc::new(|sid, _data| Ok(Box::new(EmptySurface::new(sid)) as Box<dyn Surface>)),
        snapshot: Arc::new(|_| Some(Value::Object(Default::default()))),
        // empty 는 placeholder surface — 편집 필드 없음.
        preset_fields: Vec::new(),
        param_aliases: std::collections::HashMap::new(),
        default_params: std::collections::HashMap::new(),
        consumes_egui_input: false,
        zoomable: false,
        egui_copy: false,
        copy_path: false,
        egui_paste: false,
        name_from_param: None,
        // builtin kind 는 recent 기록 대상 아님(파일-open recent 는 plugin kind 소유).
        records_recent: false,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Surface cwd invariant: SurfaceKindDef.create 가 받은 cwd 가 EmptySurface
        // 본체에 carry 되어 source_cwd() 로 그대로 노출되어야 한다.
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
        // create: path param 우선.
        let s = (def.create)(5, None, &json!({"path": "/tmp/exp"})).unwrap();
        assert_eq!(s.kind(), "explorer");
        assert_eq!(s.surface_id(), Some(5));
        // snapshot → restore 라운드트립 (탭 cwd/current + 활성 인덱스 보존).
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert_eq!(snap["tabs"][0]["cwd"], "/tmp/exp");
        assert_eq!(snap["tabs"][0]["root"], "/tmp/exp");
        let restored = (def.restore)(5, &snap).unwrap();
        assert_eq!(restored.kind(), "explorer");
        let ex = restored
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert_eq!(ex.current_root().to_string_lossy(), "/tmp/exp");
        assert_eq!(ex.cwd().to_string_lossy(), "/tmp/exp");
    }

    #[test]
    fn explorer_restore_old_snapshot_without_cwd() {
        // 구 스냅샷 호환: `cwd` 키가 없으면 `root` 값으로 cwd·current 를 동일 설정.
        let old = json!({
            "tabs": [{"root": "/x", "view_mode": "detail", "sort_column": "name", "sort_dir": "asc"}],
            "active": 0,
        });
        let reg = registry_with_builtins();
        let def = reg.get("explorer").unwrap();
        let restored = (def.restore)(9, &old).unwrap();
        let ex = restored
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert_eq!(ex.cwd().to_string_lossy(), "/x");
        assert_eq!(ex.current_root().to_string_lossy(), "/x");
    }

    #[test]
    fn explorer_create_defaults_to_cwd() {
        let reg = registry_with_builtins();
        let def = reg.get("explorer").unwrap();
        let cwd = std::path::PathBuf::from("/tmp/cwd-default");
        let s = (def.create)(1, Some(cwd.as_path()), &json!({})).unwrap();
        let ex = s
            .as_any()
            .downcast_ref::<crate::model::ExplorerPanel>()
            .unwrap();
        assert_eq!(ex.current_root(), cwd.as_path());
    }

    #[test]
    fn terminal_create_errors() {
        let reg = registry_with_builtins();
        let def = reg.get("terminal").unwrap();
        assert!((def.create)(1, None, &json!({})).is_err());
    }

    #[test]
    fn explorer_capability_flags() {
        // explorer 는 host egui 렌더 → 입력 라우팅 + 줌 + select-all/copy-path 소비.
        // copy(egui Copy)/paste 는 아님.
        let reg = registry_with_builtins();
        let ex = reg.get("explorer").unwrap();
        assert!(ex.consumes_egui_input);
        assert!(ex.zoomable);
        assert!(ex.copy_path);
        assert!(!ex.egui_copy);
        assert!(!ex.egui_paste);
        // terminal 은 GPU-PTY — capability flags 없음.
        let term = reg.get("terminal").unwrap();
        assert!(!term.consumes_egui_input);
        assert!(!term.zoomable);
        assert!(!term.copy_path);
    }

    #[test]
    fn attached_is_volatile_sentinel() {
        // attached kind 는 attach 핸들러가 직접 생성하는 런타임 marker —
        // create/restore 는 sentinel bail, snapshot 은 휘발성(None, decision 2).
        let reg = registry_with_builtins();
        let def = reg.get("attached").unwrap();
        assert!((def.create)(1, None, &json!({})).is_err());
        assert!((def.restore)(1, &json!({})).is_err());
        assert!((def.snapshot)(&crate::model::EmptySurface::new(1)).is_none());
    }
}
