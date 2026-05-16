//! Surface 종류별 메타·동작 정의 레지스트리.
//!
//! 본체 4종(Terminal/Markdown/Html/Empty)이 부팅 시 등록된다. Explorer/Image/ClipboardHistory
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

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tasty_core::model::{Surface, SurfaceId};

pub use builtins::register_builtin_kinds;

/// `ClosedPanel::from_*` 가족이 받는 snapshot 클로저를 registry 위에서 만든다.
/// 호출자는 결과를 `&mut`로 가지고 한 capture 트랜잭션 동안 reuse한다.
pub fn snapshot_fn_for(
    registry: &SurfaceKindRegistry,
) -> impl FnMut(&dyn Surface) -> Option<serde_json::Value> + '_ {
    move |s| registry.get(s.kind()).and_then(|def| (def.snapshot)(s))
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

    /// 새 surface 인스턴스를 만든다. 03F에서 IPC handler / `add_kind_tab` /
    /// `split_pane_targeted` 가 이 함수를 호출하여 종류별 분기를 일원화한다.
    ///
    /// `params`는 IPC/CLI에서 받은 JSON. 종류별로 필요한 키가 다르다 (예: markdown은 `"file"`,
    /// html은 `"url"`).
    #[allow(clippy::type_complexity)]
    pub create: Arc<
        dyn Fn(SurfaceId, &serde_json::Value) -> anyhow::Result<Box<dyn Surface>>
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
        dyn Fn(SurfaceId, &serde_json::Value) -> anyhow::Result<Box<dyn Surface>>
            + Send
            + Sync,
    >,

    /// surface의 직렬화 가능한 영속 데이터를 반환한다. `None`이면 영속화에서 제외.
    /// 03E의 `SavedSurface::capture_surface`가 호출한다. 휘발성 surface는 `None`을
    /// 반환하여 layout 저장에서 빠진다.
    pub snapshot: Arc<dyn Fn(&dyn Surface) -> Option<serde_json::Value> + Send + Sync>,
}

/// surface 종류 lookup 테이블. `Arc<SurfaceKindRegistry>` 단위로 EngineState에 보관되어
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

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_def(kind: &'static str) -> SurfaceKindDef {
        SurfaceKindDef {
            kind,
            display_name_i18n_key: "test.dummy",
            create: Arc::new(|_, _| Err(anyhow::anyhow!("dummy"))),
            restore: Arc::new(|_, _| Err(anyhow::anyhow!("dummy"))),
            snapshot: Arc::new(|_| None),
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

    #[test]
    fn builtin_registers_host_kinds() {
        let reg = SurfaceKindRegistry::new();
        register_builtin_kinds(&reg);
        // explorer는 com.tasty.explorer plugin이, image는 com.tasty.image plugin이
        // 각각 hello 시에 등록한다. diff 는 host 빌트인.
        for kind in ["terminal", "markdown", "html", "empty", "diff"] {
            assert!(reg.contains(kind), "missing builtin kind: {kind}");
        }
        assert_eq!(reg.len(), 5);
        assert!(!reg.contains("image"));
        assert!(!reg.contains("explorer"));
        assert!(!reg.contains("clipboard_viewer"));
    }
}
