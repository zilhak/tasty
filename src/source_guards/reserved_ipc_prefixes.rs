//! plugin 이 점유할 수 없는 prefix 목록이 **호스트 메서드 표와 맞물려 있는가.**
//!
//! `tasty_plugin_manifest::validators::RESERVED_IPC_PREFIXES` 는 매니페스트 검증에서
//! `[[contributes.ipc_namespace]]` 를 거절하는 데 쓰인다. 그 목록은 손으로 유지돼 왔고,
//! 호스트가 새 메서드 prefix 를 만들 때 함께 갱신되리라는 보장이 없었다. 갱신이 빠지면
//! plugin 이 그 이름을 점유할 수 있게 되고, 그 뒤 호스트가 같은 prefix 에 메서드를 더하면
//! **표에 없는 `<prefix>.*` 가 plugin 으로 forward 된다.** 실패가 그 자리에서 안 나고
//! 나중에 이름이 겹칠 때 나므로, 목록을 눈으로 유지하는 것으로는 못 막는다.
//!
//! ## 왜 A5 와 추출 방식이 다른가
//!
//! 판단의 형태는 라우팅 키 가드(`routing_key_coverage`)와 같다 — **양방향 집합 동등 +
//! 사유가 붙은 면제 목록 + 모수 연기 검사**. 다른 것은 추출뿐이고, 이유는 입력이 다르기
//! 때문이다. 저쪽은 두 변 모두 값으로 존재하지 않아(핸들러의 키는 함수 본문 속 문자열
//! 리터럴, 인식 목록은 `gui` feature 로 게이트된 모듈) 소스를 텍스트로 읽는 것 말고
//! 방법이 없었다. 여기는 두 변이 다 `pub const` 이고 이 크레이트가 둘 다 링크한다.
//! 값이 있는데 텍스트로 읽으면 **더 약해진다** — 파서가 조용히 어긋나고, `cfg` 로
//! 갈리는 항목([`DEBUG_METHODS`])을 텍스트는 구별하지 못한다.
//!
//! 그래서 스캔 루트가 없고, 자기 자신이 스캔에 잡히는지(R80 형태)를 물을 대상도 없다.
//! 대신 두 상수가 **정말 그 두 크레이트의 것**인지는 링크가 보증한다.

use std::collections::BTreeSet;

use tasty_ipc::method_meta::{DEBUG_METHODS, METHOD_TABLE, PREFIX_RULES};
use tasty_plugin_manifest::validators::RESERVED_IPC_PREFIXES;

/// 호스트 메서드 수의 하한 — **연기 검사**다. 값의 근거: 2026-09-05 실측 276 건.
const MIN_HOST_METHODS: usize = 200;

/// 그 메서드들이 만드는 prefix 수의 하한. 값의 근거: 2026-09-05 실측 45 개.
///
/// 메서드 수와 따로 두는 이유: 이름을 자르는 쪽이 망가지면 메서드는 그대로인데 prefix 만
/// 하나로 뭉친다(예: 자르기가 빈 문자열을 내면 전부 걸러져 0). 그 형태는 위 하한을
/// 통과한다.
const MIN_HOST_PREFIXES: usize = 35;

/// 번들 plugin 이 같은 이름의 namespace 를 이미 점유해서 **예약할 수 없는** prefix.
///
/// 예약하면 그 매니페스트가 `ipc_namespace prefix '…' is reserved by the host` 로
/// 거절되어 번들 plugin 이 뜨지 못한다. 호스트 메서드와 plugin namespace 가 같은 이름을
/// 공유하는 것 자체는 성립한다 — 표에 있는 메서드는 호스트가 가져가고, 표에 없는
/// `<prefix>.*` 만 plugin 으로 간다.
const CLAIMED_BY_A_BUNDLED_PLUGIN: &[(&str, &str)] = &[
    ("image", "tasty-plugin-image 의 ipc_namespace"),
    ("markdown", "tasty-plugin-markdown 의 ipc_namespace"),
];

/// 호스트 메서드가 있는데 예약되지 않은 prefix 중, **왜 예약하지 않았는지 기록이 없는**
/// 것들. 이 가드가 생기기 전부터 그랬다.
///
/// 사유를 지어내지 않고 있는 그대로 동결한다. 이 목록의 값어치는 **크기가 늘지 않는
/// 것**이다 — 새 호스트 prefix 는 예약 쪽으로 가거나, 안 간다면 위
/// [`CLAIMED_BY_A_BUNDLED_PLUGIN`] 처럼 사유가 붙은 자리에 들어가야 한다. 여기에
/// 이름을 더하는 변경은 "기록 없음" 을 하나 더 만드는 것이므로 리뷰에서 보인다.
///
/// 이 상태 자체(호스트 prefix 25 개를 plugin 이 점유할 수 있다)는 이 가드가 고치는
/// 대상이 아니다. 고치려면 매니페스트 검증의 거절 범위가 넓어지는 동작 변경이라 별도
/// 판단이 필요하다.
const UNRESERVED_WITHOUT_A_RECORDED_REASON: &[&str] = &[
    "agent",
    "attach",
    "banner",
    "clipboard",
    "completion_strategy",
    "file_handler",
    "file_picker",
    "fs",
    "git_viewer",
    "hook_handler",
    "popup",
    "preset",
    "pty",
    "recent",
    "remote",
    "session",
    "settings",
    "telemetry",
    "terminal",
    "theme",
    "timer",
    "view",
    "webhook",
    "webview",
    "workspace_category",
];

/// 예약돼 있으나 호스트 메서드 표에는 그 prefix 의 메서드가 없는 것들.
///
/// 예약이 메서드 표보다 넓은 것은 의도된 방향이다 — 이름을 미리 막아 두는 쪽이 나중에
/// 뺏는 것보다 싸다. 다만 **왜** 막아 두는지는 항목마다 달라서 여기 적는다.
const RESERVED_AHEAD_OF_ANY_METHOD: &[(&str, &str)] = &[
    (
        "ime",
        "`surface.ime_*`(debug 전용 prefix 규칙)의 이름 공간. 최상위 `ime.*` 는 아직 없다",
    ),
    (
        "ipc",
        "IPC 자체를 가리키는 이름 — plugin 이 가질 자리가 아니다",
    ),
    (
        "tool",
        "매니페스트 `[[contributes.tool]]` 이 쓰는 이름 공간",
    ),
];

/// 호스트가 자기 IPC 메서드에 실제로 쓰는 prefix.
///
/// `DEBUG_METHODS` 와 `PREFIX_RULES` 는 `#[cfg(debug_assertions)]` 로 갈려서 release
/// 빌드에서는 빈 슬라이스다. 그래서 이 집합은 debug 에서 더 넓고, 예약 목록이 두 경우
/// 모두를 덮어야 한다 — 넓은 쪽에서 도는 것이 판정이 세다.
///
/// 점이 없는 메서드(`split` · `tree`)는 이름 전체가 prefix 자리를 차지한다. plugin 이
/// 같은 이름을 점유하면 CLI·문서에서 구별되지 않으므로 prefix 로 센다.
fn host_prefixes() -> BTreeSet<&'static str> {
    let methods = METHOD_TABLE
        .iter()
        .chain(DEBUG_METHODS.iter())
        .map(|(name, _)| *name);
    let rules = PREFIX_RULES.iter().map(|(name, _)| *name);
    methods
        .chain(rules)
        .filter_map(|name| name.split('.').next())
        .filter(|p| !p.is_empty())
        .collect()
}

/// 호스트 prefix 는 **예약돼 있거나, 왜 아닌지가 적혀 있다.**
#[test]
fn every_host_method_prefix_is_reserved_or_carries_a_reason() {
    assert!(
        METHOD_TABLE.len() >= MIN_HOST_METHODS,
        "호스트 메서드가 {} 건뿐이다(하한 {MIN_HOST_METHODS}, 2026-09-05 실측 325). \
         표가 비면 아래 집합 동등은 양쪽이 빈 집합이라 그냥 통과한다",
        METHOD_TABLE.len()
    );
    let host = host_prefixes();
    assert!(
        host.len() >= MIN_HOST_PREFIXES,
        "호스트 prefix 가 {} 개뿐이다(하한 {MIN_HOST_PREFIXES})",
        host.len()
    );
    let reserved: BTreeSet<&str> = RESERVED_IPC_PREFIXES.iter().copied().collect();
    let excused: BTreeSet<&str> = CLAIMED_BY_A_BUNDLED_PLUGIN
        .iter()
        .map(|(p, _)| *p)
        .chain(UNRESERVED_WITHOUT_A_RECORDED_REASON.iter().copied())
        .collect();

    let both: Vec<&str> = excused
        .iter()
        .copied()
        .filter(|p| reserved.contains(p))
        .collect();
    assert!(
        both.is_empty(),
        "예약돼 있으면서 동시에 면제된 prefix: {both:?} — 예약했으면 면제 목록에서 빼라. \
         겹친 채로 두면 면제의 사유가 코드와 모순인 상태가 초록으로 남는다"
    );

    let unaccounted: Vec<&str> = host
        .iter()
        .copied()
        .filter(|p| !reserved.contains(p) && !excused.contains(p))
        .collect();
    let stale: Vec<&str> = excused
        .iter()
        .copied()
        .filter(|p| !host.contains(p))
        .collect();

    assert!(
        unaccounted.is_empty() && stale.is_empty(),
        "예약 목록이 호스트 메서드 표와 어긋난다.\n\
         \x20 예약도 면제도 안 된 호스트 prefix: {unaccounted:?}\n\
         \x20 면제 목록에 있으나 호스트 메서드가 없는 prefix: {stale:?}\n\
         앞의 것은 plugin 이 그 이름을 점유할 수 있다는 뜻이고, 호스트가 나중에 같은 \
         prefix 에 메서드를 더하면 표에 없는 호출이 그 plugin 으로 샌다. \
         RESERVED_IPC_PREFIXES 에 넣거나, 넣지 못하는 사유를 이 파일에 적어라."
    );
}

/// 예약이 메서드 표보다 넓은 쪽도 사유가 붙어 있다.
///
/// 이쪽을 안 보면 예약 목록은 한 방향으로만 자란다 — 지워진 메서드의 prefix 가 남아
/// plugin 이 쓸 수 있었을 이름을 계속 막는다.
#[test]
fn every_reserved_prefix_without_a_host_method_says_why() {
    let host = host_prefixes();
    let explained: BTreeSet<&str> = RESERVED_AHEAD_OF_ANY_METHOD
        .iter()
        .map(|(p, _)| *p)
        .collect();

    let unexplained: Vec<&str> = RESERVED_IPC_PREFIXES
        .iter()
        .copied()
        .filter(|p| !host.contains(p) && !explained.contains(p))
        .collect();
    let stale: Vec<&str> = explained
        .iter()
        .copied()
        .filter(|p| host.contains(p) || !RESERVED_IPC_PREFIXES.contains(p))
        .collect();

    assert!(
        unexplained.is_empty() && stale.is_empty(),
        "메서드 없는 예약: {unexplained:?} / 사유가 낡은 항목: {stale:?}. \
         뒤의 것은 그 prefix 에 호스트 메서드가 생겼거나 예약이 풀렸다는 뜻이라, \
         RESERVED_AHEAD_OF_ANY_METHOD 에서 빼야 한다"
    );
}

/// 예약 목록 자체가 정렬돼 있고 중복이 없다.
///
/// 순서가 없으면 같은 이름이 두 번 들어가도 눈에 안 띄고, 위 두 테스트는 집합으로 보므로
/// 중복을 잡지 못한다.
#[test]
fn the_reserved_list_is_sorted_and_free_of_duplicates() {
    let mut sorted = RESERVED_IPC_PREFIXES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(
        sorted.as_slice(),
        RESERVED_IPC_PREFIXES,
        "RESERVED_IPC_PREFIXES 는 사전순 · 중복 없이 유지한다"
    );
}
