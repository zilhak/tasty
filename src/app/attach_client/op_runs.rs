//! 원격에 보낸 구조 변경을 기다리는 요청의 run([`crate::request_runs`]). 보낸 연결(epoch)과
//! op_id로 원격 결과(`StructuralSucceeded`/`StructuralFailed`)를 그 run에 잇는다. 재연결하면
//! op_id가 0부터 다시 시작하므로 epoch까지 맞아야 같은 요청이다.
//!
//! 메인 루프만 쓰지만 세션 상태가 다른 크레이트에 있어 정적 목록에 둔다.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{Value, json};
use tasty_remote::connection::ConnectionEpoch;

use crate::request_runs::{RunGuard, RunState, finish};

/// 보낸 뒤 결과 전에 연결이 끝났을 때. 원격이 이미 적용했을 수 있어 성공·실패를 정하지 않는다.
pub(crate) const CONNECTION_ENDED: &str =
    "the mirror connection ended before the remote answered; the remote may have applied it";

struct Watch {
    epoch: ConnectionEpoch,
    op: u64,
    run: RunGuard,
    /// 원격이 성공을 알렸고 만든 원격 surface가 아직 로컬에 대응되지 않았다.
    awaiting: Option<u32>,
}

static WATCHES: Mutex<Vec<Watch>> = Mutex::new(Vec::new());

fn watches() -> std::sync::MutexGuard<'static, Vec<Watch>> {
    WATCHES.lock().unwrap_or_else(|poisoned| {
        tracing::warn!("mirror operation run list was poisoned; keeping its entries");
        poisoned.into_inner()
    })
}

/// 원격 큐에 넣은 구조 변경의 결과를 기다린다.
pub(crate) fn watch(epoch: ConnectionEpoch, op: u64, run: RunGuard) {
    watches().push(Watch {
        epoch,
        op,
        run,
        awaiting: None,
    });
}

fn succeed(run: &RunGuard, local: Option<u32>, remote: Option<u32>) {
    let result = match local {
        Some(surface) => format!("surface {surface} is ready"),
        None => "the remote applied the change".to_string(),
    };
    finish(
        run.take(),
        RunState::Succeeded,
        result,
        json!({ "surface_id": local, "remote_surface_id": remote }),
    );
}

/// 원격 성공. 만든 surface가 로컬에 이미 대응되면 그 ID로 끝내고, 아니면 다음 delta를 기다린다.
pub(crate) fn succeeded(
    epoch: &ConnectionEpoch,
    op: u64,
    created: Option<u32>,
    mapping: &HashMap<u32, u32>,
) {
    let mut watches = watches();
    let Some(index) = watches
        .iter()
        .position(|w| w.epoch.same(epoch) && w.op == op && w.awaiting.is_none())
    else {
        return;
    };
    match created.map(|remote| (remote, mapping.get(&remote).copied())) {
        Some((remote, None)) => watches[index].awaiting = Some(remote),
        Some((remote, Some(local))) => {
            succeed(&watches.remove(index).run, Some(local), Some(remote))
        }
        None => succeed(&watches.remove(index).run, None, None),
    }
}

/// 원격 실패. 원격이 준 사유를 그대로 남긴다. 원격에 파일이 없다는 거절이면 호출자가 문구 없이
/// 구분할 수 있도록 `reason_code: "not_found"` 를 함께 남긴다.
pub(crate) fn failed(epoch: &ConnectionEpoch, op: u64, reason: Option<&str>) {
    let mut watches = watches();
    if let Some(index) = watches
        .iter()
        .position(|w| w.epoch.same(epoch) && w.op == op && w.awaiting.is_none())
    {
        let watch = watches.remove(index);
        finish(
            watch.run.take(),
            RunState::Failed,
            match reason {
                Some(reason) => format!("the remote could not apply the change: {reason}"),
                None => "the remote could not apply the change".to_string(),
            },
            failure_extra(reason),
        );
    }
}

fn failure_extra(reason: Option<&str>) -> Value {
    match reason {
        Some(reason)
            if reason.starts_with(tasty_ipc::stream::STRUCTURAL_REASON_PATH_NOT_FOUND_PREFIX) =>
        {
            json!({ "reason_code": "not_found" })
        }
        _ => Value::Null,
    }
}

/// delta를 적용한 뒤 성공을 알린 요청을 끝낸다. 서버는 결과 바로 뒤에 delta를 보내므로 이
/// delta에 없는 surface는 그사이 원격에서 사라진 것이라 로컬 ID 없이 끝낸다.
pub(crate) fn mapped(epoch: &ConnectionEpoch, mapping: &HashMap<u32, u32>) {
    watches().retain(|w| {
        let Some(remote) = w.awaiting.filter(|_| w.epoch.same(epoch)) else {
            return true;
        };
        succeed(&w.run, mapping.get(&remote).copied(), Some(remote));
        false
    });
}

/// 결과 전에 끝난 연결의 요청을 `unknown`으로 끝낸다. 성공을 알렸으나 로컬 대응 전에 끊긴
/// 요청은 원격 적용이 확인됐으므로 로컬 ID 없이 성공으로 끝낸다.
pub(crate) fn end_closed_connections() {
    watches().retain(|w| {
        if w.epoch.is_active() {
            return true;
        }
        match w.awaiting {
            Some(remote) => succeed(&w.run, None, Some(remote)),
            None => {
                finish(
                    w.run.take(),
                    RunState::Unknown,
                    CONNECTION_ENDED,
                    Value::Null,
                );
            }
        }
        false
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request_runs::{begin, lookup};

    fn run() -> (u64, RunGuard) {
        let id = begin("markdown.navigate", None, Value::Null);
        (id, RunGuard::new(id, "dropped"))
    }

    fn epoch() -> ConnectionEpoch {
        tasty_remote::connection::channel().0.epoch()
    }

    #[test]
    fn a_remote_result_reaches_only_the_run_of_its_connection_and_operation() {
        let (old_epoch, new_epoch) = (epoch(), epoch());
        let (failed_id, failed_run) = run();
        let (other_id, other_run) = run();
        let (ok_id, ok_run) = run();
        watch(old_epoch.clone(), 7, failed_run);
        watch(new_epoch.clone(), 7, other_run);
        watch(new_epoch.clone(), 8, ok_run);

        failed(&old_epoch, 7, Some("no such kind"));
        let ended = lookup(failed_id, None).unwrap();
        assert_eq!(ended["state"], "failed");
        assert_eq!(
            ended["result"],
            "the remote could not apply the change: no such kind"
        );
        // 재연결 뒤 같은 op_id라도 다른 연결의 요청은 그대로다.
        assert_eq!(lookup(other_id, None).unwrap()["state"], "running");

        succeeded(&new_epoch, 8, Some(40), &HashMap::new());
        assert_eq!(lookup(ok_id, None).unwrap()["state"], "running");
        mapped(&new_epoch, &HashMap::from([(40, 12)]));
        let created = lookup(ok_id, None).unwrap();
        assert_eq!(created["state"], "succeeded");
        assert_eq!(created["surface_id"], 12);
        assert_eq!(created["remote_surface_id"], 40);

        succeeded(&new_epoch, 7, Some(41), &HashMap::from([(41, 13)]));
        assert_eq!(lookup(other_id, None).unwrap()["surface_id"], 13);
    }

    #[test]
    fn a_connection_that_ends_first_leaves_the_result_unknown() {
        let live = epoch();
        let (id, guard) = run();
        watch(live.clone(), 3, guard);
        live.retire();
        end_closed_connections();
        let ended = lookup(id, None).unwrap();
        assert_eq!(ended["state"], "unknown");
        assert_eq!(ended["result"], CONNECTION_ENDED);
    }

    /// 원격이 파일 없음으로 거절한 실패만 reason_code 로 구분된다. 다른 거절은 문구만 남는다.
    #[test]
    fn only_a_remote_missing_file_refusal_carries_the_not_found_code() {
        let live = epoch();
        let (missing_id, missing) = run();
        let (other_id, other) = run();
        watch(live.clone(), 1, missing);
        watch(live.clone(), 2, other);
        failed(&live, 1, Some("path not found: /srv/a.md"));
        failed(&live, 2, Some("remote tab missing"));
        let missing = lookup(missing_id, None).unwrap();
        assert_eq!(missing["state"], "failed");
        assert_eq!(missing["reason_code"], "not_found");
        assert_eq!(
            missing["result"],
            "the remote could not apply the change: path not found: /srv/a.md"
        );
        let other = lookup(other_id, None).unwrap();
        assert_eq!(other["state"], "failed");
        assert!(other.get("reason_code").is_none(), "{other}");
    }
}
