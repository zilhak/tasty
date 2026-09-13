//! surface cwd 의 단일 판정과 **출처 구분**.
//!
//! 두 인스턴스(attach 서버와 client)의 파일시스템은 다르다. mirror 워크스페이스에 속한
//! surface 의 cwd 는 원격 호스트의 경로이므로, 로컬 PTY `working_dir` · 로컬 git 조회 ·
//! preset 같은 **로컬에서 실행되는** 자리에 들어가면 안 된다. 그 구분을 호출 규약이 아니라
//! 타입으로 둔다 — [`RemoteCwd`] 에는 `Path` 로 가는 변환이 없어서, 원격 값을 로컬 경로
//! 자리에 넣으려면 안의 문자열을 꺼내 손으로 감싸야 하고 그 순간이 코드에
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

    /// 표시·wire·원격 조회 요청에 싣는 문자열. 로컬 `Path` 로 감싸 쓰지 않는다.
    pub fn as_str(&self) -> &str {
        &self.0
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

    /// wire 로 내보낼 문자열. 출처 정보는 싣지 않는다 — 받는 쪽(attach client)에게 이 값은
    /// 출처와 무관하게 언제나 원격이다.
    fn into_wire(self) -> String {
        match self {
            SurfaceCwd::Local(p) => p.to_string_lossy().into_owned(),
            SurfaceCwd::Remote(r) => r.0,
        }
    }
}

impl CoreState {
    /// surface 의 cwd 를 출처와 함께 판정한다 — surface cwd 판정의 단일 진입점이다.
    ///
    /// 값 자체는 terminal 이면 store 의 `Terminal::get_cwd()`, 그 외 kind 는
    /// `Surface::source_cwd()` 로 구한다. 그 surface 가 mirror 워크스페이스에 속하면
    /// 서버가 push 한 값(`mirror_surface_cwd`)이 우선이고, 없으면(구버전 서버 · 첫 tick 전)
    /// 위 갈래로 내려간다. 어느 쪽이든(push · OSC 7 캐시 · explorer root) mirror 면
    /// [`SurfaceCwd::Remote`] 다.
    ///
    /// `inherit_cwd` 설정은 여기서 보지 않는다 — 그 설정은 "새 surface 가 상속하는가" 의
    /// 게이트라 소비 시점(`AppState::resolve_inherit_cwd*`)이 건다.
    pub(crate) fn surface_cwd(&self, surface_id: u32) -> Option<SurfaceCwd> {
        if let Some(pushed) = self.mirror_surface_cwd.get(&surface_id) {
            return Some(SurfaceCwd::Remote(pushed.clone()));
        }
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

    /// **mirror** surface 의 cwd 를 설정(또는 해제)한다 — 원격이 보낸
    /// `StreamControl::Cwd` 를 적용하는 유일한 writer 다(`app/attach_client.rs`).
    /// `None` 은 원격도 cwd 를 모르게 됐다는 뜻이라 엔트리를 지운다 — 남겨 두면 옛 원격
    /// 경로가 영구히 남는다.
    pub fn set_mirror_surface_cwd(&mut self, surface_id: u32, cwd: Option<String>) {
        match cwd {
            Some(path) => {
                self.mirror_surface_cwd
                    .insert(surface_id, RemoteCwd::new(path));
            }
            None => {
                self.mirror_surface_cwd.remove(&surface_id);
            }
        }
    }

    /// mirror surface 의 cwd 레코드를 버린다(surface 제거 · 워크스페이스/세션 teardown ·
    /// kind 전환). busy 와 달리 cwd 는 비-terminal kind 에도 있으므로 kind 전환 전부에서
    /// 부른다.
    pub fn forget_mirror_surface_cwd(&mut self, surface_id: u32) {
        self.mirror_surface_cwd.remove(&surface_id);
    }

    /// 점유 surface 의 cwd 변화분을 `(holder, surface, cwd)` 로 돌려준다 — attach stream 으로
    /// push 할 목록이다. 서버 **자기 트리** 기준으로 계산하고(`surface_cwd` — terminal 은
    /// OSC 7 이 없어도 pid 폴백이 돈다), `inherit_cwd` 설정은 보지 않는다(관측이지 실행이
    /// 아니다, ADR-0267 결정 5).
    ///
    /// diff 캐시 `last_forwarded_cwd` 는 **(holder, 값)** 을 함께 기억한다. 값만 기억하면
    /// 같은 tick 창 안에서 점유가 풀리고 다른 client 가 잡았을 때 엔트리가 `retain` 을
    /// 살아남아 새 holder 가 초기값을 못 받는다. 점유가 끊긴 surface 의 엔트리는 매 호출
    /// 지워지므로 재점유는 언제나 초기 push 를 받는다. 초기 push 는 값이 `None` 이어도
    /// 나간다 — 받는 쪽의 stale 값을 지우는 것도 그 프레임의 일이다.
    pub fn surface_cwd_forwards(
        &mut self,
    ) -> Vec<(crate::core::attach::AttachClientId, u32, Option<String>)> {
        let locks = self.attach.locks_snapshot();
        let occupied: std::collections::HashSet<u32> = locks.iter().map(|&(sid, _)| sid).collect();
        self.last_forwarded_cwd
            .retain(|sid, _| occupied.contains(sid));
        let mut out = Vec::new();
        for (sid, lock) in locks {
            let cwd = self.surface_cwd(sid).map(SurfaceCwd::into_wire);
            let record = (lock.holder, cwd);
            if self.last_forwarded_cwd.get(&sid) != Some(&record) {
                out.push((record.0, sid, record.1.clone()));
                self.last_forwarded_cwd.insert(sid, record);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::{RemoteCwd, SurfaceCwd};
    use crate::core::CoreState;

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    /// 점유된 첫 surface. 로컬 PTY 의 cwd 는 환경마다 다르므로 값 자체가 아니라 "처음 한 번
    /// 나가고, 안 바뀌면 안 나간다" 를 본다.
    #[test]
    fn cwd_forwards_only_on_change() {
        let mut e = engine();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock");

        let first = e.surface_cwd_forwards();
        assert_eq!(first.len(), 1, "최초 호출은 baseline 1 건");
        assert_eq!((first[0].0, first[0].1), (7, sid));
        assert_eq!(first[0].2, e.surface_cwd(sid).map(SurfaceCwd::into_wire));
        assert!(
            e.surface_cwd_forwards().is_empty(),
            "값이 그대로면 안 나간다"
        );
    }

    #[test]
    fn released_then_reacquired_pushes_baseline_again() {
        let mut e = engine();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock");
        assert_eq!(e.surface_cwd_forwards().len(), 1);

        e.attach.release(sid, 7).expect("release");
        assert!(e.surface_cwd_forwards().is_empty(), "점유 없음 → push 없음");
        assert!(
            e.last_forwarded_cwd.is_empty(),
            "점유 해제분은 캐시에서 빠진다"
        );

        e.attach.acquire(sid, 7).expect("re-lock");
        assert_eq!(
            e.surface_cwd_forwards().len(),
            1,
            "재점유는 초기 push 를 다시 받는다"
        );
    }

    /// 한 tick 창 안에서 holder 만 바뀐 경우 — 캐시가 값만 기억했다면 새 holder 는 아무것도
    /// 못 받는다.
    #[test]
    fn holder_swap_within_one_tick_pushes_to_the_new_holder() {
        let mut e = engine();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock");
        assert_eq!(e.surface_cwd_forwards().len(), 1);

        e.attach.release(sid, 7).expect("release");
        e.attach.acquire(sid, 8).expect("lock by another client");
        let swapped = e.surface_cwd_forwards();
        assert_eq!(swapped.len(), 1);
        assert_eq!((swapped[0].0, swapped[0].1), (8, sid));
    }

    #[test]
    fn mirror_push_overrides_and_none_clears() {
        let mut e = engine();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.workspaces[0].mirror = true;

        e.set_mirror_surface_cwd(sid, Some("/srv/remote".to_string()));
        assert_eq!(
            e.surface_cwd(sid),
            Some(SurfaceCwd::Remote(RemoteCwd::new("/srv/remote")))
        );
        assert_eq!(
            e.local_surface_cwd(sid),
            None,
            "원격 값은 로컬 출구로 안 나온다"
        );

        e.set_mirror_surface_cwd(sid, None);
        assert!(
            !e.mirror_surface_cwd.contains_key(&sid),
            "None push 는 값을 지운다"
        );
    }

    #[test]
    fn forget_drops_the_record() {
        let mut e = engine();
        e.set_mirror_surface_cwd(42, Some("/srv/remote".to_string()));
        e.forget_mirror_surface_cwd(42);
        assert!(e.mirror_surface_cwd.is_empty());
    }
}
