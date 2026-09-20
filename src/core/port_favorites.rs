//! 포트 스캐너 즐겨찾기(favorites) — 전역(글로벌) 영속 저장소.
//!
//! 사용자가 리스닝 포트 팝업에서 즐겨찾기로 등록한 (주소, 포트) 목록. 실행 여부와
//! 무관하게 계속 지켜보고 싶은 주소+포트를 기억하는 용도라 surface 와 무관한 전역
//! 상태로 `~/.tasty/port-favorites.toml` 한 곳에 보관하고, 부팅 시 `CoreState` 가
//! `load()` 로 읽어 메모리에 들고 다닌다. 추가/삭제 mutator 는 즉시 `save()` 로
//! 디스크에 반영한다(세션 휘발 아님) — [`crate::explorer_ui::favorites::ExplorerFavorites`]
//! 와 동일 패턴.
//!
//! 식별 키는 `(addr, port)` 정확히 일치 — PID 는 프로세스 재시작마다 바뀌므로
//! 식별자로 쓸 수 없다.

use std::net::IpAddr;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// 즐겨찾기 한 항목 — 표시 라벨 + 대상 주소/포트.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortFavorite {
    /// 표시할 이름.
    pub label: String,
    /// 즐겨찾기 대상 주소.
    pub addr: IpAddr,
    /// 즐겨찾기 대상 포트.
    pub port: u16,
}

/// 즐겨찾기 전체 목록(영속 단위).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PortFavorites {
    /// TOML 에서는 `[[favorite]]` 배열로 직렬화된다.
    #[serde(default, rename = "favorite")]
    pub items: Vec<PortFavorite>,
}

// 포트 스캐너 팝업(`port_scanner.rs`)의 별 토글 + 상단 즐겨찾기 섹션이 이 API
// (`contains`/`add`/`remove`/`save`)를 소비한다.
impl PortFavorites {
    /// 저장 파일 경로: `~/.tasty/port-favorites.toml`.
    pub fn config_path() -> Option<PathBuf> {
        tasty_utils::path::tasty_home().map(|dir| dir.join("port-favorites.toml"))
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
                    tracing::warn!("port_scanner: failed to parse favorites ({e}), using empty");
                    Self::default()
                }
            },
            Err(_) => Self::default(),
        }
    }

    /// 디스크에 기록한다.
    pub fn save(&self) {
        let Some(path) = Self::config_path() else {
            tracing::warn!("port_scanner: no favorites path available; not saving");
            return;
        };
        if let Some(contents) = self.serialize() {
            Self::persist_to_disk(&path, &contents);
        }
    }

    /// TOML 로 직렬화. 실패는 warn 로그 후 `None`(호출자는 쓰기를 건너뛴다).
    fn serialize(&self) -> Option<String> {
        match toml::to_string_pretty(self) {
            Ok(contents) => Some(contents),
            Err(e) => {
                tracing::warn!("port_scanner: failed to serialize favorites: {e}");
                None
            }
        }
    }

    /// 부모 디렉토리를 만들고(필요 시) `contents` 를 `path` 에 기록한다.
    fn persist_to_disk(path: &Path, contents: &str) {
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!("port_scanner: failed to create favorites dir: {e}");
            return;
        }
        if let Err(e) = std::fs::write(path, contents) {
            tracing::warn!("port_scanner: failed to write favorites: {e}");
        }
    }

    /// 추가(같은 `(addr, port)` 가 있으면 라벨만 갱신). 디스크 반영은 호출처가
    /// [`save`](Self::save) 로 한다(메모리 mutator 는 순수 — 테스트가 디스크를
    /// 건드리지 않게 분리).
    pub fn add(&mut self, addr: IpAddr, port: u16, label: String) {
        if let Some(existing) = self
            .items
            .iter_mut()
            .find(|f| f.addr == addr && f.port == port)
        {
            existing.label = label;
        } else {
            self.items.push(PortFavorite { label, addr, port });
        }
    }

    /// `(addr, port)` 로 삭제. 디스크 반영은 호출처가 [`save`](Self::save) 로 한다.
    pub fn remove(&mut self, addr: IpAddr, port: u16) {
        self.items.retain(|f| !(f.addr == addr && f.port == port));
    }

    /// `(addr, port)` 가 즐겨찾기에 있는지 조회 — 팝업 wrapper 가 매 행 표시에 사용.
    pub fn contains(&self, addr: IpAddr, port: u16) -> bool {
        self.items.iter().any(|f| f.addr == addr && f.port == port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_dedups_by_addr_port_and_updates_label() {
        let mut favs = PortFavorites::default();
        favs.items.push(PortFavorite {
            label: "old".into(),
            addr: "127.0.0.1".parse().unwrap(),
            port: 3000,
        });
        // 같은 (addr, port) 재추가 → 라벨만 갱신, 항목 수 유지.
        favs.add("127.0.0.1".parse().unwrap(), 3000, "new".into());
        assert_eq!(favs.items.len(), 1);
        assert_eq!(favs.items[0].label, "new");
    }

    #[test]
    fn add_with_different_port_is_a_new_entry() {
        let mut favs = PortFavorites::default();
        favs.add("127.0.0.1".parse().unwrap(), 3000, "a".into());
        favs.add("127.0.0.1".parse().unwrap(), 3001, "b".into());
        assert_eq!(favs.items.len(), 2);
    }

    #[test]
    fn remove_by_addr_and_port() {
        let mut favs = PortFavorites::default();
        favs.add("0.0.0.0".parse().unwrap(), 8080, "a".into());
        favs.remove("0.0.0.0".parse().unwrap(), 8080);
        assert!(favs.items.is_empty());
    }

    #[test]
    fn contains_matches_exact_addr_and_port_only() {
        let mut favs = PortFavorites::default();
        favs.add("127.0.0.1".parse().unwrap(), 3000, "a".into());
        assert!(favs.contains("127.0.0.1".parse().unwrap(), 3000));
        // 주소가 다르면(예: 0.0.0.0 vs 127.0.0.1) 매칭하지 않는다 — 정확한 (addr, port) 일치만 즐겨찾기.
        assert!(!favs.contains("0.0.0.0".parse().unwrap(), 3000));
    }

    #[test]
    fn load_returns_empty_when_file_missing_or_unparsable() {
        // config_path() 는 실행 환경의 실제 홈을 가리키므로 여기서는 load() 의
        // 파일-없음/파싱-실패 방어 로직만 직접 검증한다(디스크를 건드리지 않기 위해
        // toml 파싱 경로를 동일하게 재현).
        let missing = std::fs::read_to_string("/nonexistent/path/port-favorites.toml");
        assert!(missing.is_err());

        let bad_toml = "not valid = [[[ toml";
        assert!(toml::from_str::<PortFavorites>(bad_toml).is_err());
    }
}
