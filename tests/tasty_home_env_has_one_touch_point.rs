//! `TASTY_HOME`/`HOME` 환경변수를 만지는 자리가 본체 `src/` 에 **한 곳뿐**임을 못박는다.
//!
//! # 왜 필요한가
//!
//! `src/test_support.rs` 의 `TastyHomeGuard` 는 RAII 다 — 생성이 `TASTY_HOME_ENV_LOCK` 을
//! 잡고 이전 값을 기억하며, `Drop` 이 그 값을 되돌린 뒤에 락을 푼다. 그래서 그 타입을
//! 거치는 한 획득과 복원이 갈라질 수 없다.
//!
//! **그 타입을 안 거치면 갈라진다.** 락만 직접 잡고 env 를 손으로 세팅하면, 단언 실패로
//! 패닉했을 때 복원 줄에 도달하지 못하고 같은 프로세스의 뒤따르는 테스트가 오염된
//! `TASTY_HOME` 을 물려받는다 — 변경과 무관한 실패가 난다.
//!
//! # 우회 형태가 둘이고, 지금은 **막는 것이 다르다**
//!
//! ```text
//! ① 락을 잡고 env 를 만진다   → 컴파일러가 막는다 (선언이 모듈 비공개다)
//! ② 락 없이 그냥 set_var 한다 → 이 가드가 막는다  (락이 필요 없는 경로다)
//! ```
//!
//! **대체가 아니라 분담이다.** ① 이 구조로 올라간 것은 선언에서 `pub` 이 빠졌기 때문이고,
//! 그 전에는 `pub(crate)` 여서 같은 크레이트의 어떤 테스트든 락을 그냥 잡을 수 있었다
//! (2026-09-06 실측: 그 프로브를 심었을 때 `source_guards::` 192 개가 전부 통과했다 —
//! 그때는 ①을 잡는 것이 아무것도 없었다. 그 측정이 이 파일과 뒤이은 승급의 근거다).
//!
//! ★ **①의 보호는 선언이 비공개인 동안에만 성립한다.** 그리고 그 가시성 자체를 지키는
//! 것은 레포에 [`the_lock_declaration_stays_module_private`] 하나뿐이다 — 승급 커밋이
//! 그 자리를 지키던 다른 시험을 함께 지웠다. 그래서 이 파일은 두 겹으로 선다: 그 시험이
//! **첫 그물**(가시성이 되돌려지는 것)이고, `LOCK_IDENT` 바늘이 **두 번째 그물**(되돌린
//! 뒤 실제로 락을 잡는 것)이다.
//!
//! # 모수가 `src/` 인 이유
//!
//! 이 크레이트에는 `lib` 타깃이 없다(`src/main.rs` 만 있다). 통합 테스트(`tests/`)는
//! bin 크레이트의 항목을 import 할 수 없으므로 `TASTY_HOME_ENV_LOCK` 을 아예 못 잡고,
//! 각 통합 타깃은 **별개 프로세스**라 `src/` 단위 테스트와 env 를 공유하지도 않는다.
//! 그쪽의 격리는 다른 물음이고 다른 장치가 필요하다 — 여기서 같이 재면 두 물음이 한 수에
//! 뭉개진다.
//!
//! # 사본을 둘 쓴다 — 두 물음이 서로의 답을 지운다
//!
//! `mask_non_code` 는 문자열 **내용**을 공백으로 덮는다. 그래서
//!
//! - "여기 진짜 호출이 있나"(`set_var(` · `remove_var(` · 락 식별자)는 **덮은 사본**에서
//!   묻는다. 안 덮으면 이 파일 같은 규칙 본문이나 명부의 사유 문자열이 호출로 세어진다.
//! - "그 줄이 **우리 키**를 부르나"(`"TASTY_HOME"` · `"HOME"`)는 **원문**에서 묻는다.
//!   덮은 사본에는 키 이름이 남지 않기 때문이다.
//!
//! 두 사본은 줄바꿈을 보존하므로 줄 번호가 같다 — 같은 줄 인덱스로 교차한다.
//!
//! # 못 잡는 것
//!
//! 키를 상수·변수로 넘기는 형태(`env::set_var(KEY, ..)`)는 원문에도 리터럴이 없어 못
//! 잡는다. 이 가드는 그 형태를 **막지 못한다** — 연기 검사이지 증명이 아니다.
//!
//! # 이 가드가 빨개졌을 때 가장 싼 초록화 경로
//!
//! 우회를 지우는 것이다(프로브 한 덩이를 빼면 된다). 보호 대상을 깎는 경로는 셋 다 더
//! 비싸거나 막혀 있다 — 면제 목록이 **없고**(유일 예외는 아래 `OWNER` 한 상수다),
//! `OWNER` 를 위반 파일로 바꾸면 그 파일이 가드를 정의하지 않으므로 전제 재검사가
//! 무너지며, 스캔 루트를 좁히면 하한에 걸린다.

use std::path::Path;

use tasty_doc_guards::source_text::mask_non_code;

/// 두 키를 만져도 되는 **유일한** 자리(레포 상대 경로).
const OWNER: &str = "src/test_support.rs";

/// 스캔 하한 — ADR-0133 의 두 용도 중 **연기 검사**다("경로가 틀렸거나 읽기에 실패했다"
/// 를 잡는 용도). 모수 고정으로 쓰지 않는다.
///
/// 값의 근거: 2026-09-06 실측으로 `src/` 아래 `.rs` 가 592 개이고 `OWNER` 를 빼면 591 개다.
/// 400 은 그 3 분의 1 을 잃는 순회 사고까지 잡으면서, 파일이 정상적으로 줄어드는 것에는
/// 안 걸릴 만큼 떨어져 있다.
const MIN_SCANNED_FILES: usize = 400;

/// **덮은 사본**에서 찾는 것 — 진짜 호출·진짜 식별자.
const ENV_WRITE_CALLS: &[&str] = &["set_var(", "remove_var("];
const LOCK_IDENT: &str = "TASTY_HOME_ENV_LOCK";

/// **원문**에서 찾는 것 — 어느 키인가.
const OUR_KEYS: &[&str] = &["\"TASTY_HOME\"", "\"HOME\""];

fn repo_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// `code` 에 `needle` 이 **식별자 하나로** 있는지. 뒤에 식별자 문자가 이어지면 다른
/// 이름이다.
///
/// 접두사 일치로 물으면 `struct TastyHomeGuard` 가 `struct TastyHomeGuardRenamed` 에도
/// 걸려, 가드를 이름만 바꿔 옮겨도 전제 재검사가 통과한다(변이로 확인했다 — 고치기 전
/// 이름 변경 변이가 살아남았다).
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

/// 한 파일에서 위반 줄을 모은다. `(줄번호, 무엇)`.
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
            // 이 갈래의 뜻은 락이 모듈 비공개가 되면서 바뀌었다. 밖에서 **획득**하는 것은
            // 이제 컴파일러가 막으므로 컴파일되는 트리에는 그 형태가 없다. 남는 것은
            // **같은 이름의 두 번째 락을 따로 세우는 것**이고, 그것은 직렬화를 획득 공유와
            // 똑같이 깨뜨리면서 타입으로는 안 잡힌다.
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
        "스캔한 `.rs` 가 {scanned}개다(하한 {MIN_SCANNED_FILES}) — 순회가 깨졌다. \
         위반 0 은 이 상태에서 아무 뜻도 없다"
    );
    assert!(
        offenders.is_empty(),
        "`TASTY_HOME`/`HOME` env 변경은 `{OWNER}` 의 `TastyHomeGuard` 를 통해서만 한다. \
         그 타입을 안 거치면 획득과 복원이 갈라져, 패닉한 테스트가 오염된 env 를 같은 \
         프로세스의 뒤 테스트에 물려준다. 그리고 `{LOCK_IDENT}` 라는 이름의 락을 **따로** \
         세우지 마라 — 락이 둘이면 서로를 안 기다려서, 직렬화는 한 개도 없는 것과 같아진다. \
         (밖에서 그 락을 **획득**하는 것은 이제 컴파일러가 막는다. 여기서 보는 것은 \
         타입이 못 보는 나머지 절반이다.) \
         위반:\n{}",
        offenders.join("\n")
    );
}

/// 면제의 **전제**를 다시 검사한다.
///
/// `OWNER` 한 곳만 빼는 것이 이 가드의 유일한 예외인데, 그 예외가 정당한 이유는 그 파일이
/// 획득과 복원을 함께 맡는 타입을 **정의하기 때문**이다. 그 사실이 거짓이 되면(가드가
/// 옮겨가거나 사라지면) 예외만 남아 그 파일이 자유롭게 env 를 만지는 자리가 된다.
/// 사유가 산문으로만 있으면 그 전락이 조용하다.
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
            "{OWNER} 에 `{needle}` 이 없다 — 이 파일을 예외로 두는 근거가 거짓이 됐다. \
             가드가 옮겨갔으면 예외도 따라가야 한다"
        );
    }
    // 비영 대조 — 파일을 실제로 읽었고 마스킹이 전부를 덮어버리지 않았다.
    // 이것이 없으면 빈 문자열에서도 위 단언이 …실패하는 대신 파일 부재로 패닉해
    // 원인이 뒤바뀐다.
    assert!(
        code.lines().filter(|l| !l.trim().is_empty()).count() > 20,
        "{OWNER} 의 코드 줄이 거의 없다 — 마스킹이나 경로가 틀렸다"
    );
}

/// 락 선언의 **가시성**을 못박는다 — 첫 그물이다.
///
/// `LOCK_IDENT` 바늘([`violations_in`])은 두 번 틀려야 발화한다: 누가 (a) 선언을
/// `pub(crate)` 로 되돌리고 (b) 다른 `src/` 모듈에서 락을 직접 잡아야 한다. 그 사이
/// 구간에서는 컴파일러도 조용하고 이 파일도 조용하다. 첫 번째에서 잡으려면 불변식
/// 자신을 물어야 한다.
///
/// 그리고 이것 말고 그 불변식을 지키는 것이 레포에 없다 — 승급 커밋이 락 이름을 보던
/// 다른 가드의 시험을 함께 지웠고, 승급 **자체**를 지키는 것은 남지 않았다. 컴파일러의
/// 보호는 선언이 비공개인 **동안에만** 성립한다.
#[test]
fn the_lock_declaration_stays_module_private() {
    let raw = std::fs::read_to_string(repo_root().join(OWNER))
        .unwrap_or_else(|e| panic!("{OWNER} 을 못 읽었다: {e}"));
    let code = mask_non_code(&raw);

    let decls: Vec<&str> = code
        .lines()
        .filter(|l| l.contains("static") && names_exactly(l, LOCK_IDENT))
        .collect();

    // 비영 대조 — 선언을 못 찾으면 아래 단언은 자극과 무관하게 초록이다. "없다" 를
    // "비공개다" 로 읽으면 이름만 바꿔도 통과한다.
    assert_eq!(
        decls.len(),
        1,
        "{OWNER} 에서 `{LOCK_IDENT}` 선언 줄을 하나로 못 집었다({} 개). 이름이 바뀌었거나 \
         마스킹이 틀렸다 — 그러면 아래 가시성 판정은 아무것도 안 잰다. 집힌 줄: {decls:?}",
        decls.len()
    );

    assert!(
        !decls[0].contains("pub"),
        "`{LOCK_IDENT}` 선언이 다시 공개됐다 — `{}`. 모듈 비공개가 우회 형태 ①(락을 잡고 \
         env 를 만지는 것)을 컴파일러 수준에서 막고 있고, 이 줄이 그 보호의 유일한 전제다. \
         공개로 되돌리려면 ①을 무엇이 대신 막는지 먼저 세워라",
        decls[0].trim()
    );
}

/// 하한 자신이 판정을 하는지 본다. 단언 안에 인라인으로 두면 그 값이 무엇을 가르는지
/// 시험할 자리가 없고, 하한이 장식이 된다.
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

/// 탐지기가 **코드**를 세고 산문·문자열을 안 세는지. 이 구분이 없으면 이 파일과
/// `src/source_guards/test_serialization_locks.rs` 의 명부 사유가 스스로 위반이 된다.
#[test]
fn the_detector_counts_code_not_prose() {
    // 진짜 호출 — 잡힌다.
    let hit = "fn f() { unsafe { std::env::set_var(\"TASTY_HOME\", \"/tmp\") }; }";
    assert_eq!(violations_in(hit).len(), 1, "진짜 env 쓰기를 놓쳤다");
    // 이 픽스처는 **문자열**이라 컴파일되지 않는다 — 그래서 락의 가시성과 무관하게
    // 늘 잡힌다. 증명하는 것은 "탐지기가 이 이름을 코드에서 센다" 까지이고, "그 형태가
    // 트리에 존재할 수 있다" 는 증명하지 않는다. 둘을 섞으면 승급으로 뜻이 바뀐 것을
    // 초록이 가린다.
    let lock_hit = "fn f() { let _g = TASTY_HOME_ENV_LOCK.lock(); }";
    assert_eq!(
        violations_in(lock_hit).len(),
        1,
        "락 이름을 코드에서 못 셌다"
    );

    // 주석 — 안 잡힌다.
    let prose = "// set_var(\"TASTY_HOME\") 은 여기서 하지 않는다. TASTY_HOME_ENV_LOCK 도.";
    assert!(violations_in(prose).is_empty(), "주석을 코드로 셌다");

    // 문자열 리터럴 안의 언급 — 안 잡힌다(명부가 사유를 그렇게 적는다).
    let ledger =
        "const R: &str = \"TASTY_HOME_ENV_LOCK 은 set_var(\\\"TASTY_HOME\\\") 를 지킨다\";";
    assert!(violations_in(ledger).is_empty(), "명부 사유를 코드로 셌다");

    // 다른 키의 env 쓰기 — 이 가드의 대상이 아니다.
    let other = "fn f() { unsafe { std::env::set_var(\"TASTY_AGENT_ID\", \"x\") }; }";
    assert!(violations_in(other).is_empty(), "우리 키가 아닌 것을 셌다");
}

/// 전제 재검사의 바늘이 **더 긴 이름**에 걸리지 않는지.
///
/// 이 절이 없으면 가드를 `TastyHomeGuardRenamed` 로 바꿔 옮겨도 전제가 살아 있는 것처럼
/// 보인다 — 접두사가 그대로 남기 때문이다. 실제로 그 변이가 한 번 살아남았고, 이 시험은
/// 그때 추가됐다.
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
    // 같은 이름이 두 번 나오고 한쪽만 정확할 때도 찾는다.
    assert!(names_exactly(
        "TastyHomeGuardRenamed; struct TastyHomeGuard {",
        "struct TastyHomeGuard"
    ));
}
