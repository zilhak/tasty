//! surface와 무관한 즐겨찾기 목록을 explorer-favorites.toml에 저장한다.
//! add·remove는 메모리만 바꾸며 저장은 호출자가 save로 요청한다. layout snapshot에는 포함하지 않는다.
//! 프로세스의 원본은 [`SharedExplorerFavorites`] 하나이고, engine은 그리기용 사본만 가진다.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplorerFavorite {
    /// add에서 빈 라벨은 파일명 또는 전체 경로로 대체한다.
    pub label: String,
    pub path: PathBuf,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExplorerFavorites {
    #[serde(default, rename = "favorite")]
    pub items: Vec<ExplorerFavorite>,
}

impl ExplorerFavorites {
    pub fn config_path() -> Option<PathBuf> {
        tasty_utils::path::tasty_home().map(|dir| dir.join("explorer-favorites.toml"))
    }

    /// 읽기나 파싱에 실패하면 빈 목록을 반환한다.
    pub fn load() -> Self {
        let Some(path) = Self::config_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(contents) => match toml::from_str::<Self>(&contents) {
                Ok(favs) => favs,
                Err(e) => {
                    tracing::warn!("explorer: failed to parse favorites ({e}), using empty");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    pub fn save(&mut self) {
        let Some(path) = Self::config_path() else {
            tracing::warn!("explorer: no favorites path available; not saving");
            return;
        };
        if let Some(contents) = Self::serialize(self) {
            Self::persist_to_disk(&path, &contents);
        }
    }

    fn serialize(&self) -> Option<String> {
        match toml::to_string_pretty(self) {
            Ok(contents) => Some(contents),
            Err(e) => {
                tracing::warn!("explorer: failed to serialize favorites: {e}");
                None
            }
        }
    }

    fn persist_to_disk(path: &Path, contents: &str) {
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!("explorer: failed to create favorites dir: {e}");
            return;
        }
        if let Err(e) = std::fs::write(path, contents) {
            tracing::warn!("explorer: failed to write favorites: {e}");
        }
    }

    /// 같은 경로가 있으면 라벨을 바꾼다. 비어 있는 라벨은 경로에서 만들며 저장은 호출자가 맡는다.
    pub fn add(&mut self, path: PathBuf, label: String) {
        let label = if label.trim().is_empty() {
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned())
        } else {
            label.trim().to_string()
        };
        if let Some(existing) = self.items.iter_mut().find(|f| f.path == path) {
            existing.label = label;
        } else {
            self.items.push(ExplorerFavorite { label, path });
        }
    }

    /// 해당 경로를 목록에서 지운다. 저장은 호출자가 맡는다.
    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|f| f.path != path);
    }
}

/// 프로세스의 모든 engine이 함께 쓰는 즐겨찾기 원본. engine마다 사본 전체로 파일을 덮으면 다른 윈도우의
/// 변경을 지우므로, 변경은 이 원본에만 하고 저장도 원본으로 한다. engine 사본은 리비전이 바뀌면 다시 복사한다.
#[derive(Default)]
pub(crate) struct SharedExplorerFavorites {
    inner: std::sync::Mutex<SharedState>,
}

#[derive(Default)]
struct SharedState {
    revision: u64,
    favorites: ExplorerFavorites,
}

impl SharedExplorerFavorites {
    pub(crate) fn load() -> Self {
        Self::from(ExplorerFavorites::load())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, SharedState> {
        // 변경은 목록 연산과 파일 쓰기뿐이라 중간에 멈춰도 목록 자체는 쓸 수 있다.
        self.inner.lock().unwrap_or_else(|poisoned| {
            tracing::warn!("explorer: favorites lock was poisoned; using the last list");
            poisoned.into_inner()
        })
    }

    /// 원본을 바꾸고 파일에 저장한 뒤 리비전을 올린다.
    pub(crate) fn update(&self, change: impl FnOnce(&mut ExplorerFavorites)) {
        let mut state = self.lock();
        change(&mut state.favorites);
        state.favorites.save();
        state.revision += 1;
    }

    pub(crate) fn revision(&self) -> u64 {
        self.lock().revision
    }

    /// 현재 리비전과 목록 사본.
    pub(crate) fn copy(&self) -> (u64, ExplorerFavorites) {
        let state = self.lock();
        (state.revision, state.favorites.clone())
    }

    /// `known` 리비전 뒤에 바뀌었으면 현재 리비전과 목록 사본을 돌려준다.
    pub(crate) fn copy_if_newer(&self, known: u64) -> Option<(u64, ExplorerFavorites)> {
        let state = self.lock();
        (known != state.revision).then(|| (state.revision, state.favorites.clone()))
    }
}

impl From<ExplorerFavorites> for SharedExplorerFavorites {
    fn from(favorites: ExplorerFavorites) -> Self {
        Self {
            inner: std::sync::Mutex::new(SharedState {
                revision: 0,
                favorites,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dedups_by_path_and_updates_label() {
        let mut favs = ExplorerFavorites::default();
        favs.items.push(ExplorerFavorite {
            label: "old".into(),
            path: PathBuf::from("/tmp/a"),
        });
        favs.add(PathBuf::from("/tmp/a"), "new".into());
        assert_eq!(favs.items.len(), 1);
        assert_eq!(favs.items[0].label, "new");
    }

    #[test]
    fn add_empty_label_falls_back_to_filename() {
        let mut favs = ExplorerFavorites::default();
        favs.add(PathBuf::from("/tmp/Downloads"), "  ".into());
        assert_eq!(favs.items[0].label, "Downloads");
    }

    #[test]
    fn remove_by_path() {
        let mut favs = ExplorerFavorites::default();
        favs.items.push(ExplorerFavorite {
            label: "a".into(),
            path: PathBuf::from("/tmp/a"),
        });
        favs.remove(Path::new("/tmp/a"));
        assert!(favs.items.is_empty());
    }
}
