//! 본체의 환경변수 변경은 복원을 보장하는 테스트 지원 가드를 사용한다.
//! TastyHomeGuard는 락·기존 값 보관·임시 홈·Drop 복원을 함께 처리한다.
//! 수동 복원은 단언의 panic에 건너뛰어지거나 원래 환경변수 값을 잃어 뒤 시험을 오염시킬 수 있다.
//!
//! src와 테스트 지원 파일을 수집해 env::set_var/remove_var 호출 형태를 찾는다. 키 이름은 가리지 않는다.
//! 함수 별칭이나 간접 호출은 추적하지 않는다. 출하 코드 예외는 홈 키 리터럴 부재를 확인하지만
//! 동적으로 조립한 키까지 판별하지는 못한다.

use tasty_doc_guards::source_text::mask_non_code;

const DOOR: &str = "crates/tasty-test-support/src/lib.rs";

const MUTATION: &[&str] = &["env::set_var(", "env::remove_var("];

const HOME_KEYS: &[&str] = &["\"TASTY_HOME\"", "\"HOME\""];

/// 출하 코드 예외의 (파일, 사유). HOME_KEYS 리터럴이 원문에 있으면 예외를 재검토한다.
/// 키의 런타임 값을 평가하는 검사는 아니다.
const PRODUCTION_EXCEPTIONS: &[(&str, &str)] = &[(
    "src/boot/locale.rs",
    "프로덕션이 OS 로케일을 자식 프로세스로 넘긴다 — 홈 키가 아니라 로케일 키다",
)];

struct Hit {
    rel: String,
    line: usize,
    text: String,
}

fn scan() -> (Vec<Hit>, usize) {
    let mut hits = Vec::new();
    let mut files = 0usize;
    for (rel_path, raw) in super::rust_sources() {
        let rel_str = rel_path.to_string_lossy();
        if !rel_str.starts_with("src/") && rel_str != DOOR {
            continue;
        }
        let rel = rel_path.to_string_lossy().to_string();
        files += 1;
        if rel == DOOR {
            continue;
        }
        for (i, line) in mask_non_code(&raw).lines().enumerate() {
            if MUTATION.iter().any(|m| line.contains(m)) {
                hits.push(Hit {
                    rel: rel.clone(),
                    line: i + 1,
                    text: line.trim().to_string(),
                });
            }
        }
    }
    (hits, files)
}

#[test]
fn tasty_home_is_only_changed_through_the_test_support_guard() {
    let (hits, files) = scan();
    assert!(
        files >= 200,
        "본체 소스를 {files}개만 수집했다(하한 200). 파일 수와 순회 범위를 확인한다."
    );

    let mut offenders = Vec::new();
    for h in &hits {
        if PRODUCTION_EXCEPTIONS.iter().any(|(f, _)| *f == h.rel) {
            continue;
        }
        offenders.push(format!("  {}:{}  {}", h.rel, h.line, h.text));
    }
    assert!(
        offenders.is_empty(),
        "`{DOOR}` 밖에서 환경변수를 바꾸는 곳이 {}개다:\n{}\n홈 변경은 test_support::TastyHomeGuard를 사용한다. 락만 직접 잡으면 panic 때 이전 값이 복원되지 않을 수 있다. 다른 키는 Drop으로 복원하는 EnvVarGuard를 쓴다. 출하 코드에 필요한 변경은 PRODUCTION_EXCEPTIONS에 구체적인 사유를 적는다.",
        offenders.len(),
        offenders.join("\n")
    );

    for (rel, reason) in PRODUCTION_EXCEPTIONS {
        let raw = std::fs::read_to_string(super::repo_root().join(rel)).unwrap_or_default();
        assert!(
            !raw.is_empty(),
            "면제로 적힌 `{rel}` 을 못 읽었다 — 파일이 옮겨졌으면 명부를 따라 옮겨라"
        );
        let masked = mask_non_code(&raw);
        assert!(
            MUTATION.iter().any(|m| masked.contains(m)),
            "`{rel}`에 환경변수 변경이 없어졌다. 불필요해진 예외를 제거한다."
        );
        // 키 리터럴의 존재를 확인해야 하므로 원문을 읽는다.
        for key in HOME_KEYS {
            assert!(
                !raw.contains(key),
                "면제된 `{rel}` 이 이제 {key} 를 부른다 — 사유(\"{reason}\")가 거짓이 됐다. \
                 홈 키를 만지면 그 자리는 `{DOOR}` 를 지나야 한다"
            );
        }
    }

    println!(
        "[환경변수 검사] 본체 파일 {files}개 · 가드 밖 변경 {}개(등록된 예외)",
        hits.len()
    );
}
