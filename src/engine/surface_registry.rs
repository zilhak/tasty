//! Surface 종류별 메타·동작 정의 레지스트리.
//!
//! 본체 4종(Terminal/Markdown/Html/Empty)이 부팅 시 등록된다. Explorer/Image
//! 등은 plugin이 hello 시점에 같은 레지스트리에 자기 kind를 추가한다.
//! 외부 plugin은 단계 05에서 같은 레지스트리에 추가될 예정.
//!
//! # 단계
//!
//! - **03C**: 빈 골격(kind 식별자만).
//! - **03D-A** (현재): `create` / `restore` / `snapshot` 함수 포인터 등록. 03E에서
//!   `SavedSurface::Generic`이 snapshot/restore를 호출하고, 03F에서 IPC handler가
//!   create를 호출한다.
//! - **추후**: render 함수 + RenderStores + SurfaceCtx + SurfaceAction을 도입해
//!   `egui_panels::draw_egui_panels`의 다운캐스트 분기를 dispatch로 대체한다 (이후
//!   단계에서 다운캐스트 메서드 6종 제거).

pub mod builtins;
pub mod meta;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::model::{Surface, SurfaceId};

pub use builtins::register_builtin_kinds;

/// `ClosedPanel::from_*` 가족이 받는 snapshot 클로저를 registry 위에서 만든다.
/// 호출자는 결과를 `&mut`로 가지고 한 capture 트랜잭션 동안 reuse한다.
pub fn snapshot_fn_for(
    registry: &SurfaceKindRegistry,
) -> impl FnMut(&dyn Surface) -> Option<serde_json::Value> + '_ {
    move |s| registry.get(s.kind()).and_then(|def| (def.snapshot)(s))
}

/// surface 의 직렬화 가능한 영속 데이터를 추출하는 콜백 타입. `None` 은
/// 영속화 제외 (휘발성 surface). [`SurfaceKindDef::snapshot`] 이 사용한다.
pub type SurfaceSnapshotFn = Arc<dyn Fn(&dyn Surface) -> Option<serde_json::Value> + Send + Sync>;

/// 편집기가 쓰는 프리셋 필드 값의 저장 대상. plugin kind 는 항상 `Params(param_key)`
/// (= `PresetSurface.params.<key>`) 로 write 하지만, builtin terminal 의 cwd/startup 은
/// `params` 가 아니라 `PresetSurface` 의 전용 컬럼이라 별도 target 으로 라우팅한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PresetFieldTarget {
    /// `PresetSurface.params.<key>` 에 문자열로 write.
    Params(String),
    /// `PresetSurface.cwd` 전용 컬럼 (builtin terminal/explorer).
    Cwd,
    /// `PresetSurface.startup_command` 전용 컬럼 (builtin terminal).
    Startup,
}

/// [`PresetFieldSpec::input`] — 편집 위젯 결정. 매니페스트 `PresetFieldInputType`
/// 의 host 측 미러.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetFieldInput {
    Text,
    FilePath,
    Dir,
    Url,
}

/// [`SurfaceKindDef`] 에 실리는 프리셋 편집 필드의 host 측 런타임 표현. 매니페스트
/// `PresetFieldDecl` 을 [`PresetFieldSpec::from_decl`] 로 변환하거나, builtin kind 는
/// 코드에서 직접 구성한다.
#[derive(Clone, Debug, PartialEq)]
pub struct PresetFieldSpec {
    /// kind 내 항목 식별자 (egui salt / 안정 참조).
    pub id: String,
    /// 항목 라벨 i18n 키.
    pub label_key: String,
    /// 값 저장 대상 (params 키 / cwd / startup).
    pub target: PresetFieldTarget,
    /// 편집 위젯 결정.
    pub input: PresetFieldInput,
    /// 프리셋 적용에 필수인지 (편집기 표시/검증 힌트).
    pub required: bool,
    /// 빈 입력 placeholder i18n 키.
    pub placeholder_key: Option<String>,
    /// kind 로 새로 전환 시 초기값.
    pub default: Option<String>,
    /// 적용 시 이 필드 경로의 부모 디렉토리를 cwd 로 파생 (file_path + params 전용).
    pub derive_cwd: bool,
}

impl PresetFieldSpec {
    /// 매니페스트 `PresetFieldDecl` → host spec. plugin 선언은 항상 `param_key` 로
    /// params 에 write 하므로 target 은 언제나 [`PresetFieldTarget::Params`].
    pub fn from_decl(decl: &crate::plugin::manifest::PresetFieldDecl) -> Self {
        use crate::plugin::manifest::PresetFieldInputType as It;
        let input = match decl.input_type {
            It::Text => PresetFieldInput::Text,
            It::FilePath => PresetFieldInput::FilePath,
            It::Dir => PresetFieldInput::Dir,
            It::Url => PresetFieldInput::Url,
        };
        Self {
            id: decl.id.clone(),
            label_key: decl.label_key.clone(),
            target: PresetFieldTarget::Params(decl.param_key.clone()),
            input,
            required: decl.required,
            placeholder_key: decl.placeholder_key.clone(),
            default: decl.default.clone(),
            derive_cwd: decl.derive_cwd,
        }
    }

    /// 매니페스트 decl 슬라이스 → host spec vec (등록 경로 3곳 공용 헬퍼).
    pub fn from_decls(decls: &[crate::plugin::manifest::PresetFieldDecl]) -> Vec<Self> {
        decls.iter().map(Self::from_decl).collect()
    }
}

/// surface 종류별 메타 + 동작 함수 묶음.
///
/// 모든 함수는 `Send + Sync + 'static`이며, `Arc<SurfaceKindDef>` 단위로 보관되어
/// 매 프레임 lookup 비용을 Arc clone 한 번으로 제한한다.
pub struct SurfaceKindDef {
    /// 안정 식별자 (lowercase snake_case). 예: `"terminal"`, `"markdown"`.
    pub kind: &'static str,

    /// 사용자에게 표시되는 표시명 i18n 키. 예: `"surface.kind.markdown"`.
    /// 03D-A에서는 자리만 둔다 — 현재 표시명은 surface 자체의 `display_name()` 메서드를 사용.
    pub display_name_i18n_key: &'static str,

    /// 탭/프리셋 leading 아이콘의 아이콘 **이름**(매니페스트 `icon`). host 는 이 이름을
    /// `icons::from_name` 으로 glyph 에 매핑한다 — 본체의 `match kind { "markdown" => MD }`
    /// 하드코딩을 generic 화. builtin 은 등록 코드에서, plugin kind 는 decl 에서 채운다.
    /// `None` 이면 UI fallback(FILE).
    pub icon: Option<String>,

    /// 새 surface 인스턴스를 만든다. 03F에서 IPC handler / `add_kind_tab` /
    /// `split_pane_targeted` 가 이 함수를 호출하여 종류별 분기를 일원화한다.
    ///
    /// `cwd` 는 호출자가 결정한 *carry cwd* — 사용자가 보고 있던 surface 의 source_cwd
    /// 를 그대로 전달한다. surface 생성자가 사용 여부를 결정 (예: explorer 는 root,
    /// terminal 은 spawn 시 working_dir). Surface cwd invariant: 호출자는 *반드시*
    /// resolve 후 명시 전달. 자세한 규칙은 `docs/architecture/invariants/surface-cwd.md`.
    ///
    /// `params`는 IPC/CLI에서 받은 JSON. 종류별로 필요한 키가 다르다 (예: markdown은 `"file"`,
    /// html은 `"url"`).
    #[allow(clippy::type_complexity)]
    pub create: Arc<
        dyn Fn(SurfaceId, Option<&Path>, &serde_json::Value) -> anyhow::Result<Box<dyn Surface>>
            + Send
            + Sync,
    >,

    /// 영속화된 데이터(`SavedSurface::Generic.data`)에서 surface를 복원한다.
    /// 03E에서 layout.json v1→v2 마이그레이션 후 사용된다.
    ///
    /// Terminal은 PTY spawn이 호스트 책임이라 별도 경로(`SavedSurface::Terminal`)를 거치며,
    /// terminal builtin의 `restore`는 호출되지 않는다 (안전한 sentinel을 반환).
    #[allow(clippy::type_complexity)]
    pub restore: Arc<
        dyn Fn(SurfaceId, &serde_json::Value) -> anyhow::Result<Box<dyn Surface>> + Send + Sync,
    >,

    /// surface의 직렬화 가능한 영속 데이터를 반환한다. `None`이면 영속화에서 제외.
    /// 03E의 `SavedSurface::capture_surface`가 호출한다. 휘발성 surface는 `None`을
    /// 반환하여 layout 저장에서 빠진다.
    pub snapshot: SurfaceSnapshotFn,

    /// 프리셋 편집기가 이 kind 를 편집할 때 노출할 입력 필드 스키마. plugin kind 는
    /// 매니페스트 `preset_fields` 에서, builtin 은 등록 코드에서 채운다. 빈 vec 이면
    /// kind 전용 필드가 없다(편집기 fallback 이 kind 별 기본 필드로 떨어짐).
    pub preset_fields: Vec<PresetFieldSpec>,

    /// surface 열기 요청 params 의 key alias → canonical 키 매핑. caller 가 옛 키로
    /// 넘기면 host 가 canonical 키로 정규화한다(매니페스트 `param_aliases`). 본체의
    /// `kind == "markdown"` 일 때 `file_path`→`file` 정규화 같은 결합을 generic 화한다.
    /// builtin 은 등록 코드에서, plugin kind 는 decl 에서 채운다.
    pub param_aliases: HashMap<String, String>,

    /// surface 생성 시 params 에 없으면 host 가 주입하는 kind별 기본값(매니페스트
    /// `default_params`). 값은 리터럴이거나 정책 토큰(`@settings.explorer_view_mode`,
    /// `@home`)이다. 정책 토큰 해석은 host 정책이라 host 에 남고, "어느 kind 가 어떤
    /// 기본키를 요구하는가"만 decl 로 옮긴 것. 본체의 `kind == "explorer"` 기본값 주입
    /// 하드코딩을 generic 화한다. builtin 은 등록 코드에서, plugin kind 는 decl 에서 채운다.
    pub default_params: HashMap<String, String>,

    /// capability flags — host 의 `kind == "..."` 입력/줌/복사 게이트를 generic 화한다.
    /// 각 의미는 매니페스트 [`crate::plugin::manifest::SurfaceKindDecl`] 의 동명 필드 참조.
    /// builtin 은 등록 코드에서, plugin kind 는 decl 에서 채운다.
    pub consumes_egui_input: bool,
    pub zoomable: bool,
    pub egui_copy: bool,
    pub copy_path: bool,
    pub egui_paste: bool,

    /// 자동 탭 명명에 basename 을 파생할 params 키(매니페스트 `name_from_param`).
    /// `Some("file")` 이면 params 의 `file` 값 basename 을 표시명으로 쓴다. `None` 이면
    /// 파생 없이 kind 의 표시명 fallback. 본체의 `kind == "markdown"` basename 명명
    /// 하드코딩을 generic 화한다. builtin 은 등록 코드에서, plugin kind 는 decl 에서 채운다.
    pub name_from_param: Option<String>,

    /// 파일 기반 kind 를 파일 핸들러로 열 때 확인 팝업을 띄우는 크기 임계값(bytes,
    /// 매니페스트 `size_confirm_limit`). `Some(n)` 이면 파일이 n bytes 를 *초과* 할 때
    /// 즉시 열지 않고 확인 팝업을 띄운다. `None` 이면 게이트 없음(항상 즉시 열기). 본체의
    /// `kind == "markdown"` 1MB 게이트 하드코딩을 generic 화한다. builtin 은 등록 코드에서,
    /// plugin kind 는 decl 에서 채운다.
    pub size_confirm_limit: Option<u64>,
}

impl SurfaceKindDef {
    /// 이 kind 의 surface 생성에 반드시 필요한 params 키 목록.
    ///
    /// `preset_fields[].required=true` 가 단일 진실원(매니페스트 `validate.rs` 가
    /// 강제)이며, 값이 params 로 흐르는 필드(target=Params)만 해당한다. terminal 의
    /// cwd/startup 처럼 전용 컬럼(Cwd/Startup)으로 라우팅되는 필드는 params 키가
    /// 아니므로 제외된다.
    pub fn required_params(&self) -> impl Iterator<Item = &str> {
        self.preset_fields.iter().filter_map(|f| match &f.target {
            PresetFieldTarget::Params(key) if f.required => Some(key.as_str()),
            _ => None,
        })
    }

    /// `params` 에서 값이 비었거나 없는 첫 required param 키를 반환. 없으면 `None`.
    /// surface 생성 IPC 핸들러가 명확한 에러 메시지를 위해 선검증할 때 사용한다.
    pub fn first_missing_required_param(&self, params: &serde_json::Value) -> Option<&str> {
        self.required_params().find(|key| {
            params
                .get(*key)
                .and_then(|v| v.as_str())
                .map(str::is_empty)
                .unwrap_or(true)
        })
    }

    /// `params` 의 옛 키를 canonical 키로 정규화한다(alias 매핑). canonical 키가 이미
    /// 있으면 alias 는 무시(명시 우선). object 가 아니거나 alias 가 없으면 no-op.
    pub fn normalize_param_aliases(&self, params: &mut serde_json::Value) {
        if self.param_aliases.is_empty() {
            return;
        }
        let Some(obj) = params.as_object_mut() else {
            return;
        };
        for (alias, canonical) in &self.param_aliases {
            if !obj.contains_key(canonical.as_str())
                && let Some(v) = obj.remove(alias)
            {
                obj.insert(canonical.clone(), v);
            }
        }
    }
}

/// surface 종류 lookup 테이블. `Arc<SurfaceKindRegistry>` 단위로 CoreState에 보관되어
/// 매 프레임 dispatch에 사용된다.
///
/// 내부적으로 `RwLock`을 사용하여 plugin이 부팅 후 동적으로 kind를 등록할 수 있게
/// 한다. Builtin은 부팅 시 한 번 register되고 read만 일어나는 hot path는 read-lock
/// 한 번이므로 사실상 lock-free에 가깝다.
#[derive(Default)]
pub struct SurfaceKindRegistry {
    kinds: RwLock<HashMap<&'static str, Arc<SurfaceKindDef>>>,
}

impl SurfaceKindRegistry {
    pub fn new() -> Self {
        Self {
            kinds: RwLock::new(HashMap::new()),
        }
    }

    /// `&self`만 받으므로 `Arc<SurfaceKindRegistry>` 너머에서도 호출 가능.
    /// plugin 매니저가 hello 받은 후 호출.
    pub fn register(&self, def: SurfaceKindDef) {
        let kind = def.kind;
        let mut map = match self.kinds.write() {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("SurfaceKindRegistry write lock poisoned: {e}");
                return;
            }
        };
        if map.insert(kind, Arc::new(def)).is_some() {
            tracing::warn!("SurfaceKindRegistry: kind '{}' overwritten", kind);
        }
    }

    pub fn get(&self, kind: &str) -> Option<Arc<SurfaceKindDef>> {
        self.kinds.read().ok()?.get(kind).cloned()
    }

    pub fn contains(&self, kind: &str) -> bool {
        self.kinds
            .read()
            .map(|m| m.contains_key(kind))
            .unwrap_or(false)
    }

    /// 등록된 kind 목록을 스냅샷으로 반환 (lock 해제 후 안전히 사용).
    pub fn kinds_snapshot(&self) -> Vec<(&'static str, Arc<SurfaceKindDef>)> {
        self.kinds
            .read()
            .map(|m| m.iter().map(|(k, v)| (*k, v.clone())).collect())
            .unwrap_or_default()
    }

    pub fn len(&self) -> usize {
        self.kinds.read().map(|m| m.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.kinds.read().map(|m| m.is_empty()).unwrap_or(true)
    }
}

impl tasty_plugin_protocol::host_port::SurfaceRegistry for SurfaceKindRegistry {
    fn contains(&self, kind: &str) -> bool {
        SurfaceKindRegistry::contains(self, kind)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_def(kind: &'static str) -> SurfaceKindDef {
        SurfaceKindDef {
            kind,
            display_name_i18n_key: "test.dummy",
            icon: None,
            create: Arc::new(|_, _, _| Err(anyhow::anyhow!("dummy"))),
            restore: Arc::new(|_, _| Err(anyhow::anyhow!("dummy"))),
            snapshot: Arc::new(|_| None),
            preset_fields: Vec::new(),
            param_aliases: HashMap::new(),
            default_params: HashMap::new(),
            consumes_egui_input: false,
            zoomable: false,
            egui_copy: false,
            copy_path: false,
            egui_paste: false,
            name_from_param: None,
            size_confirm_limit: None,
        }
    }

    #[test]
    fn register_and_lookup() {
        let reg = SurfaceKindRegistry::new();
        reg.register(dummy_def("alpha"));
        reg.register(dummy_def("beta"));
        assert!(reg.contains("alpha"));
        assert!(reg.contains("beta"));
        assert!(!reg.contains("gamma"));
        assert_eq!(reg.get("alpha").unwrap().kind, "alpha");
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn duplicate_register_overwrites() {
        let reg = SurfaceKindRegistry::new();
        reg.register(dummy_def("x"));
        reg.register(dummy_def("x"));
        assert!(reg.contains("x"));
        assert_eq!(reg.len(), 1);
    }

    /// required_params 는 preset_fields 의 required=true + target=Params 만 노출한다.
    fn def_with_file_required(kind: &'static str) -> SurfaceKindDef {
        let mut d = dummy_def(kind);
        d.preset_fields = vec![PresetFieldSpec {
            id: "file".to_string(),
            label_key: "preset.field.file".to_string(),
            target: PresetFieldTarget::Params("file".to_string()),
            input: PresetFieldInput::FilePath,
            required: true,
            placeholder_key: None,
            default: None,
            derive_cwd: true,
        }];
        d
    }

    #[test]
    fn required_params_derived_from_preset_fields() {
        let d = def_with_file_required("markdown");
        assert_eq!(d.required_params().collect::<Vec<_>>(), vec!["file"]);
        // cwd/startup (target=Cwd/Startup) 는 params 키가 아니라 required_params 제외.
        let reg = SurfaceKindRegistry::new();
        register_builtin_kinds(&reg);
        let term = reg.get("terminal").unwrap();
        assert_eq!(term.required_params().count(), 0);
    }

    #[test]
    fn first_missing_required_param_detects_absent_and_empty() {
        let d = def_with_file_required("markdown");
        assert_eq!(
            d.first_missing_required_param(&serde_json::json!({})),
            Some("file")
        );
        assert_eq!(
            d.first_missing_required_param(&serde_json::json!({"file": ""})),
            Some("file")
        );
        assert_eq!(
            d.first_missing_required_param(&serde_json::json!({"file": "/a/b.md"})),
            None
        );
    }

    #[test]
    fn normalize_param_aliases_moves_old_key() {
        let mut d = dummy_def("markdown");
        d.param_aliases = HashMap::from([("file_path".to_string(), "file".to_string())]);
        // 옛 키 → canonical 로 이동.
        let mut p = serde_json::json!({"file_path": "/a/b.md"});
        d.normalize_param_aliases(&mut p);
        assert_eq!(p, serde_json::json!({"file": "/a/b.md"}));
        // canonical 키가 이미 있으면 alias 무시(명시 우선). alias 키는 그대로 남되
        // 다운스트림 create 가 canonical 만 읽으므로 무해(옛 동작과 동일).
        let mut p2 = serde_json::json!({"file": "/keep.md", "file_path": "/drop.md"});
        d.normalize_param_aliases(&mut p2);
        assert_eq!(p2["file"], "/keep.md");
    }

    #[test]
    fn default_tab_name_uses_name_from_param() {
        let mut d = dummy_def("markdown");
        d.name_from_param = Some("file".to_string());
        // name_from_param 키가 params 에 있으면 basename.
        assert_eq!(
            crate::state::pane::default_tab_name_for_kind(
                "markdown",
                &serde_json::json!({"file": "/a/b/README.md"}),
                Some(&d),
            ),
            "README.md"
        );
        // 키가 없으면 display_name_i18n_key 번역(테스트: lang 미로드 → 키 그대로).
        assert_eq!(
            crate::state::pane::default_tab_name_for_kind(
                "markdown",
                &serde_json::json!({}),
                Some(&d),
            ),
            "test.dummy"
        );
        // name_from_param 미선언이면 파생 없이 fallback.
        let plain = dummy_def("empty");
        assert_eq!(
            crate::state::pane::default_tab_name_for_kind(
                "empty",
                &serde_json::json!({"file": "/x/y.md"}),
                Some(&plain),
            ),
            "test.dummy"
        );
        // def 미등록이면 kind 문자열 그대로(catch-all 보존).
        assert_eq!(
            crate::state::pane::default_tab_name_for_kind("plugin_x", &serde_json::json!({}), None),
            "plugin_x"
        );
    }

    #[test]
    fn builtin_registers_host_kinds() {
        let reg = SurfaceKindRegistry::new();
        register_builtin_kinds(&reg);
        // image는 com.tasty.image plugin이, markdown은 com.tasty.markdown plugin이
        // 각각 hello 시 egui-mesh whitelist 경유로 등록한다. explorer는 T11에서
        // host builtin surface로 승격되어 부팅 시 직접 등록된다.
        for kind in ["terminal", "empty", "attached", "explorer"] {
            assert!(reg.contains(kind), "missing builtin kind: {kind}");
        }
        assert_eq!(reg.len(), 4);
        assert!(!reg.contains("image"));
        assert!(!reg.contains("markdown"));
        assert!(!reg.contains("clipboard_viewer"));
    }
}
pub mod egui_mesh;
pub mod webview_kind;
