//! Explorer 즐겨찾기(favorites) — 전역(글로벌) 영속 저장소.
//!
//! 사용자가 우클릭 "Add to favorites" 로 등록한 폴더/경로 목록. surface 와 무관한
//! 전역 상태라 `~/.tasty/explorer-favorites.toml` 한 곳에 보관하고, 부팅 시
//! `CoreState` 가 `load()` 로 읽어 메모리에 들고 다닌다. 추가/삭제 mutator 는
//! 즉시 `save()` 로 디스크에 반영한다(세션 휘발 아님 — design §3.5 "Favorites are
//! global").
//!
//! 사용자 직접 조작(우클릭 메뉴)으로만 변경되므로 release 경로에서 직접 갱신한다
//! (도메인 snapshot/layout.json 비대상).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 즐겨찾기 한 항목 — 표시 라벨 + 대상 경로.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExplorerFavorite {
    /// 사이드바에 표시할 이름. 비면 경로의 마지막 컴포넌트로 대체.
    pub label: String,
    /// 즐겨찾기 대상 경로(보통 디렉토리).
    pub path: PathBuf,
}

/// 즐겨찾기 전체 목록(영속 단위).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ExplorerFavorites {
    /// TOML 에서는 `[[favorite]]` 배열로 직렬화된다.
    #[serde(default, rename = "favorite")]
    pub items: Vec<ExplorerFavorite>,
}

impl ExplorerFavorites {
    /// 저장 파일 경로: `~/.tasty/explorer-favorites.toml`.
    pub fn config_path() -> Option<PathBuf> {
        tasty_utils::path::tasty_home().map(|dir| dir.join("explorer-favorites.toml"))
    }

    /// 디스크에서 읽는다. 파일이 없거나 파싱 실패 시 빈 목록.
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

    /// 디스크에 기록한다.
    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            tracing::warn!("explorer: no favorites path available; not saving");
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!("explorer: failed to create favorites dir: {e}");
            return;
        }
        match toml::to_string_pretty(self) {
            Ok(contents) => {
                if let Err(e) = std::fs::write(&path, contents) {
                    tracing::warn!("explorer: failed to write favorites: {e}");
                }
            }
            Err(e) => tracing::warn!("explorer: failed to serialize favorites: {e}"),
        }
    }

    /// 추가(같은 경로가 있으면 라벨만 갱신). 라벨이 비면 경로 마지막 컴포넌트로
    /// 대체한다. 디스크 반영은 호출처가 [`save`](Self::save) 로 한다(메모리 mutator
    /// 는 순수 — 테스트가 디스크를 건드리지 않게 분리).
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

    /// 경로로 삭제. 디스크 반영은 호출처가 [`save`](Self::save) 로 한다.
    pub fn remove(&mut self, path: &Path) {
        self.items.retain(|f| f.path != path);
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
        // 같은 경로 재추가 → 라벨만 갱신, 항목 수 유지.
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
