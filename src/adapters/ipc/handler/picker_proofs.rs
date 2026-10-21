//! 사용자가 플러그인의 파일 피커에서 확정한 선택을 한 번만 쓸 수 있는 증명으로 보관한다.
//! 플러그인이 그 결과로 부른 요청(`markdown.navigate`의 `picker_request_id`)을
//! host가 사용자 요청으로 판정할 때 쓴다(docs/features/native-file-picker/index.md).
//! 증명은 (플러그인, request_id, 고른 경로)가 모두 맞을 때 한 번 소비된다.
//! 피커를 연 창과 관계없이 request_id가 프로세스 안에서 유일하므로 전역 한 곳에 둔다.

use std::collections::VecDeque;
use std::sync::Mutex;

/// 보관하는 확정 결과 수. 플러그인이 결과를 쓰지 않으면 오래된 것부터 버린다.
const KEEP: usize = 32;

struct Proof {
    plugin_id: String,
    request_id: u64,
    paths: Vec<String>,
}

static PROOFS: Mutex<VecDeque<Proof>> = Mutex::new(VecDeque::new());

fn proofs() -> std::sync::MutexGuard<'static, VecDeque<Proof>> {
    PROOFS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 피커가 확정 결과를 플러그인에 보낼 때 기록한다.
pub(crate) fn record(plugin_id: &str, request_id: u64, paths: &[String]) {
    if paths.is_empty() {
        return;
    }
    let mut proofs = proofs();
    if proofs.len() >= KEEP {
        proofs.pop_front();
    }
    proofs.push_back(Proof {
        plugin_id: plugin_id.to_string(),
        request_id,
        paths: paths.to_vec(),
    });
}

/// `plugin_id`가 연 피커 `request_id`에서 사용자가 `path`를 골랐으면 그 증명을 소비하고 참이다.
pub(crate) fn take(plugin_id: &str, request_id: u64, path: &str) -> bool {
    let mut proofs = proofs();
    let Some(i) = proofs.iter().position(|p| {
        p.plugin_id == plugin_id && p.request_id == request_id && p.paths.iter().any(|q| q == path)
    }) else {
        return false;
    };
    proofs.remove(i);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    // request_id는 실제로 프로세스 전역 증가값이라 시험도 겹치지 않는 큰 값을 쓴다.
    const BASE: u64 = 0x7000_0000_0000;

    #[test]
    fn a_confirmed_pick_is_taken_once_by_the_same_plugin_and_path() {
        record("p", BASE + 1, &["/a.md".into(), "/b.md".into()]);
        assert!(!take("other", BASE + 1, "/a.md"), "다른 플러그인");
        assert!(!take("p", BASE + 2, "/a.md"), "다른 요청");
        assert!(!take("p", BASE + 1, "/c.md"), "고르지 않은 경로");
        assert!(take("p", BASE + 1, "/b.md"));
        assert!(!take("p", BASE + 1, "/a.md"), "한 번 쓴 증명");
    }

    #[test]
    fn a_cancelled_pick_leaves_no_proof() {
        record("p", BASE + 10, &[]);
        assert!(!take("p", BASE + 10, ""));
    }
}
