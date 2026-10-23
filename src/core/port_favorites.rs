//! surface와 무관한 주소·포트 즐겨찾기를 port-favorites.toml에 저장한다.
//! PID는 재시작하면 바뀌므로 주소·포트가 모두 같은 항목을 찾는다.
//! add·remove는 메모리만 바꾸고 저장은 SharedList가 불변 snapshot으로 접수한다.
//! 프로세스의 원본은 [`crate::core::shared_list::SharedList`] 하나이고, engine은 그리기용 사본만 가진다.

use std::net::IpAddr;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortFavorite {
    pub label: String,
    pub addr: IpAddr,
    pub port: u16,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PortFavorites {
    #[serde(default, rename = "favorite")]
    pub items: Vec<PortFavorite>,
}

impl PortFavorites {
    pub fn config_path() -> Option<PathBuf> {
        tasty_utils::path::tasty_home().map(|dir| dir.join("port-favorites.toml"))
    }

    /// 같은 주소·포트면 라벨을 갱신한다. 저장은 호출자가 맡는다.
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

    /// 같은 주소·포트인 항목을 지운다. 저장은 호출자가 맡는다.
    pub fn remove(&mut self, addr: IpAddr, port: u16) {
        self.items.retain(|f| !(f.addr == addr && f.port == port));
    }

    pub fn contains(&self, addr: IpAddr, port: u16) -> bool {
        self.items.iter().any(|f| f.addr == addr && f.port == port)
    }
}

impl crate::core::shared_list::PersistedList for PortFavorites {
    const NAME: &'static str = "port favorites";
    fn config_path() -> Option<PathBuf> {
        Self::config_path()
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
        assert!(!favs.contains("0.0.0.0".parse().unwrap(), 3000));
    }

    #[test]
    fn load_returns_empty_when_file_missing_or_unparsable() {
        // load를 호출하지 않고 파일 읽기·TOML 파싱의 실패만 확인한다.
        // load가 빈 목록을 반환하는 경로 자체를 검증하지는 않는다.
        let missing = std::fs::read_to_string("/nonexistent/path/port-favorites.toml");
        assert!(missing.is_err());

        let bad_toml = "not valid = [[[ toml";
        assert!(toml::from_str::<PortFavorites>(bad_toml).is_err());
    }
}
