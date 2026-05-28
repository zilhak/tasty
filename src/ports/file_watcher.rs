//! FileWatcher port — 파일 변경 감지 (global hook 의 `file:` 조건).

use std::path::Path;

#[allow(dead_code)]
pub trait FileWatcher: Send + Sync {
    /// 등록된 watch 의 식별자. unwatch 시 사용.
    type Handle: Send + Sync;

    /// `path` 변경 시 callback 호출. callback 은 *별 스레드* 가능 — `Send + Sync`.
    fn watch(
        &self,
        path: &Path,
        callback: Box<dyn Fn() + Send + Sync>,
    ) -> anyhow::Result<Self::Handle>;

    fn unwatch(&self, handle: Self::Handle);
}
