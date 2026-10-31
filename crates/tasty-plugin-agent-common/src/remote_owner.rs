//! client: 원격 자식의 결과 소유자.
//!
//! 서버는 자식마다 결과 경로를 하나만 두고, 턴이 끝날 때 그 경로의 마지막 route 토큰을 결과에 싣는다. 같은 client 의
//! 부모들은 서버에서 같은 출처라 서버는 부모를 구분하지 못한다. 그래서 한 부모가 지시한 턴이 진행되는 동안 다른 부모가
//! tell 하면 앞 턴의 결과가 뒤 부모에게 정상 결과처럼 간다. 입력이 합쳐지거나 서버가 스스로 턴을 열 수도 있어 지시와
//! 결과의 수를 맞춰 볼 수도 없다. 그래서 client 는 원격 자식의 결과 소유자를 생성한 부모로 고정한다.
//!
//! - 소유자는 자식을 만든 부모다. 새 자식임이 확인된 곳(spawn·늦은 spawn 의 묶기)에서만 기록을 새로 쓴다.
//! - 부모 없이 만든 자식은 prompt 로 이미 턴을 열었으면 소유자 없음([`NOBODY`])으로 고정하고, 아니면 첫 tell 의 부모가
//!   소유자가 된다.
//! - 다른 소유자의 tell 은 완료·idle 여부와 관계없이 경로를 바꾸거나 보내기 전에 거절한다. 시간으로 풀지 않는다. 다른
//!   부모는 새 자식을 만들어야 한다.
//! - 기록은 mirror surface(L) 별 파일이며 자식을 닫으면 지운다.
//!
//! 계약: docs/plugins/remote-attach-children.md.

use std::path::{Path, PathBuf};

use serde_json::json;

use crate::remote_results::{lock, read, write};

/// plugin 데이터 폴더 아래 기록 폴더 이름.
pub const DIR: &str = "attach-owners";
/// 결과를 받을 부모가 없는 소유자.
pub const NOBODY: &str = "none";

/// 물리 surface 부모의 소유자 값. 같은 surface 라도 발급 때 Claude 세션이 다르면 다른 부모다.
pub fn surface_owner(surface: u32, session: Option<&str>) -> String {
    format!("surface:{surface}:{}", session.unwrap_or_default())
}

/// Codex 대화 부모의 소유자 값.
pub fn codex_owner(receiver_id: &str) -> String {
    format!("codex:{receiver_id}")
}

/// tell 을 보내도 되는지.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Admit {
    /// 이미 이 부모의 자식이다.
    Owner,
    /// 소유자가 없던 자식을 이 부모가 맡았다. 보내지 못한 것이 확정되면 [`unclaim`] 으로 되돌린다.
    Claimed,
    /// 다른 부모의 자식이다.
    Other(String),
}

fn file(dir: &Path, child: u32) -> PathBuf {
    dir.join(format!("{child}.json"))
}

fn owner_in(dir: &Path, child: u32) -> Result<Option<String>, String> {
    Ok(read(&file(dir, child))?.and_then(|v| v.get("owner")?.as_str().map(String::from)))
}

fn store(dir: &Path, child: u32, owner: Option<&str>) -> Result<(), String> {
    let path = file(dir, child);
    match owner {
        Some(owner) => write(
            dir,
            &path,
            &json!({ "owner": owner, "child_surface": child }),
        ),
        None => match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        },
    }
}

/// 새로 만든 자식의 소유자를 쓴다. 같은 surface id 에 남은 이전 기록은 덮어쓴다. `None` 이면 첫 tell 이 맡는다.
pub fn reset(dir: &Path, child: u32, owner: Option<&str>) -> Result<(), String> {
    let _guard = lock(dir)?;
    store(dir, child, owner)
}

/// `owner` 의 tell 을 판정한다. 소유자가 없으면 `owner` 가 맡는다.
pub fn admit(dir: &Path, child: u32, owner: &str) -> Result<Admit, String> {
    let _guard = lock(dir)?;
    match owner_in(dir, child)? {
        Some(current) if current == owner => Ok(Admit::Owner),
        Some(current) => Ok(Admit::Other(current)),
        None => {
            store(dir, child, Some(owner))?;
            Ok(Admit::Claimed)
        }
    }
}

/// [`Admit::Claimed`] 로 맡은 자식의 tell 이 확정 실패했다. 소유자 없음으로 되돌린다.
pub fn unclaim(dir: &Path, child: u32, owner: &str) -> Result<(), String> {
    let _guard = lock(dir)?;
    match owner_in(dir, child)? {
        Some(current) if current == owner => store(dir, child, None),
        _ => Ok(()),
    }
}

/// 자식을 닫았다.
pub fn forget(dir: &Path, child: u32) -> Result<(), String> {
    reset(dir, child, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_creating_parent_keeps_the_child() {
        let d = tempfile::tempdir().unwrap();
        let a = surface_owner(3, Some("s-a"));
        reset(d.path(), 15, Some(&a)).unwrap();
        assert_eq!(admit(d.path(), 15, &a).unwrap(), Admit::Owner);
        assert_eq!(admit(d.path(), 15, &a).unwrap(), Admit::Owner);
        // 다른 surface, 같은 surface 의 다른 세션, Codex 대화, 소유자 없음은 모두 다른 부모다.
        for other in [
            surface_owner(4, Some("s-a")),
            surface_owner(3, Some("s-b")),
            codex_owner("r1"),
            NOBODY.to_string(),
        ] {
            assert_eq!(
                admit(d.path(), 15, &other).unwrap(),
                Admit::Other(a.clone())
            );
        }
        // 새 자식은 남은 기록을 덮어쓴다.
        reset(d.path(), 15, Some(NOBODY)).unwrap();
        assert_eq!(admit(d.path(), 15, NOBODY).unwrap(), Admit::Owner);
        assert!(matches!(admit(d.path(), 15, &a).unwrap(), Admit::Other(_)));
    }

    #[test]
    fn an_unowned_child_goes_to_its_first_teller_until_that_fails() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (codex_owner("r1"), surface_owner(4, None));
        reset(d.path(), 15, None).unwrap();
        assert_eq!(admit(d.path(), 15, &a).unwrap(), Admit::Claimed);
        // 다른 부모의 되돌리기는 소유자를 바꾸지 않는다.
        unclaim(d.path(), 15, &b).unwrap();
        assert_eq!(admit(d.path(), 15, &b).unwrap(), Admit::Other(a.clone()));
        unclaim(d.path(), 15, &a).unwrap();
        assert_eq!(admit(d.path(), 15, &b).unwrap(), Admit::Claimed);
        forget(d.path(), 15).unwrap();
        assert_eq!(admit(d.path(), 15, &a).unwrap(), Admit::Claimed);
        forget(d.path(), 99).unwrap();
    }
}
