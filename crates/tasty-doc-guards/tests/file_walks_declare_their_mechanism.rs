//! 레포 파일을 훑는 타깃이 **어떤 수단으로 훑는지**를 한 곳에서 선언한다.
//!
//! "이 파일을 판정하는 것이 무엇인가" 는 새것을 만들 때마다 나오는 물음이고, 지금
//! 그 답을 아는 유일한 방법은 다 돌려 보는 것이다. 정적으로 답하려는 시도는
//! 순회 **범위**를 소스에서 읽어야 하는데 그것은 임의의 코드라 언제나 근사가 된다
//! (2026-09-06 실측: 순회 시작점을 기계적으로 뽑으니 36 중 17 이 안 읽혔고, 확장자
//! 필터를 뽑으니 디렉토리 이름을 확장자로 세었다).
//!
//! 범위는 못 읽어도 **"훑는가" 는 정확히 셀 수 있다** — 훑으려면 순회 API 를 불러야
//! 하기 때문이다. 그 셈이 성립하는 조건이 하나 있다: **수단이 알려진 것뿐이어야
//! 한다.** 다른 수단이 조용히 들어오면 "무엇이 새 파일을 볼 수 있는가" 를 세는 모든
//! 도구가 그날부터 덜 세고, 덜 세는 쪽은 초록으로 보인다.
//!
//! 그래서 이 가드는 대체 수단을 **금지하지 않는다.** 들어오면 여기 선언하라고 요구할
//! 뿐이다 — 선언이 늘어나는 것은 정상이고, 선언 없이 늘어나는 것만 막는다.

// 이유: 이 타깃은 시험 범위다. `let _` 로 값을 버리는 자리를 여기서 명부에 올리면
//       그 명부가 프로덕션 자리를 가리키는 뜻을 잃는다 —
//       `tests/let_underscore_documented.rs` 의 명부 순수성 판정이 그것을 막는다.
#![allow(clippy::let_underscore_must_use)]

/// 오늘 쓰이는 순회 수단. 다른 것이 들어오면 여기에 한 줄을 더한다.
const KNOWN_MECHANISMS: &[(&str, &str)] = &[(
    "std::fs::read_dir",
    "표준 라이브러리 재귀 순회 — 2026-09-06 기준 유일한 수단",
)];

/// 선언 없이 들어올 수 있는 대체 수단들. 발견되면 위 목록에 추가하라는 뜻이지
/// 쓰지 말라는 뜻이 아니다.
const UNDECLARED_MECHANISMS: &[&str] = &[
    "walkdir",
    "jwalk",
    "glob::glob",
    "include_dir",
    "ignore::Walk",
];

/// 검사한 타깃 수의 하한 — 모수가 비면 "다른 수단 없음" 은 언제나 참이다.
/// 값의 근거: 2026-09-06 실측 99(루트 52 + 크레이트 47).
///
/// **판별식** — 통합 테스트 타깃은 파일 하나가 타깃 하나라, 셸에서 직접 셀 수 있다:
///
/// ```text
/// ls tests/*.rs | wc -l ; ls crates/*/tests/*.rs | wc -l    # 두 뿌리를 따로
/// cargo test -p tasty-doc-guards --test file_walks_declare_their_mechanism -- --nocapture
///   → [순회 수단 선언] 통합 테스트 타깃 <N> · 하한 70
/// ```
///
/// 실측 2026-09-07(`de0572359`): 루트 **53** + 크레이트 **58** = **111** 이고 이 시험도
/// 111 을 센다. 09-06 의 99(52+47)에서 늘었다 — **두 뿌리를 따로 세는 것이 중요하다.**
/// 합만 보면 한쪽이 죽고 다른 쪽이 늘어난 것을 못 가른다.
///
/// ★ 같은 모수를 보는 게이트가 하나 더 있다 — `scripts/check-shared-walk-ratchet.sh` 가
/// **이 타깃들 안의 직접 `read_dir(` 건수**를 상한으로 잡는다. 이 하한이 모수의 크기를,
/// 그쪽 상한이 그 모수 안에서 공용 순회를 안 쓰는 자리 수를 본다. 한쪽만 움직이면
/// 정상이지만 **이 수가 줄었는데 그쪽이 그대로면** 사라진 타깃이 순회를 안 하던 것이다.
///
/// **이 수를 내려서 초록을 만들지 마라.** 아래 판정("선언되지 않은 순회 수단이 없다")은
/// 모수를 순회하므로, 수집이 절반 죽으면 절반만 검사하면서 초록이 된다.
///
/// 정당한 수선: 타깃을 실제로 지웠으면 이 수를 함께 내려라. 그때 **두 뿌리를 각각** 세서
/// 어느 쪽이 줄었는지 적어라 — 합만 맞추면 다음 사람이 같은 물음을 다시 판다.
const MIN_TARGETS: usize = 70;

/// 소스에 선언되지 않은 순회 수단이 **코드로** 등장하는지. 주석 안의 언급은 세지
/// 않는다 — 이 판정의 물음은 "그 수단을 쓰는가" 이지 "그 낱말이 있는가" 가 아니다.
fn undeclared_mechanisms_in(src: &str) -> Vec<&'static str> {
    let code = tasty_doc_guards::strip_line_comments(src);
    UNDECLARED_MECHANISMS
        .iter()
        .filter(|needle| code.contains(**needle))
        .copied()
        .collect()
}

/// 통합 테스트 타깃 전부 — 루트 패키지와 각 크레이트의 `tests/`.
fn integration_test_targets(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![root.join("tests")];
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        for entry in entries.flatten() {
            dirs.push(entry.path().join("tests"));
        }
    }
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn every_directory_walk_uses_a_declared_mechanism() {
    assert!(
        !KNOWN_MECHANISMS.is_empty(),
        "선언 목록이 비면 아래 판정은 아무것도 뜻하지 않는다"
    );
    let root = tasty_doc_guards::repo_root();
    let targets = integration_test_targets(&root);

    println!(
        "[순회 수단 선언] 통합 테스트 타깃 {} · 하한 {MIN_TARGETS}",
        targets.len()
    );
    assert!(
        targets.len() >= MIN_TARGETS,
        "통합 테스트 타깃을 {} 개만 찾았다(하한 {MIN_TARGETS}) — 수집이 죽으면 \
         아래 판정은 빈 집합을 훑고 조용히 통과한다.",
        targets.len()
    );

    // 이 파일 자신은 대체 수단의 이름을 **목록으로** 담는 것이 본질이라 제외한다.
    // 이름이 아니라 자기 참조로 가른다 — 파일이 옮겨져도 따라온다.
    let own = std::path::Path::new(file!())
        .file_name()
        .expect("file!() 에 파일명이 없다");

    let mut findings = Vec::new();
    for path in &targets {
        if path.file_name() == Some(own) {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        for needle in undeclared_mechanisms_in(&src) {
            let rel = path.strip_prefix(&root).unwrap_or(path);
            findings.push(format!("  {} — {needle}", rel.display()));
        }
    }

    assert!(
        findings.is_empty(),
        "선언되지 않은 순회 수단이 쓰인다:\n{}\n\n\
         쓰지 말라는 뜻이 아니다 — `KNOWN_MECHANISMS` 에 한 줄을 더하라는 뜻이다. \
         \"무엇이 이 파일을 볼 수 있는가\" 를 세는 도구는 순회 수단을 열거해서 센다. \
         선언되지 않은 수단이 있으면 그 셈은 그날부터 덜 세고, 덜 세는 쪽은 초록으로 \
         보인다.",
        findings.join("\n")
    );
}

/// 위 판정이 **무엇이든 잡을 수 있는지** 를 같은 함수로 확인한다.
/// 한 방향만 재면 "위반 0" 과 "판정이 죽었다" 가 구별되지 않는다.
#[test]
fn the_detector_separates_an_undeclared_mechanism_from_the_declared_one() {
    assert_eq!(
        undeclared_mechanisms_in("let w = walkdir::WalkDir::new(p);"),
        vec!["walkdir"],
        "코드에 쓰인 대체 수단을 잡아야 한다"
    );
    assert!(
        undeclared_mechanisms_in("let d = std::fs::read_dir(p);").is_empty(),
        "선언된 수단은 위반이 아니다"
    );
    assert!(
        undeclared_mechanisms_in("// walkdir 은 이 자리에서 쓰지 않는다").is_empty(),
        "주석 안의 언급은 그 수단을 쓰는 것이 아니다"
    );
}

/// [`MIN_TARGETS`] 의 **양성 대조** — 수집이 죽으면 이 수가 하한 밑으로 떨어지나.
///
/// 이 수집기는 **두 뿌리**를 훑는다(`tests/` 와 `crates/*/tests/`). 상수 doc 에 "두 뿌리를
/// 따로 세라 — 합만 보면 한쪽이 죽고 다른 쪽이 늘어난 것을 못 가른다" 고 적었는데,
/// 그 말이 성립하려면 **두 뿌리가 각각 실제로 읽히는지**가 먼저다. 칸을 그렇게 나눈다.
#[test]
fn the_target_floor_sees_a_collapsed_collection() {
    let root = std::env::temp_dir().join(format!(
        "tasty-targetfloor-{}-{}",
        std::process::id(),
        line!()
    ));
    // 앞선 실행의 잔여를 치운다 — 없는 것이 정상이라 실패가 정보가 아니다.
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("임시 디렉토리를 못 만들었다");

    assert_eq!(
        integration_test_targets(&root).len(),
        0,
        "뿌리가 비었는데 0 이 아니면 이 수집기는 입력을 안 보는 것이다"
    );

    // 뿌리 ① — 레포 루트의 tests/
    std::fs::create_dir_all(root.join("tests")).expect("생성 실패");
    std::fs::write(root.join("tests/a.rs"), "").expect("쓰기 실패");
    assert_eq!(
        integration_test_targets(&root).len(),
        1,
        "루트 뿌리를 안 읽으면 상수 doc 의 '루트 + 크레이트' 분해가 거짓이 된다"
    );

    // 확장자가 아닌 것은 타깃이 아니다 — 이것이 섞이면 수가 실제보다 커진다.
    std::fs::write(root.join("tests/notes.md"), "").expect("쓰기 실패");
    assert_eq!(
        integration_test_targets(&root).len(),
        1,
        "`.rs` 가 아닌 파일을 세면 모수가 부풀고, 부푼 만큼 하한이 무뎌진다"
    );

    // 뿌리 ② — crates/*/tests/
    std::fs::create_dir_all(root.join("crates/x/tests")).expect("생성 실패");
    std::fs::write(root.join("crates/x/tests/b.rs"), "").expect("쓰기 실패");
    assert_eq!(
        integration_test_targets(&root).len(),
        2,
        "크레이트 뿌리를 안 읽으면 한쪽만 세면서 하한을 통과하게 된다 — 이 축이 잡으려는 \
         '수집이 절반 죽는' 형태가 정확히 그것이다"
    );

    // 뒷정리 실패는 무시한다 — 임시 디렉토리라 남아도 다음 실행이 먼저 지우고, 여기서
    // 죽으면 위 단정의 결과가 정리 오류에 가린다.
    let _ = std::fs::remove_dir_all(&root);
}
