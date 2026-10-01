//! plugin이 선언한 webview 종류 이름을 프로세스 전역에 기록한다.
//! remote kind 정의 등록·권한 검사·실제 overlay 생성은 호출 경로가 따로 맡는다.

use std::collections::HashSet;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;

static WEBVIEW_KINDS: RwLock<Option<HashSet<String>>> = RwLock::new(None);

/// 같은 poison을 프레임마다 반복해 로그에 남기지 않는다.
static POISON_REPORTED: AtomicBool = AtomicBool::new(false);

/// poison을 첫 로그와 함께 복구해 기존 집합을 계속 사용한다. 내용을 재검증하지는 않는다.
const WHAT: &str = "webview kind set";

pub fn register_webview_kind(plugin_id: &str, kind: &str) {
    let inserted = {
        let mut guard = crate::poison::recover_write(WEBVIEW_KINDS.write(), WHAT, &POISON_REPORTED);
        let set = guard.get_or_insert_with(HashSet::new);
        set.insert(kind.to_string())
    };
    if inserted {
        tracing::info!(
            "registered webview-enabled surface kind '{}' for plugin '{}'",
            kind,
            plugin_id
        );
    }
}

#[cfg(any(feature = "gui", test))]
pub fn is_webview_kind(kind: &str) -> bool {
    crate::poison::recover_read(WEBVIEW_KINDS.read(), WHAT, &POISON_REPORTED)
        .as_ref()
        .is_some_and(|s| s.contains(kind))
}

/// 전역 집합을 등록·초기화·조회하는 검사는 이 락을 함께 사용해야 한다.
/// 등록만 하는 헬퍼는 그 호출을, 초기화·조회 검사는 검사 전체를 잠근다.
/// 제품 등록 경로는 이 검사 락을 쓰지 않는다. 접근 이름을 찾는 source guard는
/// 이름이 드러나지 않는 간접 호출까지 추적하지는 못한다.
#[cfg(test)]
pub static WEBVIEW_KIND_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub fn reset_for_test() {
    *crate::poison::recover_write(WEBVIEW_KINDS.write(), WHAT, &POISON_REPORTED) = None;
}

#[cfg(test)]
// 이유: 검사에서 오류를 의도적으로 버리는 표현을 허용한다.
#[allow(clippy::let_underscore_must_use)]
mod tests {
    use super::*;

    #[test]
    fn register_and_query() {
        let _guard = WEBVIEW_KIND_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        reset_for_test();
        assert!(!is_webview_kind("foo"));
        register_webview_kind("com.example", "foo");
        assert!(is_webview_kind("foo"));
        assert!(!is_webview_kind("bar"));
    }

    #[test]
    fn register_and_is_webview_kind_survive_poison() {
        let _guard = WEBVIEW_KIND_TEST_LOCK
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        reset_for_test();

        let _ = std::thread::spawn(|| {
            let _guard = WEBVIEW_KINDS.write().unwrap();
            panic!("simulate poison");
        })
        .join();

        register_webview_kind("plugin-a", "html");
        assert!(is_webview_kind("html"));
        assert!(!is_webview_kind("markdown"));

        reset_for_test();
    }
}
