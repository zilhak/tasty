//! 본체 src에서 TASTY_HOME/HOME의 직접 변경과 별도 TASTY_HOME_ENV_LOCK 사용을 찾는다.
//! 테스트의 홈 격리는 tasty-test-support의 TastyHomeGuard를 사용해야 한다.
//! 이 타입은 락을 잡고 TASTY_HOME을 바꾼 뒤 Drop에서 환경을 복원하고 락을 푼다.
//! 락만 직접 잡고 환경을 바꾸면 panic 때 복원을 빠뜨릴 수 있다.
//!
//! 공용 락은 모듈 비공개여야 하며, src에서 같은 이름의 락을 따로 선언하는 것도 검사한다.
//! 통합 테스트는 별도 프로세스이므로 이 검사는 루트 tests까지 포함하지 않는다.
//!
//! 호출·락 이름은 마스킹한 코드에서, 환경변수 키는 같은 줄의 원문에서 찾는다.
//! 상수·변수로 키를 전달하거나 호출과 키가 다른 줄이면 놓칠 수 있다.
//! 소스 형태를 검사하므로 모든 환경 변경 경로의 안전성을 증명하지는 않는다.

use std::path::{Path, PathBuf};

use tasty_doc_guards::source_text::mask_non_code;

/// 홈 환경을 관리하는 공유 타입의 정의 파일.
const OWNER: &str = "crates/tasty-test-support/src/lib.rs";

/// 빈 순회가 통과하지 않게 하는 하한이다. 2026-09-06의 기존 순회에서 제외 후 591파일을 측정했다.
/// 하한 400은 파일 정리를 허용할 여유를 둔 값이다. 현재 OWNER는 src 밖에 있다.
const MIN_SCANNED_FILES: usize = 400;

const ENV_WRITE_CALLS: &[&str] = &["set_var(", "remove_var("];
const LOCK_IDENT: &str = "TASTY_HOME_ENV_LOCK";

const OUR_KEYS: &[&str] = &["\"TASTY_HOME\"", "\"HOME\""];

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// 일치한 이름 뒤에 식별자 문자가 이어지면 더 긴 이름으로 보고 제외한다.
fn names_exactly(code: &str, needle: &str) -> bool {
    let mut from = 0usize;
    while let Some(rel) = code[from..].find(needle) {
        let end = from + rel + needle.len();
        let next = code[end..].chars().next();
        if !next.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            return true;
        }
        from = end;
    }
    false
}

fn violations_in(raw: &str) -> Vec<(usize, &'static str)> {
    let masked = mask_non_code(raw);
    let raw_lines: Vec<&str> = raw.lines().collect();
    let mut out = Vec::new();
    for (i, code) in masked.lines().enumerate() {
        let raw_line = raw_lines.get(i).copied().unwrap_or("");
        if ENV_WRITE_CALLS.iter().any(|c| code.contains(c))
            && OUR_KEYS.iter().any(|k| raw_line.contains(k))
        {
            out.push((i + 1, "env 쓰기"));
        }
        if code.contains(LOCK_IDENT) {
            // 공용 락이 비공개여도 같은 이름의 별도 락을 만드는 것은 컴파일될 수 있다.
            out.push((i + 1, "같은 이름의 락을 따로 세웠다"));
        }
    }
    out
}

fn visit(dir: &Path, offenders: &mut Vec<String>, scanned: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, offenders, scanned);
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let rel = path
            .strip_prefix(repo_root())
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if rel == OWNER {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        *scanned += 1;
        for (line, what) in violations_in(&raw) {
            offenders.push(format!("{rel}:{line}  {what}"));
        }
    }
}

#[test]
fn tasty_home_env_is_only_touched_through_the_guard() {
    let mut offenders = Vec::new();
    let mut scanned = 0usize;
    visit(&repo_root().join("src"), &mut offenders, &mut scanned);

    assert!(
        scanned >= MIN_SCANNED_FILES,
        "src의 Rust 파일을 {scanned}개만 수집했다(하한 {MIN_SCANNED_FILES}). 경로와 실제 파일 수를 확인한다."
    );
    assert!(
        offenders.is_empty(),
        "TASTY_HOME/HOME을 직접 바꾸거나 {LOCK_IDENT}를 별도로 사용한 곳이다:\n{}\n홈 격리는 {OWNER}의 TastyHomeGuard를 사용해 TASTY_HOME 복원과 직렬화를 함께 보장한다. HOME을 직접 변경하는 방식과 서로 다른 락으로 같은 키를 보호하는 방식은 피한다.",
        offenders.join("\n")
    );
}

/// 공유 파일에서 가드와 복원 타입의 정의가 사라지면 경로만 남은 예외가 되므로 확인한다.
#[test]
fn the_owner_still_defines_the_guard_that_justifies_its_exemption() {
    let raw = std::fs::read_to_string(repo_root().join(OWNER))
        .unwrap_or_else(|e| panic!("{OWNER} 을 못 읽었다: {e}"));
    let code = mask_non_code(&raw);
    for needle in [
        "struct TastyHomeGuard",
        "impl Drop for EnvVarGuard",
        LOCK_IDENT,
    ] {
        assert!(
            names_exactly(&code, needle),
            "{OWNER}에서 {needle}을 찾지 못했다. 공유 가드가 이동했다면 검사 경로도 갱신한다."
        );
    }
    assert!(
        code.lines().filter(|l| !l.trim().is_empty()).count() > 20,
        "{OWNER} 의 코드 줄이 거의 없다 — 마스킹이나 경로가 틀렸다"
    );
}

/// 실제 직접 사용이 생기기 전에도 공용 락의 가시성 변경을 검출한다.
#[test]
fn the_lock_declaration_stays_module_private() {
    let raw = std::fs::read_to_string(repo_root().join(OWNER))
        .unwrap_or_else(|e| panic!("{OWNER} 을 못 읽었다: {e}"));
    let code = mask_non_code(&raw);

    let decls: Vec<&str> = code
        .lines()
        .filter(|l| l.contains("static") && names_exactly(l, LOCK_IDENT))
        .collect();

    // 선언 미발견을 비공개라고 오인하지 않게 하나의 선언을 찾았는지 먼저 확인한다.
    assert_eq!(
        decls.len(),
        1,
        "{OWNER}에서 {LOCK_IDENT} 선언을 하나로 찾지 못했다({}개): {decls:?}. 이름·경로·마스킹을 확인한다.",
        decls.len()
    );

    assert!(
        !decls[0].contains("pub"),
        "{LOCK_IDENT} 선언이 공개됐다: {}. 공용 RAII 가드 밖에서 락만 잡고 환경을 바꾸는 우회를 막도록 모듈 비공개로 유지한다. 공개가 필요하면 대체 보호 방법도 검토한다.",
        decls[0].trim()
    );
}

#[test]
fn the_floor_refuses_to_believe_an_empty_walk() {
    let mut offenders = Vec::new();
    let mut scanned = 0usize;
    visit(
        &repo_root().join("src-does-not-exist"),
        &mut offenders,
        &mut scanned,
    );
    assert_eq!(scanned, 0, "없는 루트를 훑고도 파일을 셌다");
    assert!(scanned < MIN_SCANNED_FILES, "빈 순회가 하한을 넘었다");
}

#[test]
fn the_detector_counts_code_not_prose() {
    let hit = "fn f() { unsafe { std::env::set_var(\"TASTY_HOME\", \"/tmp\") }; }";
    assert_eq!(violations_in(hit).len(), 1, "진짜 env 쓰기를 놓쳤다");
    // 문자열 안의 합성 코드이므로 컴파일 가능 여부와 무관하게 이름 검출만 확인한다.
    let lock_hit = "fn f() { let _g = TASTY_HOME_ENV_LOCK.lock(); }";
    assert_eq!(
        violations_in(lock_hit).len(),
        1,
        "락 이름을 코드에서 못 셌다"
    );

    let prose = "// set_var(\"TASTY_HOME\") 은 여기서 하지 않는다. TASTY_HOME_ENV_LOCK 도.";
    assert!(violations_in(prose).is_empty(), "주석을 코드로 셌다");

    let ledger =
        "const R: &str = \"TASTY_HOME_ENV_LOCK 은 set_var(\\\"TASTY_HOME\\\") 를 지킨다\";";
    assert!(violations_in(ledger).is_empty(), "명부 사유를 코드로 셌다");

    let other = "fn f() { unsafe { std::env::set_var(\"TASTY_AGENT_ID\", \"x\") }; }";
    assert!(violations_in(other).is_empty(), "우리 키가 아닌 것을 셌다");
}

#[test]
fn the_premise_needle_is_a_whole_identifier() {
    assert!(names_exactly(
        "pub struct TastyHomeGuard {",
        "struct TastyHomeGuard"
    ));
    assert!(!names_exactly(
        "pub struct TastyHomeGuardRenamed {",
        "struct TastyHomeGuard"
    ));
    assert!(names_exactly(
        "TastyHomeGuardRenamed; struct TastyHomeGuard {",
        "struct TastyHomeGuard"
    ));
}
