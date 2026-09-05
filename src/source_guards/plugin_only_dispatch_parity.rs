//! `plugin_only` 표식과 **plugin host-call 진입부의 인터셉트**가 같은 집합인가.
//!
//! 표(`METHOD_TABLE`)는 원래 caller 게이트만 담았다 — `plugin(&[…])` 은 "plugin 이
//! 부를 수 있다", `local_only()` 는 "plugin 은 못 부른다". 그래서 **plugin 만** 부를 수
//! 있는 메서드를 적을 자리가 없었고, 그런 메서드도 `plugin(&[…])` 으로 적혀 외부
//! 호출자에게는 `-32601`("그런 메서드 없다")로 답했다. 이름은 맞고 표에도 있는데
//! 없다고 답한 것이라, 플랫폼 축에서 같은 거짓을 고친
//! [ADR-0154](../../docs/adr/0154-a-platform-gated-dispatch-arm-answers-why-not-what.md)
//! 와 같은 형태다.
//!
//! 실측(2026-09-05, gui debug 인스턴스에 plugin 설치된 세계에서 외부 프로브):
//! `plugin_callable = true` 인 **231** 개 중 외부 호출이 `-32601` 로 끝나는 것은 **4** 개
//! (`banner.open` · `banner.close` · `popup.close` · `host.shared_buffer.create`).
//! 나머지는 `-32602`(188) · 실행 성공(37) · `-32000`(2, plugin 으로 forward 된 뒤의 답)
//! 이었다. 즉 작은 축이고, 표 설계 문제가 아니라 **말할 수단이 없던 한 칸**이다.
//!
//! ## 왜 텍스트로 세지 않고 이 형태인가
//!
//! 같은 집합을 "외부 라우터 소스에 이름이 안 보이는 것" 으로 세면 **실제보다 넓게
//! 잡힌다** — `window.*` · `view.*` · `ui.screenshot` 처럼 match 팔이 아니라 명부로
//! 라우팅되는 것들이 섞여 들어오기 때문이다(2026-09-05 실측으로 확인한 성질이다).
//! 몇 배인지는 적지 않는다 — 두 항이 다 커밋마다 바뀌는 값이라 그 비도 함께 낡는다. 그래서 이 가드는
//! "외부에 없다" 를 텍스트로 재지 않는다. 대신 **plugin 진입부가 실제로 인터셉트하는
//! 이름**을 뽑아 표식과 양방향 대조한다 — 그쪽은 `call.method == …` 비교라 형태가 좁다.
//!
//! 인터셉트 하나는 리터럴이 아니라 상수(`METHOD_HOST_SHARED_BUFFER_CREATE`)로 적혀
//! 있어서, 리터럴만 긁는 추출기는 그것을 **누락으로 오인**한다. 상수를 값으로 풀고,
//! 그 해석이 살아 있는지를 [`the_constant_resolution_is_alive`] 가 못 박는다.

use std::collections::BTreeSet;
use std::path::PathBuf;

use tasty_ipc::method_meta::METHOD_TABLE;

use super::{mask_non_code, repo_root};

/// plugin host-call 진입부 — 두 조합 각각의 자리.
const DISPATCH_SOURCES: &[&str] = &[
    "src/app/dispatch/plugin_ipc.rs",
    "src/boot/headless_plugins.rs",
];

/// 인터셉트가 상수로 적힌 자리를 풀기 위해 읽는 상수 정의 소스.
const PROTOCOL_CONSTS: &str = "crates/tasty-plugin-protocol/src/protocol.rs";

/// 호스트 메서드 수의 하한 — **연기 검사**다. 표가 비면 아래 대조는 빈 집합끼리라
/// 그냥 통과한다. 값의 근거: 2026-09-05 실측 276 건.
const MIN_HOST_METHODS: usize = 200;

/// `plugin_only` 표식 수의 하한. 값의 근거: 2026-09-05 실행 census 4 건.
const MIN_PLUGIN_ONLY: usize = 4;

fn read(rel: &str) -> String {
    let p: PathBuf = repo_root().join(rel);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n")
}

/// `pub const NAME: &str = "value";` 를 (NAME → value) 로 모은다.
fn string_consts(src: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in src.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("pub const ") else {
            continue;
        };
        let Some((name, tail)) = rest.split_once(':') else {
            continue;
        };
        let Some((_, value)) = tail.split_once('=') else {
            continue;
        };
        let value = value.trim().trim_end_matches(';').trim();
        let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
            continue;
        };
        out.push((name.trim().to_string(), inner.to_string()));
    }
    out
}

/// `call.method == <리터럴 | 상수경로>` 에서 비교 대상 메서드 이름을 뽑는다.
///
/// 원문에서 리터럴을 읽어야 하므로 마스킹본은 **비교의 존재 판정에만** 쓴다 —
/// 주석 안의 같은 문장에 속지 않기 위해서다.
fn intercepted(src: &str, consts: &[(String, String)]) -> BTreeSet<String> {
    let masked = mask_non_code(src);
    let mut out = BTreeSet::new();
    for (raw, code) in src.lines().zip(masked.lines()) {
        let Some(at) = code.find("call.method ==") else {
            continue;
        };
        let after_raw = &raw[at.min(raw.len())..];
        let Some(rhs) = after_raw.split_once("==").map(|(_, r)| r.trim()) else {
            continue;
        };
        if let Some(lit) = rhs.strip_prefix('"').and_then(|v| v.split('"').next()) {
            out.insert(lit.to_string());
            continue;
        }
        // 상수 경로 — 마지막 세그먼트로 해석한다.
        let ident: String = rhs
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':'))
            .next()
            .unwrap_or("")
            .rsplit("::")
            .next()
            .unwrap_or("")
            .to_string();
        if let Some((_, v)) = consts.iter().find(|(n, _)| *n == ident) {
            out.insert(v.clone());
        }
    }
    out
}

fn dispatched_names() -> BTreeSet<String> {
    let consts = string_consts(&read(PROTOCOL_CONSTS));
    let mut out = BTreeSet::new();
    for f in DISPATCH_SOURCES {
        out.extend(intercepted(&read(f), &consts));
    }
    out
}

fn marked_plugin_only() -> BTreeSet<String> {
    METHOD_TABLE
        .iter()
        .filter(|(_, m)| m.plugin_only)
        .map(|(n, _)| (*n).to_string())
        .collect()
}

/// 표식과 인터셉트가 **양방향으로** 같은 집합이다.
#[test]
fn the_plugin_only_mark_and_the_intercepts_are_the_same_set() {
    assert!(
        METHOD_TABLE.len() >= MIN_HOST_METHODS,
        "호스트 메서드가 {} 건뿐이다(하한 {MIN_HOST_METHODS}). 표가 비면 아래 대조는 \
         빈 집합끼리라 그냥 통과한다",
        METHOD_TABLE.len()
    );
    let marked = marked_plugin_only();
    assert!(
        marked.len() >= MIN_PLUGIN_ONLY,
        "`plugin_only` 표식이 {} 개뿐이다(하한 {MIN_PLUGIN_ONLY}, 2026-09-05 실행 census 4). \
         표식을 지웠다면 그 메서드의 외부 응답이 다시 `-32601` 로 돌아간 것이다",
        marked.len()
    );
    let dispatched = dispatched_names();

    let unmarked: Vec<&String> = dispatched
        .iter()
        .filter(|n| !marked.contains(*n))
        .filter(|n| METHOD_TABLE.iter().any(|(m, _)| *m == n.as_str()))
        .collect();
    assert!(
        unmarked.is_empty(),
        "plugin 진입부가 인터셉트하는데 표에 `plugin_only` 가 아니다 — 외부 호출자가 \
         `-32601`(그런 메서드 없다)을 받는다. 표식을 붙여라: {unmarked:?}"
    );

    let undispatched: Vec<&String> = marked.iter().filter(|n| !dispatched.contains(*n)).collect();
    assert!(
        undispatched.is_empty(),
        "표는 `plugin_only` 라는데 plugin 진입부에 인터셉트가 없다 — plugin 도 못 부르면 \
         그 메서드는 아무도 못 부른다: {undispatched:?}"
    );
}

/// 상수로 적힌 인터셉트를 값으로 풀고 있다.
///
/// 이 해석이 죽으면 위 대조는 `host.shared_buffer.create` 를 "표식만 있고 인터셉트
/// 없음" 으로 잘못 신고한다 — 그 형태가 **가드를 못 믿게 만드는 오탐**이다.
#[test]
fn the_constant_resolution_is_alive() {
    let consts = string_consts(&read(PROTOCOL_CONSTS));
    assert!(
        consts.iter().any(|(_, v)| v == "host.shared_buffer.create"),
        "프로토콜 상수에서 `host.shared_buffer.create` 를 못 찾았다 — 상수 정의 자리가 \
         옮겨졌으면 `PROTOCOL_CONSTS` 를 고쳐라"
    );
    let src = "if call.method == tasty_plugin_protocol::METHOD_HOST_SHARED_BUFFER_CREATE {";
    let got = intercepted(src, &consts);
    assert!(
        got.contains("host.shared_buffer.create"),
        "상수 경로를 값으로 못 풀었다: {got:?}"
    );
}

/// 추출기가 리터럴 비교도 읽고, 주석 안의 같은 문장에는 안 속는다.
#[test]
fn the_extractor_reads_literals_and_skips_comments() {
    let src = "\
// if call.method == \"ns.commented\" { }
if call.method == \"ns.real\" {
";
    let got = intercepted(src, &[]);
    assert!(got.contains("ns.real"), "리터럴 비교를 놓쳤다: {got:?}");
    assert!(
        !got.contains("ns.commented"),
        "주석 안의 비교를 집었다 — 마스킹이 안 걸렸다: {got:?}"
    );
}
