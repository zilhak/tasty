//! 에이전트가 호출하는 경로의 활성 상태 읽기를 파일·분류·횟수·사유로 기록한다.
//! 대상을 ID로 지정해야 한다는 docs/identity.md §2.3과 docs/design/policies/focus.md의 규칙을 따른다.
//! 같은 식별자만으로 상태 보고와 대상 선택을 구별할 수 없어, 분류의 타당성은 사람이 판단한다.
//! 이 검사는 미등록·개수 차이·사유 누락·OpenDefect 등록을 찾는다.
//! 주석·문자열·테스트 전용 코드는 제외한다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use tasty_doc_guards::cfg_predicate::blank_gated_lines;
use tasty_doc_guards::shipping_scope::test_only_files;
use tasty_doc_guards::source_text::{mask_non_code, rust_sources};

/// 활성 상태 식별자. active_tab은 ID로 찾은 페인의 필드일 수도 있으므로
/// 제외하지 않고 사람이 IdResolved 여부를 분류한다.
const NEEDLES: &[&str] = &[
    "active_workspace",
    "focused_window",
    "active_surface",
    "active_tab",
    "active_pane",
];

/// IPC 핸들러, 호출되는 도메인 코드와 포트 구현 파일을 검사한다.
/// 포트의 활성 상태 접근은 호출부에 드러나지 않을 수 있어 구현 파일도 필요하다(ADR-0002).
/// src/state와 src/file에는 사용자 입력 경로도 있으므로 필요한 파일만 등록한다.
/// 에이전트 경로에 포트 구현이 추가되면 이 목록도 갱신한다.
const AGENT_FACING: &[&str] = &[
    "src/adapters/ipc/handler.rs",
    "src/adapters/ipc/handler/",
    "src/app/dispatch/",
    "src/app/dispatch_domain.rs",
    "src/core/structural_cascade.rs",
    "src/core/structural_exec.rs",
    "src/file/identify_worker.rs",
    "src/state/cascade_window.rs",
    "src/state/ipc_window.rs",
];

/// 파일 단위 항목은 부모 디렉터리를 순회하고 is_agent_facing에서 거른다.
const SCAN_ROOTS: &[&str] = &[
    "src/adapters/ipc",
    "src/app",
    "src/core",
    "src/file",
    "src/state",
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// 응답에 "무엇이 활성인지" 를 싣는다. 대상 선택이 아니라 상태 보고다.
    Report,
    /// 호출자가 준 ID로 찾은 객체의 속성이다. 전역 포커스로 대상을 고르지 않는다.
    IdResolved,
    /// 권한·상한 정책을 적용할 워크스페이스다. 요청 대상을 고르는 값은 아니다.
    PolicyScope,
    /// 기록·알림·승인의 기본 워크스페이스. 호출자가 생략하면 활성 워크스페이스를 쓴다.
    Attribution,
    /// debug 전용 파일로 release 빌드에서 제외된다.
    DebugOnly,
    /// **사용자 기원** 경로가 활성 포인터를 갱신한다. 에이전트 행동이 아니다.
    UserOrigin,
    /// 워크스페이스가 0 이 된 뒤의 복구 — 활성 포인터를 **다시 만든다**.
    Recovery,
    /// 구조분해에서 `_` 로 버린다. 읽기가 아니다.
    PatternOnly,
    /// focused가 없으면 다른 창을 써도 된다고 소스에 설명된 경우다.
    AnyWindow,
    /// 활성 상태로 대상을 고르는 결함. 해결을 막는 이유를 적는다.
    OpenDefect,
}
use Kind::*;

/// (파일, 분류, 해당 분류의 출현 수, 사유).
/// 줄 번호는 무관한 편집에도 바뀌므로 파일별로 집계하고 변경 시 분류를 다시 검토한다.
const ROSTER: &[(&str, Kind, usize, &str)] = &[
    (
        "src/adapters/ipc/handler/tab.rs",
        Report,
        3,
        "탭 목록의 \"active\" 플래그와 TabCreated 이벤트가 실어 온 active_tab 을 응답에 그대로 싣는다",
    ),
    (
        "src/core/structural_exec.rs",
        IdResolved,
        2,
        "탭 생성과 split 의 cwd 상속 원본을 고를 때 호출자가 준 pane_id(split 은 resolved_pane_id)로 푼 페인의 활성 탭을 본다 — 전역 포커스가 아니다",
    ),
    (
        "src/core/structural_exec.rs",
        Report,
        3,
        "TabCreated 이벤트가 실어 온 active_tab 을 결과 값(TabCreated)에 옮겨 담는다 — 핸들러가 그것을 응답에 싣는다",
    ),
    (
        "src/adapters/ipc/handler/workspace.rs",
        Report,
        1,
        "워크스페이스 목록에서 어느 것이 활성인지를 플래그로 알린다",
    ),
    (
        "src/adapters/ipc/handler/checked.rs",
        PolicyScope,
        1,
        "permission·cap 게이트에 넘길 workspace_id — 요청 대상을 고르는 값이 아니라 정책을 귀속시킬 스코프다",
    ),
    (
        "src/adapters/ipc/handler.rs",
        Report,
        5,
        "system_info의 활성 index 선언·응답·ID 해소와 워크스페이스 표의 활성 플래그",
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
        "승인 요청에 workspace_id와 surface_id가 모두 없을 때만 활성 워크스페이스를 쓴다. surface_id가 있으면 해당 서피스의 워크스페이스를 쓴다(ADR-0017).",
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
        "상한 계산에서 사용량을 집계할 워크스페이스를 정한다. 요청의 실행 대상은 바꾸지 않는다.",
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
        "debug 전용 핸들러 파일이므로 release 빌드의 검사 대상에서 제외된다.",
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
        3,
        "닫은 항목 복원·워크스페이스 이동 — 전부 origin 이 User 일 때만 도는 cascade 다",
    ),
    (
        "src/core/structural_cascade.rs",
        UserOrigin,
        1,
        "surface split 뒤 포커스 서피스 갱신 — origin 이 User 일 때만 돈다(에이전트 split 은 앞에서 return)",
    ),
    (
        "src/core/structural_cascade.rs",
        Recovery,
        1,
        "마지막 워크스페이스가 닫힌 뒤 기본 워크스페이스를 다시 만들고 그 인덱스를 활성으로 둔다 — 활성이 없는 상태를 안 남기는 것",
    ),
    (
        "src/state/cascade_window.rs",
        Recovery,
        2,
        "포트 메서드 set_active_workspace 의 구현 이름과 그 몸체의 대입 — 도메인의 호출은 structural_cascade 의 Recovery 한 자리뿐이다(마지막 워크스페이스가 닫힌 뒤 되만든 기본 워크스페이스를 활성으로 둔다)",
    ),
    (
        "src/state/ipc_window.rs",
        Attribution,
        2,
        "포트 메서드 active_workspace_index 의 구현 이름과 그 몸체의 읽기 — 핸들러 호출 자리의 분류를 물려받는다. 호출 자리 11 곳은 Attribution 7 · Report 3 · PolicyScope 1 로 명부에 있고 대상을 고르는 자리는 0 이다; 한 구현이 여러 갈래로 나뉠 수 없어 가장 많은 Attribution 에 둔다",
    ),
    (
        "src/app/dispatch/intents.rs",
        AnyWindow,
        1,
        "appearance 의 단일 출처를 고른다. focused 가 없으면 아무 main 이든 된다고 소스 주석이 밝히므로 결과가 포커스에 안 걸린다",
    ),
];

/// 수집 결과가 비어 명부 대조만 통과하는 경우를 막는다.
const MIN_FILES_SCANNED: usize = 60;
/// 같은 이유의 출현 하한. 2026-09-07 실측 33.
const MIN_OCCURRENCES: usize = 20;

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop();
    p.pop();
    p
}

/// 공유 파서로 주석·문자열과 test 조건부 구간을 제외한다.
fn shipped_code(src: &str) -> String {
    blank_gated_lines(&mask_non_code(src), "test")
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

/// (스캔한 파일 수, 파일별 출하 코드의 출현 수).
/// 파일 내부 test 구간과 test 모듈로만 선언된 파일을 모두 제외한다.
fn measure() -> (usize, BTreeMap<String, usize>) {
    let root = repo_root();
    let sources = rust_sources(&root, SCAN_ROOTS);
    let scanned = sources.len();
    let test_only = test_only_files(&root, &sources);
    let mut found = BTreeMap::new();
    for (rel, text) in &sources {
        if test_only.contains(rel) {
            continue;
        }
        let rel = rel.to_string_lossy().into_owned();
        if !is_agent_facing(&rel) {
            continue;
        }
        let n = count_needles(&shipped_code(text));
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
        "스캔 루트 {SCAN_ROOTS:?}에서 .rs 파일을 {scanned}개만 수집했다. 하한을 낮추기 전에 경로와 수집 범위를 확인한다."
    );
    let total: usize = found.values().sum();
    assert!(
        total >= MIN_OCCURRENCES,
        "활성 상태 읽기를 {total}개만 찾았다(2026-09-07 실측 33). 마스킹과 cfg 제외가 과도한지 먼저 확인한다. 실제로 줄었다면 명부와 하한을 함께 갱신하고 측정 근거를 남긴다."
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
        "에이전트 경로의 활성 상태 읽기가 명부와 다르다.\n{}\n새 읽기는 분류와 사유를 검토해 ROSTER에 등록한다. 활성 상태로 대상을 고르면 OpenDefect로 기록한다.",
        bad.join("\n")
    );
}

/// 출현 수가 0인 경로도 명부에서 빠지지 않도록 AGENT_FACING 항목의 존재를 확인한다.
#[test]
fn every_agent_facing_entry_exists() {
    let root = repo_root();
    let scanned: Vec<String> = rust_sources(&root, SCAN_ROOTS)
        .into_iter()
        .map(|(rel, _)| rel.to_string_lossy().replace('\\', "/"))
        .collect();
    let missing: Vec<&&str> = AGENT_FACING
        .iter()
        .filter(|p| {
            !scanned
                .iter()
                .any(|r| r == **p || (p.ends_with('/') && r.starts_with(**p)))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "AGENT_FACING 항목이 스캔 루트 {SCAN_ROOTS:?}에 없다. 파일이 이동했다면 항목을 삭제하지 말고 새 경로로 고친다.\n  {missing:?}"
    );
}

#[test]
fn the_roster_has_no_file_the_tree_does_not_have() {
    let (_, found) = measure();
    let listed: BTreeSet<&str> = ROSTER.iter().map(|(p, _, _, _)| *p).collect();
    let stale: Vec<&&str> = listed.iter().filter(|p| !found.contains_key(**p)).collect();
    assert!(
        stale.is_empty(),
        "명부에 등록됐지만 해당 읽기가 없는 파일이다. 코드 변경을 확인하고 오래된 항목을 제거한다:\n  {stale:?}"
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
        "활성 상태로 대상을 고르는 OpenDefect가 등록됐다. docs/identity.md §2.3에 따라 대상을 ID로 지정하도록 수정해야 한다."
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
        "같은 파일의 서로 다른 분류에 동일한 사유가 쓰였다. 각 분류의 근거를 따로 설명한다:\n{}",
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
        "분류 사유가 너무 짧다. 코드에서 판단한 근거를 설명한다:\n{}",
        thin.join("\n")
    );
}

/// 출하 코드만 세는지 합성 입력의 원문과 마스킹 결과를 비교한다.
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
    // 원문에는 주석·문자열·테스트를 포함해 네 번 나타난다.
    assert_eq!(
        count_needles(fixture),
        4,
        "합성 원문에서 예상한 출현 수가 달라져 마스킹 결과와 비교할 수 없다"
    );
}
