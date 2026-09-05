//! plugin 이 점유할 수 없는 prefix 목록이 **호스트 메서드 표와 맞물려 있는가.**
//!
//! `tasty_plugin_manifest::validators::RESERVED_IPC_PREFIXES` 는 매니페스트 검증에서
//! `[[contributes.ipc_namespace]]` 를 거절하는 데 쓰인다. 그 목록은 손으로 유지돼 왔고,
//! 호스트가 새 메서드 prefix 를 만들 때 함께 갱신되리라는 보장이 없었다. 갱신이 빠지면
//! plugin 이 그 이름을 점유할 수 있게 되고, 그 뒤 호스트가 같은 prefix 에 메서드를 더하면
//! **표에 없는 `<prefix>.*` 가 plugin 으로 forward 된다.** 실패가 그 자리에서 안 나고
//! 나중에 이름이 겹칠 때 나므로, 목록을 눈으로 유지하는 것으로는 못 막는다.
//!
//! 지금은 호스트 prefix 45 개 중 **번들 plugin 이 점유한 둘을 뺀 전부**가 예약돼 있다.
//! 그 결정과 감수한 비용은
//! [ADR-0140](../../docs/adr/0140-host-ipc-prefixes-are-reserved-where-they-can-be-enforced.md).
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
use std::path::PathBuf;

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
        "fs",
        "`fs.pick_file` 이 ADR-0162 로 빠져 그 아래 호스트 메서드가 0 개가 됐다. \
         이름은 계속 막는다 — 비었다고 내주면 `fs.*` 가 호스트 파일시스템 표면처럼 \
         읽히는 자리를 plugin 이 갖는다(ADR-0140 의 '뺏는 것보다 막는 쪽이 싸다')",
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
        "호스트 메서드가 {} 건뿐이다(하한 {MIN_HOST_METHODS}, 2026-09-05 실측 \
         `METHOD_TABLE.len()` = 276). 표가 비면 아래 집합 동등은 양쪽이 빈 집합이라 \
         그냥 통과한다",
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

// ─── 예약에서 뺀 prefix 가 만드는 두 번째 결과 ────────────────────────────────

/// 호스트가 dispatch 하는 메서드 표. `plugin` 이 아니라 **본체**의 match arm 이라
/// 링크 가능한 값이 아니다 — 그래서 여기만 텍스트로 읽는다(R96 의 반대편 조건).
const DISPATCH_ROOT: &str = "src/adapters/ipc/handler.rs";

/// 그 파일에서 걷은 `"<a>.<b>"` 리터럴 수의 하한 — **연기 검사**.
/// 값의 근거: 2026-09-05 실측 213 개.
const MIN_DISPATCH_METHOD_LITERALS: usize = 150;

/// 번들 plugin 이 점유한 prefix 안에서 **호스트도 dispatch arm 을 가진** 메서드.
///
/// `IpcNamespaceRegistry::resolve` 는 **prefix 만** 본다 — 메서드 단위 예외가 없다.
/// 그래서 plugin 이 떠 있으면 그 prefix 의 **모든** 메서드가 plugin 으로 가고, 같은
/// 이름의 호스트 arm 은 외부 호출자에게서 가려진다.
///
/// 지금 그것이 사고가 아닌 이유는 image plugin 이 이 셋을 **호스트로 되던지기**
/// 때문이다(`trampoline` — 받은 메서드를 그대로 `host.call` 한다). 즉 이 셋은
/// "가려졌지만 우회로가 있는" 상태이고, **넷째가 우회로 없이 생기면 그 호스트 구현은
/// 외부에서 닿지 않게 된다.** 이 가드가 지키는 것이 그 경계다.
///
/// ## 누가 답했는지는 응답 문구가 가른다
///
/// 세 이름 모두 **plugin 이 먼저 받는다**(forward 가 라우터의 첫 단계다). plugin 이
/// trampoline 으로 `host.call` 하면 SDK 가 실패를 `host call '<call#N>' failed: …` 로
/// 감싸므로, **그 래퍼의 유무가 "host 가 직접 답했나" 를 가른다.** 결과값만 보면
/// 두 경로가 구별되지 않는다 — 성공 응답은 host 것이 그대로 통과하기 때문이다.
///
/// 실측(2026-09-05, gui 격리 홈):
///
/// - plugin 실행중 · `image.open {}` → `-32602 host call 'call#2' failed: missing
///   'surface_id'` — 래퍼가 있다. forward → plugin → trampoline → host 다
/// - plugin 실행중 · `markdown.navigate {}` → `-32602 host call 'call#1' failed:
///   invalid params: missing field ``surface_id``` — 같은 형태
/// - plugin 실행중 · `image.list` → `{"entries":[]}` (host 값이 그대로 나온다)
/// - plugin **미실행**(`plugin.disable`) · `image.list` → `-32002 plugin
///   'com.tasty.image' is not running` — 소유는 매니페스트에서 오므로 유지되고
///   실행만 거절된다(ADR-0173)
/// - plugin **제거**(`plugin.remove`) · `image.open {}` → `-32602 missing
///   'surface_id'` — **래퍼가 없다.** 소유가 풀려 host 가 직접 답한다
///
/// headless 는 세 arm 이 전부 `#[cfg(feature = "gui")]` 라 **구현 자체가 없다** —
/// trampoline 이 되던져도 `-32601` 이다. 조합 차이의 원인은 라우팅 순서가 **아니다**
/// (ADR-0173 이후 두 조합 모두 forward 가 먼저다).
const SHARED_WITH_A_BUNDLED_PLUGIN: &[(&str, &str)] = &[
    (
        "image.list",
        "호스트는 surface 순회, plugin 도 같은 이름을 구현한다",
    ),
    ("image.open", "plugin 이 trampoline 으로 호스트에 되던진다"),
    (
        "markdown.navigate",
        "plugin 이 자기 주소창에서 host.call 로 부른다 — 받지는 않는다",
    ),
];

/// 주석을 걷어낸 dispatch 소스에서 `"<a>.<b>"` 메서드 리터럴을 뽑는다.
///
/// 주석 제거는 **이 파일에서는 판정을 바꾸지 않는다** — 켜고 끄고 결과가 같다(실측).
/// `handler.rs` 의 주석이 번들 plugin 의 두 prefix 를 인용하지 않기 때문이고, 다른
/// 파일이었으면 갈렸을 자리라 방어로 남긴다. 이 사실을 적어 두는 이유는 "변이가
/// 안 죽였다" 를 나중에 결함으로 오인하지 않게 하기 위해서다.
fn dispatch_method_literals(src: &str) -> BTreeSet<String> {
    let body = super::strip_comments(src);
    let mut out = BTreeSet::new();
    let mut rest = body.as_str();
    while let Some(at) = rest.find('"') {
        let after = &rest[at + 1..];
        match after.find('"') {
            Some(end) => {
                let lit = &after[..end];
                let ok = lit.split_once('.').is_some_and(|(a, b)| {
                    !a.is_empty()
                        && !b.is_empty()
                        && a.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                        && b.chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                });
                if ok {
                    out.insert(lit.to_string());
                }
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

fn dispatch_root() -> PathBuf {
    super::repo_root().join(DISPATCH_ROOT)
}

/// 번들 plugin 과 이름이 겹치는 호스트 메서드는 **동결돼 있다.**
#[test]
fn the_methods_a_bundled_plugin_can_shadow_are_pinned() {
    let src = std::fs::read_to_string(dispatch_root())
        .unwrap_or_else(|e| panic!("{DISPATCH_ROOT} 읽기 실패: {e}"));
    let literals = dispatch_method_literals(&src);
    assert!(
        literals.len() >= MIN_DISPATCH_METHOD_LITERALS,
        "dispatch 리터럴을 {} 개만 걷었다(하한 {MIN_DISPATCH_METHOD_LITERALS},          2026-09-05 실측 213). 추출이 죽으면 아래 집합 동등은 양쪽이 빈 집합이라 통과한다",
        literals.len()
    );

    let claimed: BTreeSet<&str> = CLAIMED_BY_A_BUNDLED_PLUGIN
        .iter()
        .map(|(p, _)| *p)
        .collect();
    let found: BTreeSet<&str> = literals
        .iter()
        .filter(|m| m.split_once('.').is_some_and(|(p, _)| claimed.contains(p)))
        .map(|m| m.as_str())
        .collect();
    let pinned: BTreeSet<&str> = SHARED_WITH_A_BUNDLED_PLUGIN
        .iter()
        .map(|(m, _)| *m)
        .collect();

    assert_eq!(
        found, pinned,
        "번들 plugin 의 namespace 안에서 호스트가 dispatch 하는 메서드 집합이 달라졌다.\n\
         늘었다면 호스트 구현 하나가 plugin namespace 뒤로 가려진 것이다. plugin 이 \
         그 이름을 host 로 되던지지 않으면 외부 호출자는 그 호스트 구현에 닿지 못한다 — \
         trampoline 이 있는지 확인하고 사유와 함께 여기 적어라. 줄었다면 빼라"
    );
}

/// 스캔 루트가 이 가드 자신을 포함하지 않는다.
///
/// 이 파일은 자기가 찾는 형태(`"image.open"` 같은 메서드 리터럴)를 동결 목록으로 담고
/// 있다. 루트가 넓어져 이 파일을 삼키면 자기 목록을 "호스트가 dispatch 하는 메서드" 로
/// 세고, 집합 동등이 자기 자신을 상대로 성립한다.
#[test]
fn the_dispatch_scan_root_is_a_single_file_that_is_not_this_one() {
    assert!(
        !DISPATCH_ROOT.contains("source_guards"),
        "스캔 루트({DISPATCH_ROOT})가 이 가드가 사는 곳을 포함한다"
    );
    assert!(
        dispatch_root().is_file(),
        "{DISPATCH_ROOT} 가 파일이 아니다 — 디렉터리로 넓어지면 이 가드도 삼켜진다"
    );
}
