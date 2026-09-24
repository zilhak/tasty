//! 호스트의 surface 종류를 조회하는 trait.
//! 플러그인 관리자는 구체적인 SurfaceKindRegistry 구현 대신 이 trait를 사용한다.

pub trait SurfaceRegistry: Send + Sync {
    /// 등록된 surface kind 인지 확인. plugin manager 가 활성화 가능 여부 검증에 사용.
    fn contains(&self, kind: &str) -> bool;
}
