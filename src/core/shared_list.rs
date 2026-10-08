//! 윈도우(engine)마다 사본을 두면 저장할 때 다른 윈도우의 변경을 지우는 프로세스 공용 목록.
//! 원본은 [`SharedList`] 하나이고 engine은 그리기용 [`Replica`]만 가진다. 변경은 원본에만 하고
//! 저장도 원본 전체로 한다. 사본은 원본 리비전이 바뀌면 다시 복사한다.

use std::sync::{Arc, Mutex, MutexGuard};

/// 파일 하나에 통째로 저장하는 목록. `load`는 읽기 실패를 빈 목록으로 처리한다.
pub(crate) trait PersistedList: Clone + Default {
    /// 경고 로그에 쓰는 이름.
    const NAME: &'static str;
    fn load() -> Self;
    fn save(&mut self);
}

#[derive(Default)]
pub(crate) struct SharedList<T> {
    inner: Mutex<SharedState<T>>,
}

#[derive(Default)]
struct SharedState<T> {
    revision: u64,
    list: T,
}

impl<T: PersistedList> SharedList<T> {
    pub(crate) fn load() -> Self {
        Self::from(T::load())
    }

    fn lock(&self) -> MutexGuard<'_, SharedState<T>> {
        // 변경은 목록 연산과 파일 쓰기뿐이라 중간에 멈춰도 목록 자체는 쓸 수 있다.
        self.inner.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("{}: lock was poisoned; using the last list", T::NAME);
            poisoned.into_inner()
        })
    }

    /// 원본을 바꾸고 파일에 저장한 뒤 리비전을 올린다.
    pub(crate) fn update(&self, change: impl FnOnce(&mut T)) {
        let mut state = self.lock();
        change(&mut state.list);
        state.list.save();
        state.revision += 1;
    }

    pub(crate) fn revision(&self) -> u64 {
        self.lock().revision
    }

    /// 현재 리비전과 목록 사본.
    pub(crate) fn copy(&self) -> (u64, T) {
        let state = self.lock();
        (state.revision, state.list.clone())
    }

    /// `known` 리비전 뒤에 바뀌었으면 현재 리비전과 목록 사본을 돌려준다.
    fn copy_if_newer(&self, known: u64) -> Option<(u64, T)> {
        let state = self.lock();
        (known != state.revision).then(|| (state.revision, state.list.clone()))
    }
}

impl<T> From<T> for SharedList<T> {
    fn from(list: T) -> Self {
        Self {
            inner: Mutex::new(SharedState { revision: 0, list }),
        }
    }
}

/// engine이 그리기에 쓰는 사본. 읽기만 `Deref`로 열고 변경은 [`Replica::change`]로 원본에 한다.
pub(crate) struct Replica<T> {
    list: T,
    revision: u64,
    source: Arc<SharedList<T>>,
}

impl<T: PersistedList> Replica<T> {
    pub(crate) fn new(source: Arc<SharedList<T>>) -> Self {
        let (revision, list) = source.copy();
        Self {
            list,
            revision,
            source,
        }
    }

    /// 원본을 바꾸고 이 사본을 맞춘다. 다른 engine의 사본은 다음 그리기 전에 맞춘다.
    pub(crate) fn change(&mut self, change: impl FnOnce(&mut T)) {
        self.source.update(change);
        self.sync();
    }

    /// 원본이 이 사본 뒤에 바뀌었으면 다시 복사하고 true를 돌려준다.
    pub(crate) fn sync(&mut self) -> bool {
        let Some((revision, list)) = self.source.copy_if_newer(self.revision) else {
            return false;
        };
        self.list = list;
        self.revision = revision;
        true
    }
}

impl<T> std::ops::Deref for Replica<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.list
    }
}
