//! 에이전트 대면 경로가 **전역 활성/포커스 포인터**를 읽는 자리를 전수 분류한다.
//!
//! 지키는 것은 `docs/identity.md` §2.3(포커스 독립성)과
//! [`docs/design/policies/focus.md`](../../../docs/design/policies/focus.md) 이다 —
//! "모든 명령은 대상을 ID 로 직접 지정, 활성 상태 의존 동작 금지".
//!
//! ## 이 가드가 **안 묻는 것** — 그리고 왜 그래도 값이 있는가
//!
//! 물어야 할 진짜 물음은 "이 읽기는 **보고**인가 **선택**인가" 다. 그런데 둘이 **같은
//! 식별자**를 쓴다 — `"active": i == state.active_workspace` 는 합법이고
//! `engine.workspaces[state.active_workspace]` 로 대상을 고르는 것은 위반인데, 텍스트로는
//! 안 갈린다. 그래서 이 축은 오래 `[구두]` 로 남아 있었다.
//!
//! **그 판단은 물음을 안 가른 것이었다.** 규칙 옆에 **배치 물음**이 붙어 있다:
//!
//! - 의미 물음 — "보고인가 선택인가". 사람이 판정한다. 여기서 **안 묻는다**.
//! - 배치 물음 — "그 자리가 **갈래·사유와 함께 명부에 적혀 있는가**". 판정된다.
//!
//! 같은 갈림이 `debug_handlers_live_in_cfg_declared_modules` 에서 먼저 풀렸고, 같은
//! 명부 형태가 `window_owned_lists_are_classified` 에서 이미 두 번째로 선다.
//!
//! ## 무엇이 잡히는가
//!
//! 새 읽기가 에이전트 대면 경로에 들어오면 명부에 없어서 빨갛다. 그때 사람이 갈래를
//! 골라야 하고, 고르는 순간 **그 자리가 무엇인지 적힌다.** 지금 `OpenDefect` 가 0 이라는
//! 사실도 이 명부가 있어야 다음 사람이 믿을 수 있다 — 0 은 안 세면 언제나 참이다.
//!
//! ## 모수 — 세 단계로 좁혔고 각 단계의 값을 적는다
//!
//! 2026-09-07 실측, **출현 단위**(줄이 아니다 — `let active_tab = … p.active_tab` 처럼 한
//! 줄에 둘인 자리가 있다). 저장소 전체로 세면 **922 출현**이고 그 수가 "값싸게는 못
//! 만든다" 의 근거였다. 그런데 이 원칙이 말하는 것은 **에이전트가 부르는 경로**다:
//!
//! | 모수 | 출현 |
//! |---|---|
//! | 저장소 전체 | 922 |
//! | 에이전트 대면 경로 원문 | 55 |
//! | + 주석·문자열 마스킹 | 41 |
//! | + `#[cfg(test)]` 제거 | **33** |
//!
//! 마지막 단계가 특히 크다 — `handler/webview.rs` 의 셋과 `handler/workspace.rs` 의 둘은
//! **테스트 헬퍼**인데 눈으로는 위반처럼 보인다(`expect` 를 쓰고 id 해석이 없다). 마스킹만
//! 걸고 읽으면 없는 결함을 셋 본다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use tasty_doc_guards::cfg_predicate::{cfg_attr_lines, cfg_gated_lines};
use tasty_doc_guards::source_text::{mask_non_code, rust_sources};

/// 전역 활성/포커스 포인터의 이름.
///
/// `active_tab` 은 페인의 **필드**이기도 하다 — 그래서 명부에 `IdResolved` 갈래가 있다.
/// 바늘에서 빼면 "ID 로 푼 페인의 활성 탭" 과 "전역 활성 탭" 을 **바늘 단계에서** 가르는
/// 셈이 되는데, 그 구분이야말로 사람이 판정할 것이라 여기서 미리 갈라 두면 안 된다.
const NEEDLES: &[&str] = &[
    "active_workspace",
    "focused_window",
    "active_surface",
    "active_tab",
    "active_pane",
];

/// 에이전트 대면 경로. IPC 핸들러와 그것이 부르는 도메인 cascade.
const AGENT_FACING: &[&str] = &[
    "src/adapters/ipc/handler.rs",
    "src/adapters/ipc/handler/",
    "src/app/dispatch/",
    "src/app/dispatch_domain.rs",
];

/// 스캔 루트 — 위 접두사를 담는 가장 작은 디렉토리들.
const SCAN_ROOTS: &[&str] = &["src/adapters/ipc", "src/app"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// 응답에 "무엇이 활성인지" 를 싣는다. 대상 선택이 아니라 상태 보고다.
    Report,
    /// **ID 로 해석한 객체의 자기 속성.** `find_pane_by_id(id) … .active_tab` 은 전역
    /// 포커스를 안 읽는다 — 호출자가 준 id 로 푼 페인이 자기 활성 탭을 아는 것뿐이다.
    IdResolved,
    /// 권한·상한 게이트가 정책을 **귀속**시킬 워크스페이스. 대상 선택이 아니다.
    PolicyScope,
    /// 기록·알림·승인의 **기본 워크스페이스 귀속**. 호출자가 안 주면 활성으로 채운다.
    Attribution,
    /// debug 격리 파일. 원칙 1 로 이 축의 범위 밖이다.
    DebugOnly,
    /// **사용자 기원** 경로가 활성 포인터를 갱신한다. 에이전트 행동이 아니다.
    UserOrigin,
    /// 워크스페이스가 0 이 된 뒤의 복구 — 활성 포인터를 **다시 만든다**.
    Recovery,
    /// 구조분해에서 `_` 로 버린다. 읽기가 아니다.
    PatternOnly,
    /// focused 가 있으면 쓰고 없으면 아무 창 — **결과가 focus 에 안 걸린다**고 소스가 밝힌다.
    AnyWindow,
    /// **아직 활성 상태로 대상을 고르는 자리 — 열린 결함.** 사유 칸에 무엇이 막고 있는지 적는다.
    OpenDefect,
}
use Kind::*;

/// (파일, 갈래, 그 파일에서 그 갈래인 출현 수, 사유).
///
/// 좌표를 **줄 번호로 안 잡는다** — 줄은 무관한 편집에도 밀리고, 밀린 명부를 맞추는
/// 가장 싼 길은 수를 다시 세는 것이라 판정을 안 거친다. 파일+갈래+수 는 그 셋이 함께
/// 움직일 때만 갱신되고, 그때는 사람이 무엇이 변했는지 봐야 한다.
const ROSTER: &[(&str, Kind, usize, &str)] = &[
    (
        "src/adapters/ipc/handler/tab.rs",
        Report,
        3,
        "탭 목록의 \"active\" 플래그와 TabCreated 이벤트가 실어 온 active_tab 을 응답에 그대로 싣는다",
    ),
    (
        "src/adapters/ipc/handler/tab.rs",
        IdResolved,
        1,
        "cwd 상속 원본을 고를 때 호출자가 준 pane_id 로 푼 페인의 활성 탭을 본다 — 전역 포커스가 아니다",
    ),
    (
        "src/adapters/ipc/handler/pane.rs",
        IdResolved,
        1,
        "위와 같은 cwd 상속인데 페인을 resolved_pane_id 로 먼저 푼다는 점도 같다",
    ),
    (
        "src/adapters/ipc/handler/workspace.rs",
        Report,
        1,
        "워크스페이스 목록에서 어느 것이 활성인지를 플래그로 알린다",
    ),
    (
        "src/adapters/ipc/handler.rs",
        PolicyScope,
        1,
        "permission·cap 게이트에 넘길 workspace_id — 요청 대상을 고르는 값이 아니라 정책을 귀속시킬 스코프다",
    ),
    (
        "src/adapters/ipc/handler.rs",
        Report,
        2,
        "debug 상태 덤프의 active_workspace 필드와 워크스페이스 표의 \"active\" 플래그",
    ),
    (
        "src/adapters/ipc/handler/approval.rs",
        Attribution,
        1,
        "승인 요청을 어느 워크스페이스 것으로 기록할지 — 승인 대상은 이미 파라미터로 정해져 있다",
    ),
    (
        "src/adapters/ipc/handler/approval/request.rs",
        Attribution,
        1,
        "승인 요청 생성 쪽의 같은 귀속. approval.rs 와 파일이 달라 사유를 따로 적는다",
    ),
    (
        "src/adapters/ipc/handler/telemetry/record.rs",
        Attribution,
        2,
        "호출자가 ws 를 안 주면 채울 기본값 — 이벤트를 어디 것으로 셀지의 문제다",
    ),
    (
        "src/adapters/ipc/handler/telemetry/anomaly.rs",
        Attribution,
        1,
        "이상 판정이 볼 워크스페이스 스코프. 대상 선택이 아니라 표본의 귀속이다",
    ),
    (
        "src/adapters/ipc/handler/telemetry/cap.rs",
        Attribution,
        2,
        "상한 판정의 귀속 워크스페이스 — 상한을 **어느 창에 물릴지**가 아니라 어느 창 몫으로 셀지다",
    ),
    (
        "src/adapters/ipc/handler/debug_state.rs",
        DebugOnly,
        5,
        "debug 상태 덤프 전용 파일. 원칙 1 로 release 에 없다",
    ),
    (
        "src/adapters/ipc/handler/debug.rs",
        DebugOnly,
        1,
        "debug 핸들러 파일. 위와 같은 이유로 범위 밖이고 파일이 달라 따로 적는다",
    ),
    (
        "src/app/dispatch_domain.rs",
        PatternOnly,
        1,
        "TabCreated 를 구조분해하며 active_tab 을 `_` 로 버린다 — 값을 읽지 않는다",
    ),
    (
        "src/app/dispatch_domain.rs",
        Attribution,
        2,
        "알림을 밀어 넣을 때 어느 워크스페이스 알림인지를 채운다",
    ),
    (
        "src/app/dispatch_domain.rs",
        UserOrigin,
        4,
        "닫은 항목 복원·워크스페이스 이동·포커스 서피스 갱신 — 전부 origin 이 User 일 때만 도는 cascade 다",
    ),
    (
        "src/app/dispatch_domain.rs",
        Recovery,
        1,
        "마지막 워크스페이스가 닫힌 뒤 기본 워크스페이스를 다시 만들고 그 인덱스를 활성으로 둔다 — 활성이 없는 상태를 안 남기는 것",
    ),
    (
        "src/app/dispatch/intents.rs",
        AnyWindow,
        1,
        "appearance 의 단일 출처를 고른다. focused 가 없으면 아무 main 이든 된다고 소스 주석이 밝히므로 결과가 포커스에 안 걸린다",
    ),
    (
        "src/app/dispatch/intents.rs",
        Attribution,
        2,
        "audit 기록에 실을 워크스페이스 id — 없으면 거부는 그대로 남기고 기록만 못 남긴다",
    ),
];

/// 이 모수에서 파일이 이 수 아래로 떨어지면 걷기가 깨진 것이다.
///
/// 하한이 아니라 **모수 붕괴 탐지**다 — 명부 대조는 양쪽이 비면 공짜로 성립한다.
const MIN_FILES_SCANNED: usize = 60;
/// 같은 이유의 출현 하한. 2026-09-07 실측 33.
const MIN_OCCURRENCES: usize = 20;

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // crates/tasty-doc-guards → 레포 루트.
    p.pop();
    p.pop();
    p
}

/// 출하되는 코드만 남긴 사본. 주석·문자열·`#[cfg(test)]`·`cfg_attr(test, …)` 를 뺀다.
fn shipped_code(src: &str) -> String {
    let masked = mask_non_code(src);
    let lines: Vec<&str> = masked.split('\n').collect();
    let gated = cfg_gated_lines(&lines, "test");
    let attrs = cfg_attr_lines(&lines, "test");
    let mut out = String::with_capacity(masked.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if !gated[i] && !attrs[i] {
            out.push_str(line);
        }
    }
    out
}

fn is_agent_facing(rel: &str) -> bool {
    AGENT_FACING
        .iter()
        .any(|p| rel == *p || (p.ends_with('/') && rel.starts_with(p)))
}

fn count_needles(text: &str) -> usize {
    NEEDLES
        .iter()
        .map(|n| text.matches(n).count())
        .sum::<usize>()
}

/// (스캔한 파일 수, 파일별 출하 출현 수).
fn measure() -> (usize, BTreeMap<String, usize>) {
    let root = repo_root();
    let sources = rust_sources(&root, SCAN_ROOTS);
    let scanned = sources.len();
    let mut found = BTreeMap::new();
    for (rel, text) in sources {
        let rel = rel.to_string_lossy().into_owned();
        if !is_agent_facing(&rel) {
            continue;
        }
        let n = count_needles(&shipped_code(&text));
        if n > 0 {
            found.insert(rel, n);
        }
    }
    (scanned, found)
}

#[test]
fn the_population_did_not_collapse() {
    let (scanned, found) = measure();
    assert!(
        scanned >= MIN_FILES_SCANNED,
        "스캔 루트 {SCAN_ROOTS:?} 에서 .rs 를 {scanned} 개만 걷었다 — 2026-09-07 실측은 훨씬 크다. \
         걷기가 깨지면 아래 대조는 양쪽이 비어 공짜로 성립한다.\n\
         ★ 이 하한을 내려서 통과시키지 마라 — 이 값이 막는 사고는 하나뿐이고, 내리면 그 하나가 사라진다."
    );
    let total: usize = found.values().sum();
    assert!(
        total >= MIN_OCCURRENCES,
        "에이전트 대면 경로에서 활성 상태 읽기를 {total} 개만 찾았다 (2026-09-07 실측 33). \
         정말 줄었으면 명부와 이 하한을 함께 내리고 근거 날짜를 갱신하라 — 다만 **먼저 의심할 것은 \
         마스킹·cfg 제거가 너무 많이 지운 것**이다."
    );
}

#[test]
fn every_occurrence_is_registered() {
    let (_, found) = measure();
    let mut registered: BTreeMap<&str, usize> = BTreeMap::new();
    for (path, _, count, _) in ROSTER {
        *registered.entry(path).or_default() += count;
    }
    let mut bad = Vec::new();
    for (path, n) in &found {
        let r = registered.get(path.as_str()).copied().unwrap_or(0);
        if r != *n {
            bad.push(format!("  {path}: 실측 {n} · 명부 {r}"));
        }
    }
    assert!(
        bad.is_empty(),
        "에이전트 대면 경로의 활성 상태 읽기가 명부와 안 맞는다.\n{}\n\
         새 읽기가 들어왔으면 **갈래를 고르고 사유를 적어** ROSTER 에 넣어라. 갈래를 고르는 \
         그 순간이 이 가드가 사려는 것이다 — 수만 맞추면 아무것도 판정되지 않는다.\n\
         ★ 대상을 고르는 자리라면 갈래는 `OpenDefect` 다. 그 갈래로 적으면 아래 시험이 수를 \
         묻고, 그것이 이 저장소가 그 결함을 아는 유일한 방법이 된다.",
        bad.join("\n")
    );
}

#[test]
fn the_roster_has_no_file_the_tree_does_not_have() {
    let (_, found) = measure();
    let listed: BTreeSet<&str> = ROSTER.iter().map(|(p, _, _, _)| *p).collect();
    let stale: Vec<&&str> = listed.iter().filter(|p| !found.contains_key(**p)).collect();
    assert!(
        stale.is_empty(),
        "명부에 있는데 트리에 그 읽기가 없는 파일이다. 사라진 자리의 잔재는 다음 사람이 \
         계속 검토하게 만든다 — 지워라:\n  {stale:?}"
    );
}

#[test]
fn the_open_ones_are_not_silently_emptied() {
    let open: usize = ROSTER
        .iter()
        .filter(|(_, k, _, _)| *k == OpenDefect)
        .map(|(_, _, c, _)| c)
        .sum();
    assert_eq!(
        open, 0,
        "활성 상태로 **대상을 고르는** 자리의 수가 바뀌었다. 늘었으면 그것이 원칙 2.3 위반이고 \
         이 가드가 잡으려던 것이다. 고쳐서 0 이 됐으면 이 수는 이미 0 이라 여기 안 걸린다 — \
         걸렸다는 것은 늘었다는 뜻이다."
    );
}

#[test]
fn rows_in_the_same_file_do_not_share_evidence() {
    let mut seen: BTreeMap<(&str, &str), usize> = BTreeMap::new();
    for (path, _, _, why) in ROSTER {
        *seen.entry((path, why)).or_default() += 1;
    }
    let dupes: Vec<String> = seen
        .iter()
        .filter(|(_, n)| **n > 1)
        .map(|((p, w), n)| format!("  {p} ×{n}: {w}"))
        .collect();
    assert!(
        dupes.is_empty(),
        "같은 파일의 두 행이 **같은 사유**를 쓴다. 갈래가 둘인데 사유가 하나면 둘 중 하나는 \
         복사된 것이고, 복사된 사유는 판정을 안 거친 표시다:\n{}\n\
         ☆ 파일이 다르면 사유가 비슷해도 된다 — 두 파일이 같은 이유로 같은 일을 할 수 있다.",
        dupes.join("\n")
    );
}

#[test]
fn every_row_carries_a_reason() {
    let thin: Vec<String> = ROSTER
        .iter()
        .filter(|(_, _, _, why)| why.split_whitespace().count() < 6)
        .map(|(p, k, _, w)| format!("  {p} {k:?}: {w}"))
        .collect();
    assert!(
        thin.is_empty(),
        "사유가 너무 짧다. 갈래 이름은 사유가 아니다 — 다음 사람이 그 판정을 **재현**할 수 \
         있어야 한다:\n{}",
        thin.join("\n")
    );
}

/// 추출기의 극성 — 무엇을 세고 무엇을 안 세는가.
///
/// 이 픽스처가 없으면 위 대조들은 "추출기가 아무것도 안 센다" 여도 명부를 함께 비우는
/// 순간 통과한다. 특히 `cfg(test)` 제거는 **없는 결함 셋을 만들었다가 지운** 단계라
/// (모듈 머리말 참조) 그 동작을 여기서 단정해 둔다.
#[test]
fn the_extractor_counts_shipped_code_only() {
    let fixture = concat!(
        "fn shipped() { let a = state.active_workspace; }\n",
        "// active_workspace in a comment\n",
        "fn s2() { let m = \"active_workspace in a string\"; }\n",
        "#[cfg(test)]\n",
        "mod tests {\n",
        "    fn helper() { let b = state.active_workspace; }\n",
        "}\n",
    );
    let n = count_needles(&shipped_code(fixture));
    assert_eq!(
        n, 1,
        "출하 코드의 한 건만 세야 한다 — 주석·문자열·cfg(test) 안의 셋은 빼고. 실제 {n}"
    );
    // 반대 극성: 마스킹 없이 원문을 세면 넷이다. 이 값이 같아지면 마스킹이 죽은 것이다.
    assert_eq!(
        count_needles(fixture),
        4,
        "픽스처 자체가 바뀌었다 — 극성 대조가 성립하지 않는다"
    );
}
