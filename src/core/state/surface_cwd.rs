//! 원격 cwd를 로컬 파일 작업에 잘못 쓰지 않도록 출처를 구분한다.

use std::path::PathBuf;

use super::CoreState;

/// 원격 호스트의 cwd. 표시·전송·원격 조회용이며 로컬 Path 변환을 제공하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCwd(String);

impl RemoteCwd {
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    #[cfg(any(feature = "gui", test))]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SurfaceCwd {
    Local(PathBuf),
    Remote(RemoteCwd),
}

impl SurfaceCwd {
    /// 로컬 경로만 반환한다. 원격이면 None이다.
    pub fn into_local(self) -> Option<PathBuf> {
        match self {
            SurfaceCwd::Local(p) => Some(p),
            SurfaceCwd::Remote(_) => None,
        }
    }

    /// 출처 표지 없이 전송한다. attach client에게는 서버의 경로다.
    fn into_wire(self) -> String {
        match self {
            SurfaceCwd::Local(p) => p.to_string_lossy().into_owned(),
            SurfaceCwd::Remote(r) => r.0,
        }
    }
}

impl CoreState {
    /// mirror push 값이 있으면 우선한다. 없으면 Terminal 또는 surface에서 찾는다.
    /// mirror 소속이면 대체 조회한 값도 Remote다. inherit_cwd 설정은 소비자가 적용한다.
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

    pub(crate) fn local_surface_cwd(&self, surface_id: u32) -> Option<PathBuf> {
        self.surface_cwd(surface_id)
            .and_then(SurfaceCwd::into_local)
    }

    /// 원격 push 캐시를 설정한다. None은 캐시를 지우지만 surface_cwd의 대체 조회까지 막지는 않는다.
    #[cfg(any(feature = "gui", test))]
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

    #[cfg(any(feature = "gui", test))]
    pub fn forget_mirror_surface_cwd(&mut self, surface_id: u32) {
        self.mirror_surface_cwd.remove(&surface_id);
    }

    /// 하드 점유 surface의 (holder, ID, cwd) 전송 후보. 최초 None도 포함한다.
    /// holder와 값으로 중복을 구분하고 점유가 끝난 캐시는 지운다.
    /// 전송 전에 캐시를 갱신하므로 실패 후 같은 값의 재전송은 보장하지 않는다.
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

    #[test]
    fn cwd_forwards_only_on_change() {
        let mut e = engine();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock");

        let first = e.surface_cwd_forwards();
        assert_eq!(first.len(), 1, "최초 호출은 전송 후보 1건");
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
            "재점유하면 초기 전송 후보를 다시 만든다"
        );
    }

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
            "원격 cwd는 로컬 경로로 반환하지 않는다"
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
