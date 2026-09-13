//! surface cwd 의 단일 판정과 **출처 구분**.
//!
//! 두 인스턴스(attach 서버와 client)의 파일시스템은 다르다. mirror 워크스페이스에 속한
//! surface 의 cwd 는 원격 호스트의 경로이므로, 로컬 PTY `working_dir` · 로컬 git 조회 ·
//! preset 같은 **로컬에서 실행되는** 자리에 들어가면 안 된다. 그 구분을 호출 규약이 아니라
//! 타입으로 둔다 — [`RemoteCwd`] 에는 `Path` 로 가는 변환이 없어서, 원격 값을 로컬 경로
//! 자리에 넣으려면 [`RemoteCwd::as_str`] 을 꺼내 손으로 감싸야 하고 그 순간이 코드에
//! 드러난다. 결정 근거: `docs/adr/0267-mirror-surface-cwd-is-pushed-by-the-server.md`.

use std::path::PathBuf;

use super::CoreState;

/// 원격 호스트 기준의 cwd 문자열. 표시·wire·원격 조회 앵커로만 쓴다.
///
/// `AsRef<Path>` / `Into<PathBuf>` 를 **일부러 두지 않는다** — 이 값이 로컬 파일시스템
/// 연산으로 흘러가는 경로를 컴파일러가 막게 하려는 것이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCwd(String);

impl RemoteCwd {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }
}

/// 출처가 구분된 surface cwd.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceCwd {
    /// 이 인스턴스의 파일시스템 경로.
    Local(PathBuf),
    /// mirror surface 의 원격 경로.
    Remote(RemoteCwd),
}

impl SurfaceCwd {
    /// 로컬 경로일 때만 꺼낸다. 원격 출처는 `None` — 로컬 실행 자리의 유일한 출구다.
    pub fn into_local(self) -> Option<PathBuf> {
        match self {
            SurfaceCwd::Local(p) => Some(p),
            SurfaceCwd::Remote(_) => None,
        }
    }
}

impl CoreState {
    /// surface 의 cwd 를 출처와 함께 판정한다 — surface cwd 판정의 단일 진입점이다.
    ///
    /// 값 자체는 terminal 이면 store 의 `Terminal::get_cwd()`, 그 외 kind 는
    /// `Surface::source_cwd()` 로 구한다. 그 surface 가 mirror 워크스페이스에 속하면
    /// 어느 갈래에서 나온 값이든(OSC 7 캐시 · explorer root) [`SurfaceCwd::Remote`] 다.
    ///
    /// `inherit_cwd` 설정은 여기서 보지 않는다 — 그 설정은 "새 surface 가 상속하는가" 의
    /// 게이트라 소비 시점(`AppState::resolve_inherit_cwd*`)이 건다.
    pub(crate) fn surface_cwd(&self, surface_id: u32) -> Option<SurfaceCwd> {
        let surface = self.find_surface_by_id(surface_id)?;
        let path = if surface.kind() == "terminal" {
            self.terminals.get(surface_id).and_then(|t| t.get_cwd())
        } else {
            surface.source_cwd()
        }?;
        if self.is_mirror_surface(surface_id) {
            Some(SurfaceCwd::Remote(RemoteCwd::new(
                path.to_string_lossy().into_owned(),
            )))
        } else {
            Some(SurfaceCwd::Local(path))
        }
    }

    /// [`Self::surface_cwd`] 중 로컬 출처만. 로컬 파일시스템을 건드리는 소비자용이다.
    pub(crate) fn local_surface_cwd(&self, surface_id: u32) -> Option<PathBuf> {
        self.surface_cwd(surface_id)
            .and_then(SurfaceCwd::into_local)
    }
}
