//! client: 원격 자식마다 아직 결과를 받지 못한 지시의 소유자.
//!
//! 서버는 자식마다 결과 경로를 하나만 두고, 턴이 끝날 때 그 경로의 마지막 route 토큰을 결과에 싣는다. 그래서 한 부모가
//! 지시한 턴이 진행되는 동안 다른 부모가 tell 하면 앞 턴의 결과가 뒤 부모에게 정상 결과처럼 간다. 서버는 같은 client
//! 의 부모들을 구분하지 못하므로 client 가 판정한다.
//!
//! - 턴을 여는 지시(prompt 를 준 spawn, tell)를 보내기 전에 소유자와 미완 수를 기록한다. 다른 소유자의 미완 지시가
//!   있으면 경로를 바꾸거나 서버에 보내기 전에 거절한다. 같은 소유자의 후속 지시는 허용하고 미완 수를 늘린다.
//! - 미완 수는 그 소유자에게 온 새 결과(재전송 중복 제외)마다 줄고 0 이 되면 기록을 지운다. 결과 상태가 `exited` 이거나
//!   자식을 닫으면 바로 지운다. 시간 경과나 상태 통지(`idle` 등)로는 풀지 않는다. 상태 통지가 결과보다 먼저 올 수 있고,
//!   그때 다른 부모를 받으면 같은 오배달이 생기기 때문이다.
//! - 같은 소유자의 지시 여럿이 자식 안에서 한 턴으로 합쳐지면 결과가 지시 수보다 적게 와서 기록이 남는다. 이때 다른
//!   부모는 그 자식을 닫거나(kill) 소유자가 남은 결과를 받을 때까지 거절된다. 잘못 전달하지 않는 쪽을 택한 한계다.
//! - 판정과 기록은 한 잠금 안에서 한다([`open`]). 판정을 경로 변경보다 앞에 두고, 그 뒤 단계가 확정 실패하면 [`undo`]
//!   로 되돌린다.
//! - 기록은 mirror surface(L) 별 파일이다. 수동 재attach 는 새 세션이라 새 mirror 의 기록은 비어 있다.
//!
//! 계약: docs/plugins/remote-attach-children.md.

use std::path::Path;

use serde_json::{Value, json};

use crate::remote_results::{lock, read, write};

/// plugin 데이터 폴더 아래 기록 폴더 이름.
pub const DIR: &str = "attach-turns";
/// 소유자가 없는 지시(호출 surface 도 Codex 대화도 아닌 호출자).
pub const NOBODY: &str = "none";

/// 물리 surface 부모의 소유자 값. 같은 surface 에서 세션이 바뀌어도 같은 소유자다(앞 결과는 알림만 억제된다).
pub fn surface_owner(surface: u32) -> String {
    format!("surface:{surface}")
}

/// Codex 대화 부모의 소유자 값.
pub fn codex_owner(receiver_id: &str) -> String {
    format!("codex:{receiver_id}")
}

/// 지시를 열 수 있는지.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Opened {
    /// 기록했다. 보내지 못한 것이 확정되면 [`undo`] 로 되돌린다.
    Opened,
    /// 다른 소유자의 미완 지시가 있다.
    Busy { owner: String, open: u64 },
}

fn file(dir: &Path, child: u32) -> std::path::PathBuf {
    dir.join(format!("{child}.json"))
}

fn record(dir: &Path, child: u32) -> Result<Option<(String, u64)>, String> {
    Ok(read(&file(dir, child))?.and_then(|v| {
        let owner = v.get("owner")?.as_str()?.to_string();
        let open = v.get("open")?.as_u64()?;
        (open > 0).then_some((owner, open))
    }))
}

fn store(dir: &Path, child: u32, owner: &str, open: u64) -> Result<(), String> {
    let path = file(dir, child);
    if open == 0 {
        return match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        };
    }
    write(
        dir,
        &path,
        &json!({ "owner": owner, "open": open, "child_surface": child }),
    )
}

/// 턴을 여는 지시를 기록한다. 다른 소유자의 미완 지시가 있으면 기록하지 않고 `Busy` 다.
pub fn open(dir: &Path, child: u32, owner: &str) -> Result<Opened, String> {
    let _guard = lock(dir)?;
    match record(dir, child)? {
        Some((other, open)) if other != owner => Ok(Opened::Busy { owner: other, open }),
        current => {
            let open = current.map_or(0, |(_, n)| n) + 1;
            store(dir, child, owner, open)?;
            Ok(Opened::Opened)
        }
    }
}

/// 보내지 못한 것이 확정된 지시를 되돌린다. 결과불명이면 부르지 않는다(서버에서 실행됐을 수 있다).
pub fn undo(dir: &Path, child: u32, owner: &str) -> Result<(), String> {
    finish(dir, child, owner, None)
}

/// `owner` 에게 그 자식의 새 결과가 왔다. 미완 수를 하나 줄이고, `exited` 면 기록을 지운다.
pub fn finish(dir: &Path, child: u32, owner: &str, status: Option<&str>) -> Result<(), String> {
    let _guard = lock(dir)?;
    if status == Some("exited") {
        return store(dir, child, owner, 0);
    }
    match record(dir, child)? {
        Some((current, open)) if current == owner => store(dir, child, owner, open - 1),
        _ => Ok(()),
    }
}

/// 자식을 닫았다. 남은 지시 기록을 지운다.
pub fn forget(dir: &Path, child: u32) -> Result<(), String> {
    let _guard = lock(dir)?;
    store(dir, child, NOBODY, 0)
}

/// 결과 본문의 상태(`completion.status`).
pub fn status_of(completion: &Value) -> Option<&str> {
    completion.get("status").and_then(Value::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 기록을 바꾸지 않고 `owner` 가 지시할 수 있는지 본다.
    fn check(dir: &Path, child: u32, owner: &str) -> Result<Opened, String> {
        Ok(match record(dir, child)? {
            Some((other, open)) if other != owner => Opened::Busy { owner: other, open },
            _ => Opened::Opened,
        })
    }

    #[test]
    fn another_owner_waits_until_every_open_order_has_a_result() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (surface_owner(3), surface_owner(4));
        assert_eq!(open(d.path(), 15, &a).unwrap(), Opened::Opened);
        assert_eq!(open(d.path(), 15, &a).unwrap(), Opened::Opened);
        assert_eq!(
            open(d.path(), 15, &b).unwrap(),
            Opened::Busy {
                owner: a.clone(),
                open: 2
            }
        );
        assert_eq!(check(d.path(), 15, &a).unwrap(), Opened::Opened);
        finish(d.path(), 15, &a, Some("turn_ended")).unwrap();
        assert!(matches!(
            check(d.path(), 15, &b).unwrap(),
            Opened::Busy { open: 1, .. }
        ));
        // 다른 소유자 앞으로 온 결과는 미완 수를 줄이지 않는다.
        finish(d.path(), 15, &b, Some("turn_ended")).unwrap();
        assert!(matches!(
            check(d.path(), 15, &b).unwrap(),
            Opened::Busy { open: 1, .. }
        ));
        finish(d.path(), 15, &a, Some("turn_ended")).unwrap();
        assert_eq!(open(d.path(), 15, &b).unwrap(), Opened::Opened);
        // 다른 자식은 따로 센다.
        assert_eq!(open(d.path(), 16, &a).unwrap(), Opened::Opened);
    }

    #[test]
    fn exit_kill_and_undo_release_the_child() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (codex_owner("r1"), surface_owner(4));
        open(d.path(), 15, &a).unwrap();
        open(d.path(), 15, &a).unwrap();
        finish(d.path(), 15, &a, Some("exited")).unwrap();
        assert_eq!(check(d.path(), 15, &b).unwrap(), Opened::Opened);
        open(d.path(), 15, &a).unwrap();
        forget(d.path(), 15).unwrap();
        assert_eq!(check(d.path(), 15, &b).unwrap(), Opened::Opened);
        open(d.path(), 15, &a).unwrap();
        undo(d.path(), 15, &a).unwrap();
        assert_eq!(check(d.path(), 15, &b).unwrap(), Opened::Opened);
        // 기록 없는 자식의 결과·닫기는 오류가 아니다.
        finish(d.path(), 99, &a, None).unwrap();
        forget(d.path(), 99).unwrap();
    }

    #[test]
    fn the_record_survives_a_new_process() {
        let d = tempfile::tempdir().unwrap();
        open(d.path(), 15, &surface_owner(3)).unwrap();
        let text = std::fs::read_to_string(d.path().join("15.json")).unwrap();
        assert!(text.contains("surface:3"), "{text}");
        assert!(matches!(
            check(d.path(), 15, NOBODY).unwrap(),
            Opened::Busy { .. }
        ));
    }
}
