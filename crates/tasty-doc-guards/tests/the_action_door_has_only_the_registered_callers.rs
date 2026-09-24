//! dispatch_action_by_id를 쓰는 파일과 팔레트 pending_run에 쓰는 파일을 등록 목록과 대조한다.
//! 새 호출 경로는 사용자 행동과 에이전트 행동의 분리·포커스 독립성을 사람이 검토해야 한다.
//! 이 검사는 그 원칙의 충족 여부나 각 액션의 에이전트 제공 여부를 판단하지 않는다.
//!
//! 지연 실행은 pending_run에서 꺼내 처리하므로 직접 호출 외에 이 필드의 쓰기도 확인한다.
//! 디스패치의 match 항목은 KeybindingSettings에 있어야 한다. 반대로 모든 설정 필드가
//! 이 함수에서 처리되는 것은 아니므로 역방향 일치는 요구하지 않는다.

// 이유: 테스트의 임시 파일 정리 실패는 무시하며 제품 코드의 값 무시 lint 목록에서 제외한다.
#![allow(clippy::let_underscore_must_use)]

use std::collections::BTreeSet;
use std::path::PathBuf;

use tasty_doc_guards::match_arms::{Source, matching_close};
use tasty_doc_guards::source_text::{mask_non_code, rust_sources};
use tasty_doc_guards::temp_scratch::Scratch;

const DOOR: &str = "dispatch_action_by_id";
const DOOR_FILE: &str = "src/adapters/ui/input/shortcuts/dispatch.rs";
/// 지연 실행 필드의 쓰기만 찾는다. take 읽기는 직접 호출 경로에 포함돼 있다.
const PALETTE_SLOT: &str = "pending_run =";
const KEYBINDINGS: &str = "crates/tasty-settings/src/keybindings.rs";

/// 직접 호출 파일과 허용 근거.
const DOOR_CALLERS: &[(&str, &str)] = &[(
    "src/view/main/redraw.rs",
    "Command Palette에서 고른 액션을 팝업 렌더링과 닫기 처리가 끝난 뒤 실행한다. 값은 state.command_palette.pending_run에서만 가져온다.",
)];

/// 팔레트의 지연 실행 필드에 쓰는 파일과 근거.
const SLOT_WRITERS: &[(&str, &str)] = &[(
    "src/adapters/ui/popup/command_palette.rs",
    "팝업에서 Enter 로 고른 항목을 슬롯에 넣는다. 사람이 팝업을 열어 고른 것이라 \
     사용자 입력 경로이고, release 에서 에이전트는 팝업을 강제로 못 연다(원칙 1)",
)];

/// 빈 순회가 목록 비교를 통과하지 않게 한다.
const MIN_SOURCES: usize = 300;
/// 액션 필드 하한. 2026-09-07 실측 71.
const MIN_FIELDS: usize = 50;
/// 디스패치 match 항목 하한. 2026-09-07 측정 42개.
const MIN_ARMS: usize = 30;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn sources() -> Vec<(PathBuf, String)> {
    sources_under(&repo_root(), MIN_SOURCES)
}

/// 작은 합성 트리도 같은 순회로 검사할 수 있게 루트와 하한을 인자로 받는다.
fn sources_under(root: &std::path::Path, floor: usize) -> Vec<(PathBuf, String)> {
    let out = rust_sources(root, &["src"]);
    assert!(
        out.len() >= floor,
        "src에서 Rust 파일을 {}개만 수집했다(하한 {floor}). 경로와 수집 범위를 확인한다.",
        out.len()
    );
    out
}

/// 주석·리터럴을 지운 코드에서 이름을 찾고 정의 파일은 제외한다.
fn code_uses(needle: &str, own: &str) -> BTreeSet<String> {
    code_uses_in(&sources(), needle, own)
}

fn code_uses_in(sources: &[(PathBuf, String)], needle: &str, own: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for (rel, text) in sources {
        let rel = rel.to_string_lossy().into_owned();
        if rel == own {
            continue;
        }
        if mask_non_code(text).contains(needle) {
            out.insert(rel);
        }
    }
    out
}

fn check(found: BTreeSet<String>, roster: &[(&str, &str)], what: &str, why_it_matters: &str) {
    let listed: BTreeSet<String> = roster.iter().map(|(p, _)| (*p).to_string()).collect();
    let extra: Vec<&String> = found.difference(&listed).collect();
    let stale: Vec<&String> = listed.difference(&found).collect();
    assert!(
        extra.is_empty(),
        "{what}에 미등록 파일이 있다: {extra:?}\n{why_it_matters}\n등록 전에 docs/identity.md의 사용자·에이전트 행동 분리와 포커스 독립성 원칙을 검토한다. 이 검사는 원칙 충족 여부를 판단하지 않는다."
    );
    assert!(
        stale.is_empty(),
        "등록돼 있지만 코드에서 찾지 못한 {what} 파일이다. 이동·삭제를 확인해 목록을 갱신한다: {stale:?}"
    );
}

#[test]
fn the_action_door_has_only_the_registered_callers() {
    check(
        code_uses(DOOR, DOOR_FILE),
        DOOR_CALLERS,
        "액션 디스패치 호출 파일",
        "이 함수가 지원하는 사용자 액션을 ID로 실행할 수 있다. 에이전트가 사용자 입력을 재현하는 호출 경로인지 확인한다.",
    );
}

#[test]
fn the_palette_slot_has_only_the_registered_writers() {
    check(
        code_uses(PALETTE_SLOT, "src/state/command_palette.rs"),
        SLOT_WRITERS,
        "팔레트 지연 실행 필드의 쓰기 파일",
        "필드에 쓰면 팝업 렌더링과 닫기 처리 뒤 액션이 실행되므로 직접 호출과 함께 검토해야 한다.",
    );
}

/// 설정 필드를 소스에서 읽어 디스패치 match 항목이 모두 그 안에 있는지 확인한다. 역방향은 요구하지 않는다.
#[test]
fn the_action_set_is_derived_not_written() {
    let root = repo_root();
    let fields = keybinding_fields(&root);
    let arms = door_arms(&root);
    assert!(
        fields.len() >= MIN_FIELDS,
        "`KeybindingSettings` 필드를 {} 개만 뽑았다 (2026-09-07 실측 71) — 추출이 깨졌다",
        fields.len()
    );
    assert!(
        arms.len() >= MIN_ARMS,
        "디스패치 match 항목을 {}개만 읽었다(2026-09-07 측정 42개). 함수 형태와 파서를 확인한다.",
        arms.len()
    );
    let unknown: Vec<&String> = arms.difference(&fields).collect();
    assert!(
        unknown.is_empty(),
        "디스패치에 KeybindingSettings에 없는 ID가 있다: {unknown:?}. 이름 오타나 설정 필드 삭제를 확인한다."
    );
}

fn keybinding_fields(root: &std::path::Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(root.join(KEYBINDINGS))
        .unwrap_or_else(|e| panic!("{KEYBINDINGS} 를 읽지 못했다 — {e}"));
    let body = brace_body(&text, "pub struct KeybindingSettings")
        .expect("`KeybindingSettings` 선언을 못 찾았다 — 구조가 바뀌었으면 이 가드를 옮겨라");
    let mut out = BTreeSet::new();
    for line in body.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("pub ")
            && let Some(name) = rest.split(':').next()
            && !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            out.insert(name.to_string());
        }
    }
    out
}

/// 공용 파서로 함수·match 본문을 읽어 문자열의 중괄호에 잘리지 않게 한다.
/// cfg별 함수 정의가 여러 개면 모두 확인한다.
fn door_arms(root: &std::path::Path) -> BTreeSet<String> {
    let text = std::fs::read_to_string(root.join(DOOR_FILE))
        .unwrap_or_else(|e| panic!("{DOOR_FILE} 를 읽지 못했다 — {e}"));
    let src = Source::new(&text);
    let bodies = src.fn_bodies("dispatch_action_by_id");
    assert!(
        !bodies.is_empty(),
        "dispatch_action_by_id 정의를 찾지 못했다. 이동·개명을 확인한다."
    );
    let mut out = BTreeSet::new();
    for body in bodies {
        let block = src
            .code_slice(&body)
            .find("match action_id")
            .and_then(|k| {
                src.code[body.start + k..]
                    .find('{')
                    .map(|o| body.start + k + o)
            })
            .and_then(|open| matching_close(&src.code, open).map(|close| open..close + 1))
            .expect("`match action_id` 를 못 찾았다 — 디스패치 모양이 바뀌었다");
        let arms = src
            .match_arms(block)
            .unwrap_or_else(|e| panic!("액션 디스패치의 match 항목을 읽지 못했다: {e}"));
        for arm in arms {
            for alt in src.alternatives(&arm.pattern) {
                if let Some(name) = src.plain_string(&alt)
                    && !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                {
                    out.insert(name.to_string());
                }
            }
        }
    }
    out
}

/// `marker` 뒤 첫 `{` 부터 짝이 맞는 `}` 까지 — 짝은 주석·리터럴을 덮은 사본에서 센다.
fn brace_body<'a>(src: &'a str, marker: &str) -> Option<&'a str> {
    let source = Source::new(src);
    let at = source.code.find(marker)?;
    let open = at + source.code[at..].find('{')?;
    let close = matching_close(&source.code, open)?;
    Some(&src[open + 1..close])
}

#[test]
fn every_registered_place_carries_a_reason() {
    let thin: Vec<&str> = DOOR_CALLERS
        .iter()
        .chain(SLOT_WRITERS.iter())
        .filter(|(_, why)| why.split_whitespace().count() < 8)
        .map(|(p, _)| *p)
        .collect();
    assert!(
        thin.is_empty(),
        "호출을 허용한 근거가 너무 짧다. 해당 경로를 검토할 수 있도록 이유를 적는다: {thin:?}"
    );
}

/// 합성 코드에서 주석·리터럴·정의 파일을 호출로 세지 않는지 확인한다. 실제 검사 상수와 독립된 이름을 쓴다.
#[test]
fn the_predicate_counts_code_and_not_prose_on_a_substituted_tree() {
    let probe = Scratch::new("action-door-reader");
    let dir = probe.path();
    std::fs::create_dir_all(dir.join("src/nested")).expect("합성 트리를 만들지 못했다");
    std::fs::create_dir_all(dir.join("crates")).expect("합성 형제 트리를 만들지 못했다");

    let needle = "zeta_probe_call";
    std::fs::write(
        dir.join("src/real_caller.rs"),
        "fn go() {\n    zeta_probe_call(1);\n}\n",
    )
    .expect("합성 호출자 실패");
    std::fs::write(
        dir.join("src/nested/deep_caller.rs"),
        "fn deep() {\n    zeta_probe_call(2);\n}\n",
    )
    .expect("합성 하위 호출자 실패");
    std::fs::write(
        dir.join("src/only_comment.rs"),
        "/// zeta_probe_call 이 무엇인지 설명하는 doc 주석이다.\nfn unrelated() {}\n",
    )
    .expect("합성 주석 파일 실패");
    std::fs::write(
        dir.join("src/only_string.rs"),
        "fn name() -> &'static str {\n    \"zeta_probe_call\"\n}\n",
    )
    .expect("합성 문자열 파일 실패");
    std::fs::write(
        dir.join("src/own_def.rs"),
        "pub fn zeta_probe_call(n: u8) {\n    let _ = n;\n}\n",
    )
    .expect("합성 정의 파일 실패");
    std::fs::write(
        dir.join("crates/outside.rs"),
        "fn out() {\n    zeta_probe_call(9);\n}\n",
    )
    .expect("합성 바깥 파일 실패");

    // 실제 소스의 하한 대신 작은 합성 트리에 맞는 하한을 사용한다.
    let sources = sources_under(dir, 5);
    let rels: BTreeSet<String> = sources
        .iter()
        .map(|(p, _)| p.to_string_lossy().into_owned())
        .collect();
    assert!(
        !rels.iter().any(|r| r.starts_with("crates/")),
        "걷기가 `src` 밖으로 나갔다 — 모수가 넓어졌다"
    );
    assert!(
        rels.contains("src/nested/deep_caller.rs"),
        "걷기가 하위 디렉토리로 안 내려갔다"
    );

    let found = code_uses_in(&sources, needle, "src/own_def.rs");
    assert_eq!(
        found,
        ["src/nested/deep_caller.rs", "src/real_caller.rs"]
            .iter()
            .map(|s| (*s).to_string())
            .collect::<BTreeSet<String>>(),
        "술어가 합성 트리에서 다른 답을 냈다"
    );
    assert!(
        !found.contains("src/only_comment.rs"),
        "주석에만 있는 이름을 호출 파일로 집계했다"
    );
    assert!(
        !found.contains("src/only_string.rs"),
        "문자열에만 있는 이름을 호출 파일로 집계했다"
    );
    assert!(
        !found.contains("src/own_def.rs"),
        "정의가 사는 파일을 자기 호출자로 셌다"
    );
}
