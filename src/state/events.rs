//! `AppState` 가 주고받는 창 쪽 타입 — 지금은 키보드 라우팅용 [`FocusedSurfaceType`] 하나다.
//! surface 간 메시지(`SurfaceMessage`)는 그것을 담는 `CoreState` 쪽(`core/state/message.rs`)에,
//! 호스트 이벤트 큐 항목(`PendingHostEvent` · `PendingSurfaceClosed`)은 그것을 세우는 도메인
//! cascade 쪽(`core/host_event.rs`)에 있다.
//!
//! `state.rs` 에서 그대로 옮겨 온 것이다 — 동작 변경이 없다. 이 자리를 고른 이유는
//! 크기가 아니라 **방향**이다: 이 타입들은 `state.rs` 의 다른 타입을 하나도 안 들고
//! (`AppState` 도 안 든다), `cfg(feature)` 도 안 쓴다. 그래서 의존이 한 방향으로만
//! 흐르고, 이 분리는 되돌릴 수 있으며 다음 분리의 전제를 만들지 않는다.
//!
//! 이름은 부모가 재수출한다(`state.rs` 의 `pub use events::{…}`). glob 을 안 쓰는 이유는
//! 여기에 타입을 하나 더 넣었을 때 **공개면이 조용히 자라지 않게** 하려는 것이다 —
//! 이 레포가 조용한 증가를 여러 게이트로 막는 것과 같은 취향이다.

use crate::core::CoreState;

/// Type of the currently focused surface, used for keyboard routing.
///
/// Terminal은 PTY 입출력 경로가 별도라 빠른 분기 위해 전용 variant로 둔다.
/// 나머지는 surface kind 식별자 기반의 `Kind(String)`으로 일반화 — 외부 plugin도
/// 추가 enum 변경 없이 동작한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusedSurfaceType {
    None,
    Terminal,
    Kind(String),
}

impl FocusedSurfaceType {
    /// 이 surface가 주어진 kind 식별자에 해당하는지 검사.
    pub fn is_kind(&self, kind: &str) -> bool {
        matches!(self, Self::Kind(k) if k == kind)
    }

    /// registry 에서 이 surface kind 의 capability flag 를 조회한다. `Terminal`/`None`
    /// 은 kind 문자열이 아니므로 항상 false. host 가 `kind == "..."` 하드코딩 대신
    /// plugin/builtin 이 선언한 capability 로 게이트를 판정하게 한다.
    pub fn kind_capability(
        &self,
        engine: &CoreState,
        f: impl Fn(&crate::core::surface_registry::SurfaceKindDef) -> bool,
    ) -> bool {
        match self {
            Self::Kind(k) => engine
                .surface_registry
                .get(k)
                .map(|d| f(&d))
                .unwrap_or(false),
            _ => false,
        }
    }
}
