//! Webview-enabled surface kind 등록.
//!
//! plugin 매니페스트의 `[[surface_kinds]]` 에 `rendering = "webview"` 를 선언하면
//! 호스트가 그 kind 의 surface 가 생성될 때 자동으로 OS-level native WebView overlay
//! 를 생성·동기화한다. plugin 은 `webview.set_url(surface_id, url)` 등 IPC 로
//! URL/navigation 만 제어한다. host 는 어떤 컨텐츠 (html / svg / ...) 인지 모름.
//!
//! egui-mesh 채널과 달리 화이트리스트가 없다 — 어떤 plugin 이든 webview overlay
//! 를 사용할 수 있다 (`webview.*` IPC 권한으로 제어). host 는 자기 builtin kind
//! 정의를 갖지 않으며, plugin 이 정의한 kind 의 SurfaceKindDef 는 일반 remote
//! 메커니즘 (`remote_kind`) 으로 등록된다.
//!
//! 본 모듈은 surface_registry 에 "webview-enabled" 플래그만 기록한다 —
//! `sync_webviews` 가 매 프레임 이 flag 를 확인해서 해당 surface 에 webview
//! overlay 를 붙인다.

use std::collections::HashSet;
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;

/// 전역 webview-enabled kind 집합 (plugin 이 hello 시 등록).
static WEBVIEW_KINDS: RwLock<Option<HashSet<String>>> = RwLock::new(None);

/// poison 을 보고했는가(첫 1 회만). `is_webview_kind` 가 **매 프레임** `sync_webviews`
/// 에서 도는 hot path 라 매번 남기면 폭주한다.
static POISON_REPORTED: AtomicBool = AtomicBool::new(false);

/// 복구는 원래부터 맞았지만 **조용했다** — poison 은 "어딘가에서 이미 패닉이 있었다" 는
/// 신호인데 조용한 복구가 그 신호를 지운다. 임계구역은 `HashSet` 삽입·조회뿐이라 불변식이
/// 성립하고, 조회는 메인 스레드의 프레임 경로라 패닉하면 프로세스가 죽는다 — 선택은
/// 그대로 복구, 다만 첫 1 회를 남긴다
/// ([`error-handling.md`](../../../docs/dev-guide/error-handling.md) "락 poison").
const WHAT: &str = "webview kind set";

/// plugin manager 가 hello 직후 매니페스트에 `rendering = "webview"` 선언이 있을 때 호출.
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

/// 주어진 surface kind 가 webview overlay 를 사용하는지 query.
/// `sync_webviews` 가 매 프레임 호출.
pub fn is_webview_kind(kind: &str) -> bool {
    crate::poison::recover_read(WEBVIEW_KINDS.read(), WHAT, &POISON_REPORTED)
        .as_ref()
        .is_some_and(|s| s.contains(kind))
}

/// test only — `WEBVIEW_KINDS` 는 프로세스 전역이고, 이 크레이트의 여러 테스트가 그것을
/// 병렬로 등록/reset 한다. 특히 `state::tests::test_state_with_memory`(176 곳이 호출)가
/// markdown kind 를 등록하는데, 그 register 가 `register_and_is_webview_kind_survive_poison`
/// 의 `!is_webview_kind("markdown")` 단언 중에 끼어들면 단언이 깨진다(형태 A). 이 전역을
/// 만지는 테스트는 전부 이 락을 잡아 직렬화한다 — 락을 잡지 않는 접근이 하나라도 있으면
/// 직렬화가 무효가 된다(락은 잡는 쪽끼리만 막는다). register 만 하는 헬퍼는 그 호출을 이
/// 락으로 감싸고, 전역을 reset/read 하는 테스트는 함수 끝까지 잡는다.
///
/// 프로덕션 등록 경로(`register_webview_kind`, plugin lifecycle)는 이 락을 잡지 않는다 —
/// 부팅 시 단일 스레드 등록을 가정하기 때문이다. 위의 "하나라도 락 밖 접근이 있으면 무효"
/// 는 그래서 **테스트 축의** 불변식이다.
///
/// 그 불변식에 이제 채널이 있다 — `source_guards::test_serialization_locks` 가 이 크레이트의
/// `#[test]` 중 이 전역의 접근면(전역 자신 · `register_webview_kind` · `is_webview_kind` ·
/// `reset_for_test`)을 부르는 것을 전부 걷어, 이 락을 안 잡으면 실패한다. 남는 사각은
/// **이름을 하나도 안 쓰고 두 겹 너머로 닿는 경로**다(가드가 언급으로 판정한다).
#[cfg(test)]
pub static WEBVIEW_KIND_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// test only — 등록된 kind 모두 제거.
#[cfg(test)]
pub fn reset_for_test() {
    *crate::poison::recover_write(WEBVIEW_KINDS.write(), WHAT, &POISON_REPORTED) = None;
}

#[cfg(test)]
// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다(전수 가드가 제외한다) —
// 여기 경고는 조치 대상이 될 수 없어 프로덕션 신호만 가린다. error-handling.md.
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

        // 의도적으로 poison 유발
        let _ = std::thread::spawn(|| {
            let _guard = WEBVIEW_KINDS.write().unwrap();
            panic!("simulate poison");
        })
        .join();

        // poison 이후에도 패닉하지 않고 정상 등록/조회되어야 한다.
        register_webview_kind("plugin-a", "html");
        assert!(is_webview_kind("html"));
        assert!(!is_webview_kind("markdown"));

        reset_for_test();
    }
}
