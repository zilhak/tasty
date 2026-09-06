//! **본체에서 `TASTY_HOME` 을 바꾸는 문은 하나다.**
//!
//! `src/test_support.rs` 의 [`TastyHomeGuard`] 는 락 획득 · 이전 값 보관 · 임시 디렉토리
//! 생성 · `Drop` 에서의 복원을 **한 벌로** 묶는다. 그 문서가 왜 수동 `set_var`/`remove_var`
//! 쌍으로 대신하면 안 되는지도 적는다 — 단언이 패닉하면 복원 줄에 도달하지 못하고,
//! `remove_var` 로 끝내면 원래 값이 있던 환경에서 그 값을 잃는다. 어느 쪽이든 같은
//! 프로세스의 뒤따르는 테스트가 오염된 환경을 물려받고, 그 오염은 **단독 실행에서 재현되지
//! 않는다.**
//!
//! # 왜 이 파일이 필요한가 — 같은 위험을 한쪽만 지키고 있었다
//!
//! `tasty-host-plugin` 에는 그 크레이트 자신을 훑는 시험이 있다
//! (`home_env_is_only_touched_through_this_module`). **본체에는 없었다.** 리포 전역
//! 가드([`tasty_doc_guards::env_isolation`])가 있지만 그것이 묻는 것은 다른 물음이다 —
//! *직렬화를 밝혔는가*. 락을 손으로 잡고 `set_var("TASTY_HOME")` 하는 자리는 그 물음에
//! **참**이라 통과한다. 직렬은 맞지만 복원이 없다.
//!
//! 실측(2026-09-06): `src/webhook/config.rs` 에 락을 직접 잡고 `set_var` 하는 프로브를
//! 심었더니 `source_guards::` 192 개가 전부 통과했다. 그 자리를 여기서 빨갛게 만든다.
//!
//! # 자매 가드와의 대조 — 아직 같은 물음이 아니다 (2026-09-07 읽기로 대조)
//!
//! `tasty-host-plugin` 의 `home_env_is_only_touched_through_this_module` 과 이제 둘 다
//! 있다. **같은 규칙의 두 벌이 아니다** — 네 자리가 다르고, 그 차이는 양방향이다.
//!
//! | | host-plugin | 여기(본체) |
//! |---|---|---|
//! | 키 범위 | 홈 키를 **이름으로** 든 줄만 본다 | 키를 안 가리고 env 변형 전부를 본다 |
//! | 프로덕션 예외 | 필요 없다(키로 좁혀 안 걸린다) | 명부 + **사유 재검사**가 필요하다 |
//! | 락 축 | 락이 `static`(비공개)이라 **컴파일러가** 막는다 | 락이 `pub(crate)` 라 텍스트로 막는다 |
//! | 순회 | 자기 크레이트를 자기가 `read_dir` | 공용 스캐너(모수가 git 목록과 못 박혀 있다) |
//!
//! **양방향인 이유**: 키로 좁힌 쪽은 키가 변수로 들어간 자리를 못 본다(본체
//! `boot/locale.rs` 가 그 모양이다 — 그래서 이쪽은 좁힐 수 없었다). 반대로 키를 안 가린
//! 쪽은 예외가 필요해지고, 예외는 그 자체가 도망길이라 사유를 다시 묻는 장치가 따라붙는다.
//!
//! ★ **락 축의 비대칭에는 더 싼 답이 있다.** 저쪽은 락이 `static`(모듈 비공개)이라 밖에서
//! 이름을 부르는 것 자체가 **컴파일 오류**다. 여기 락은 `pub(crate)` 라 텍스트 스캔이 그
//! 일을 대신한다. 가시성을 좁히면 아래 두 번째 시험이 구조적으로 불필요해진다 — 지금
//! 그 락을 이 파일 밖에서 부르는 자리는 0 이므로 한 낱말 변경이다. **이 회차에서는 안
//! 했다**(읽고 적는 범위였다). 하게 되면 그때 이 절과 그 시험을 함께 지운다.
//!
//! # 이 가드가 못 보는 것
//!
//! 텍스트 스캔이다. 함수 두 겹 너머의 간접 접근이나 런타임에 조립한 키 이름은 못 본다.
//! 그리고 [`TastyHomeGuard`] 를 **쓰되 락 없이** 쓰는 형태(가드 안에서 변형이 일어나므로
//! 스캔에 안 걸린다)도 못 본다 — 그건 가드가 락을 자기가 잡게 한 구조가 막는다.

use tasty_doc_guards::source_text::mask_non_code;

/// 이 문 하나만이 `TASTY_HOME` 을 바꾼다.
const DOOR: &str = "src/test_support.rs";

/// 프로세스 전역 env 를 바꾸는 호출. 여는 괄호까지 넣어 동명 식별자에 안 걸리게 한다.
const MUTATION: &[&str] = &["env::set_var(", "env::remove_var("];

/// 홈 관련 키 — 이 이름이 보이면 [`DOOR`] 밖에서는 무조건 위반이다.
const HOME_KEYS: &[&str] = &["\"TASTY_HOME\"", "\"HOME\""];

/// (파일, 사유, **그 사유가 참이면 이 파일에 없어야 하는 것**).
///
/// 셋째 칸이 이 명부의 핵심이다. 사유가 산문뿐이면 그 사유가 거짓이 되어도 표는 그대로
/// 남아 면제만 살아남는다 — 그래서 사유를 **기계가 다시 물을 수 있는 형태**로 적는다.
/// 여기서는 "홈 키가 아니라 로케일 키를 넘긴다" 가 그 진술이고, 홈 키 리터럴이 그 파일에
/// 나타나면 면제가 무너진 것이다.
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
        if !rel_path.starts_with("src/") {
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
        "본체 소스를 {files} 개밖에 훑지 못했다(하한 200) — 순회가 좁아지면 아래 \
         \"위반 0\" 은 안 봐서 나온 0 이 된다"
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
        "`{DOOR}` 밖에서 프로세스 전역 env 를 바꾸는 자리가 {} 개다:\n{}\n\n\
         `TASTY_HOME` 이면 `test_support::TastyHomeGuard` 를 써라 — 락만 손으로 잡는 것은 \
         **이행이 아니다.** 그 가드가 묶는 것은 락 하나가 아니라 락 + 이전 값 보관 + \
         `Drop` 복원 셋이고, 락만 잡으면 단언이 패닉했을 때 복원이 안 돌아 **뒤따르는 \
         테스트가 오염된 홈을 물려받는다.** 그 오염은 단독 실행에서 재현되지 않는다.\n\
         다른 키면 `test_support::EnvVarGuard` 를 써라(복원을 `Drop` 이 맡는다).\n\
         프로덕션이 정말 env 를 넘겨야 하면 `PRODUCTION_EXCEPTIONS` 에 **사유와 함께** \
         올려라 — 그 사유는 아래에서 기계가 다시 묻는다.",
        offenders.len(),
        offenders.join("\n")
    );

    // 면제의 사유를 다시 묻는다 — 산문만 남으면 사유가 거짓이 되어도 면제는 살아남는다.
    for (rel, reason) in PRODUCTION_EXCEPTIONS {
        let raw = std::fs::read_to_string(super::repo_root().join(rel)).unwrap_or_default();
        assert!(
            !raw.is_empty(),
            "면제로 적힌 `{rel}` 을 못 읽었다 — 파일이 옮겨졌으면 명부를 따라 옮겨라"
        );
        let masked = mask_non_code(&raw);
        assert!(
            MUTATION.iter().any(|m| masked.contains(m)),
            "`{rel}` 에 env 변경이 더 이상 없다 — 면제가 필요 없어졌으면 명부에서 빼라. \
             남겨 두면 다음 사람이 그 자리를 면제된 것으로 읽는다"
        );
        // 원문에서 묻는다: 마스킹하면 키 리터럴이 통째로 사라져 이 물음이 없어진다.
        for key in HOME_KEYS {
            assert!(
                !raw.contains(key),
                "면제된 `{rel}` 이 이제 {key} 를 부른다 — 사유(\"{reason}\")가 거짓이 됐다. \
                 홈 키를 만지면 그 자리는 `{DOOR}` 를 지나야 한다"
            );
        }
    }

    println!(
        "[홈 env 문] 훑은 본체 소스 {files} · 문 밖 변경 {} (전부 면제)",
        hits.len()
    );
}

/// **락만 손으로 잡는 것도 막는다.**
///
/// [`TastyHomeGuard`] 의 doc 이 "직접 잡을 일은 없다 — 가드가 락 획득과 원값 복원을 함께
/// 맡는다" 고 적는다. 그 문장을 지키는 것이 없었다. 락을 손으로 잡으면 위 시험의 그물
/// (`set_var` 호출)에는 걸리지만, 그 자리가 `EnvVarGuard` 로 값을 바꾸면 걸리지 않는다 —
/// 복원은 되지만 임시 디렉토리 없이 실제 홈을 가리키게 될 수 있다.
#[test]
fn the_home_env_lock_is_only_taken_inside_its_own_module() {
    let mut takers = Vec::new();
    let mut files = 0usize;
    for (rel_path, raw) in super::rust_sources() {
        if !rel_path.starts_with("src/") {
            continue;
        }
        files += 1;
        let rel = rel_path.to_string_lossy().to_string();
        if rel == DOOR {
            continue;
        }
        for (i, line) in mask_non_code(&raw).lines().enumerate() {
            if line.contains("TASTY_HOME_ENV_LOCK") {
                takers.push(format!("  {rel}:{}", i + 1));
            }
        }
    }
    assert!(
        files >= 200,
        "순회가 {files} 개로 좁다 — 위 0 이 공허해진다"
    );
    assert!(
        takers.is_empty(),
        "`{DOOR}` 밖에서 `TASTY_HOME_ENV_LOCK` 을 이름으로 부르는 자리가 {} 개다:\n{}\n\n\
         그 락은 `TastyHomeGuard` 가 잡는다. 손으로 잡는 것은 그 가드를 우회하는 첫 걸음이다",
        takers.len(),
        takers.join("\n")
    );
}
