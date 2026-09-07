//! **환경변수·cwd 를 만지는 테스트가 직렬화 없이 만지지 않는가** 를 워크스페이스 전역에서
//! 본다(ADR-0129 형태 A). 판정 규칙·극성·직렬화 인정 기준·R16 사각은
//! [`tasty_doc_guards::env_isolation`] 모듈 주석에 있다.
//!
//! ## 극성과 사유 — 왜 명부가 아닌가
//!
//! "이 테스트가 전역을 만지나" 가 아니라 **"직렬화 없이 만지나"** 를 묻는다(temp_path 축과
//! 같은 뒤집힌 극성). env/cwd 변형은 그 자리(enclosing 함수)에서 직렬화를 밝혀야 한다 —
//! 락 참조(`SERIAL`/`*_LOCK`.lock()), 직렬화 마커(`직렬화`/`이유:`/`reason:`), 또는 단일
//! `#[test]` 격리. 명부를 안 쓰는 이유는 temp_path 와 같다(R395·R380).
//!
//! ## 왜 리포 전역 한 자리인가 (R384)
//!
//! 강제 패턴(`tasty-host-plugin` 자기스캔)은 "가드가 통째로 한 파일" 형태라 세 크레이트
//! (인라인 struct 를 쓰는 telemetry·settings, 프로덕션 env 를 set 하는 본체)에 그대로
//! 안 옮겨진다. 그래서 효과는 복제하되 형태는 리포 전역으로 둔다 — 모듈 주석 참조.
//!
//! ## 이 부류의 창립 멤버 (R399)
//!
//! 마커(자리 사유) 기구를 처음 들이는 것은 부류를 만드는 일이다. 이 가드가 인정하는 실측
//! (2026-09-05) 직렬화 자리는 전부 **RAII env 가드의 내부**(set/unset/drop)이고, 락을
//! 호출부가 쥐거나 단일 `#[test]` 로 격리한다:
//! - `tasty-telemetry` `AgentIdEnvGuard` · `tasty-cli` `SurfaceIdEnvGuard` ·
//!   본체 `EnvVarGuard` — 각 메서드가 그 자리 주석으로 직렬화 조건을 밝힌다.
//!
//! ## 0 을 통과로 만들지 않는다
//!
//! 스캔이 죽으면 모수가 0 이 되고 0 은 언제나 초록이다(ADR-0133). 위반 목록이 비었다는
//! 단언 **앞에** 훑은 파일 수·변형 자리 수·직렬화로 통과한 수의 하한을 둔다 — 각 갈래가
//! 죽으면 그 하나가 무너진다(하나의 총수로 세지 않는다). 하한은 여유를 둔 바닥이다.
//!
//! ## 총수의 하한은 뿌리 하나를 삼킨다 (그래서 뿌리마다 따로 건다)
//!
//! 총수 하한은 **여유가 작은 뿌리보다 크면** 그 뿌리가 통째로 빠져도 안 문다. 실측
//! (2026-09-07): 세 뿌리 합 1298 에 총수 하한 1100 이라 여유가 198 이고, 루트 `tests/`
//! 는 59 다 — `SCAN_ROOTS` 에서 `tests` 를 지워도 1239 라 **초록**이다. 그래서 총수와
//! 별개로 뿌리마다 바닥을 건다.
//!
//! **무엇이 조용하고 무엇이 이미 시끄러운지 갈라 둔다** — 이 하한이 무는 것은 앞의 둘뿐이다:
//! - (나) `SCAN_ROOTS` 상수에서 뿌리를 뺀다 → **조용하다.** 이 하한이 문다.
//! - (다) 뿌리는 있는데 `.rs` 가 하나도 없다 → **조용하다**(빈 디렉터리는 읽기 성공이다).
//!   이 하한이 문다.
//! - (가) 디렉터리가 없어지거나 이름이 바뀐다 → **이미 시끄럽다.** 이 순회기
//!   ([`tasty_doc_guards::source_text::rust_sources`])는 `read_dir` 실패에 **panic** 한다.
//!   ★ 이 하한이 (가)를 막는 것이 아니다 — panic 이 막는다. 그 panic 을 삼키는 형태로
//!   바꾸면 (가)도 조용해지고, 그때는 이 하한이 그것까지 떠맡아야 한다.
//!   ☆ 형제 [`tasty_doc_guards::floored_walk`] 는 반대다: `let Ok(..) else { return; }`
//!   로 실패를 삼켜 0 이 된다(그래서 그쪽에 Floor 기계가 붙어 있다). **두 순회기의 실패
//!   의미가 반대이니 한쪽의 서술을 다른 쪽에 옮겨 쓰지 마라.**
//!
//! ## 수는 초록일 때도 남긴다
//!
//! 뿌리별 수는 단정 **앞**에서 `eprintln!` 로 싣는다 — 빨간 경로에서도 모수가 남게. libtest
//! 가 통과한 테스트의 출력은 삼키므로 초록에서 읽으려면 이렇게 부른다:
//!
//! ```text
//! cargo test -p tasty-doc-guards --test no_unserialized_env_mutation -- --nocapture
//! ```

use tasty_doc_guards::env_isolation::census;
use tasty_doc_guards::repo_root;

// 스캔 범위. `src`·`crates` 에 더해 **레포 루트 `tests/`(통합 테스트)** 를 본다 — 이
// 가드가 겨냥하는 "테스트가 프로세스 전역(env/cwd)을 직렬화 없이 만짐" 이 가장 잘 나는
// 곳이 통합 테스트다(공유 인스턴스·격리 HOME·포트 파일). `crates/*/tests/` 는 `crates`
// 순회에 이미 들어왔고, [`tasty_doc_guards::shipping_scope::test_only_files`] 가 그 둘을
// cargo 통합테스트 타깃으로 test 맥락에 넣는다(정본 판정기) — 그전엔 루트 `tests/*.rs` 가
// `mod` 선언 없는 루트 파일이라 test 맥락 밖으로 조용히 새던 사각이었다. 이 셋 밖
// (`benches`·`examples`)은 이 레포에 없어 제외한다 — 누락이 아니라 범위 결정이다.
const SCAN_ROOTS: &[&str] = &["src", "crates", "tests"];

// 실측(2026-09-05, 루트 tests/ 편입 후): files=1257 · mutations=25 · serialized=25 · bare=0.
//
// ★ 셋은 **연기 검사**다. 하한 밑으로 내려갔을 때 세계가 둘이고(모수가 정말 줄었다 /
// 수집이 깨졌다) 가장 싼 수선이 값을 내리는 것이라, 가르는 법을 안 적으면 언제나
// 앞쪽으로 읽힌다. 그래서 각 메시지가 그 축의 **판별식**을 싣는다.
// ★★ 이 셋에는 다른 가드에 없는 판별 수단이 하나 더 있다 — **보존식**이다:
//   `serialized + bare = mutations`. 실측에서 bare=0 이라 두 수가 정확히 같다.
//   그 등식이 깨지는 방식이 어느 판정이 죽었는지를 말해 준다.
// 규율 전문은 docs/dev-guide/guard-population.md 의 "하한에는 판별식이 붙어야 한다".
const MIN_FILES: usize = 1100;

// 뿌리별 하한. **래칫이 아니라 뿌리 생존 검사다.**
//
// ★ 이 하한이 무는 것은 (나) `SCAN_ROOTS` 에서 뿌리를 뺀 경우와 (다) 뿌리는 있는데 빈
//   경우다. (가) 디렉터리 소실·개명은 **panic 이 이미 막는다** — 순회기가 `read_dir`
//   실패에 panic 하고, 빈 디렉터리는 읽기가 성공하므로 여기까지 온다. 다음 사람이 알아야
//   할 전부가 이 세 줄이다. 이 하한을 "디렉터리 소실도 막는다" 로 읽고 panic 쪽을 지우지 마라.
//
// 실측(2026-09-07): src 596 · crates 643 · tests 59 (합 1298 — `git ls-files` 와 census
// 술어를 흉내 낸 `find` 두 계기가 같은 값을 냈다).
//
// 값을 현재 수의 대략 3/4 로 둔 이유: 뿌리가 순회에서 빠지면 그 수는 **0** 이 되므로 어떤
// 양수 하한이든 문다. 그러니 남는 여유는 "무는 폭" 이 아니라 **파일이 실제로 줄 때 거짓
// 빨강을 안 내려는 폭**이다. 둘은 다른 물음이라 여기서 폭을 아낄 이유가 없다.
// ★ 이 값을 현재 수 가까이 **올리지 마라.** 올리면 래칫이 되고, 래칫이면 묻는 것이
//   "뿌리가 살아 있나" 가 아니라 "파일이 줄었나" 로 바뀐다 — 이 자리가 답하는 물음이
//   아니고, 리팩터링마다 거짓 빨강을 낸다.
const MIN_PER_ROOT: &[(&str, usize)] = &[("src", 450), ("crates", 480), ("tests", 40)];
const MIN_MUTATIONS: usize = 15;
const MIN_SERIALIZED: usize = 15;

#[test]
fn every_test_env_mutation_is_serialized() {
    let root = repo_root();
    let c = census(&root, SCAN_ROOTS);

    // 단정보다 **앞**에 둔다 — 빨간 경로에서도 모수가 남아야 한다(형제 가드와 같은 형태).
    eprintln!(
        "[env-isolation] 파일 {} (뿌리별 {}) · 변형 {} · 직렬화 {} · 위반 {}",
        c.files_scanned,
        c.per_root
            .iter()
            .map(|(r, n)| format!("{r}={n}"))
            .collect::<Vec<_>>()
            .join(" "),
        c.mutations,
        c.serialized,
        c.bare.len()
    );

    // ── 자기-공허 방지: 갈래마다 선다 ──────────────────────────────────────────
    assert!(
        c.files_scanned >= MIN_FILES,
        "훑은 파일이 {} 개뿐이다(하한 {MIN_FILES}) — 순회가 죽었으면 아래 초록은 \
         거짓이다.\n  \
         [판별식] **위에 찍힌 뿌리별 수를 봐라.** 어느 뿌리가 0 이면 그 뿌리가 \
         `SCAN_ROOTS` 에서 빠졌거나 비었고, 셋이 **함께** 줄었으면 레포가 정말 줄어든 \
         것이다. 뿌리별 하한이 앞의 경우를 따로 문다.\n  \
         ★ 이 총수만 내리지 마라 — 이 수의 여유는 작은 뿌리 하나를 통째로 삼킨다.\n  \
         [정말 줄었으면] 무엇이 없어졌는지 위 실측 주석에 적고 값을 내려라.",
        c.files_scanned
    );
    // 뿌리마다 따로 — 총수의 여유가 작은 뿌리를 삼키기 때문이다(모듈 주석 참조).
    assert_eq!(
        c.per_root.len(),
        SCAN_ROOTS.len(),
        "census 가 뿌리별 수를 {} 개 냈다(뿌리는 {} 개다) — 뿌리 나눔 자체가 깨졌다.",
        c.per_root.len(),
        SCAN_ROOTS.len()
    );
    assert_eq!(
        c.per_root.iter().map(|(_, n)| n).sum::<usize>(),
        c.files_scanned,
        "뿌리별 수의 합이 총수와 다르다 — 어느 파일도 두 번 세지 않고 하나에는 붙어야 \
         한다. 뿌리가 서로 겹치거나 경로 접두 판정이 깨진 것이다."
    );
    for &(root_name, floor) in MIN_PER_ROOT {
        let got = c
            .per_root
            .iter()
            .find(|(r, _)| r == root_name)
            .map(|(_, n)| *n);
        assert!(
            got.is_some(),
            "하한을 건 뿌리 `{root_name}` 이 census 의 뿌리 목록에 없다 — `SCAN_ROOTS` 와 \
             `MIN_PER_ROOT` 가 어긋났다. 뿌리를 **뺐다면** 그것이 바로 이 검사가 겨냥하는 \
             변경이다. 여기서 값을 지우기 전에 왜 그 뿌리를 안 봐도 되는지 위 주석에 적어라."
        );
        let got = got.unwrap_or(0);
        assert!(
            got >= floor,
            "뿌리 `{root_name}` 에서 훑은 파일이 {got} 개다(하한 {floor}) — 이 뿌리가 \
             순회에서 빠졌거나 비었다.\n  \
             [판별식] **0 인가 아닌가로 먼저 갈라라.** 0 이면 뿌리가 `SCAN_ROOTS` 에서 \
             빠졌거나 그 아래 `.rs` 가 하나도 없는 것이다 — 디렉터리가 없어지거나 이름이 \
             바뀐 경우는 여기까지 오지 못한다(순회기가 그 앞에서 panic 한다). 0 이 \
             아니면서 하한 밑이면 그 뿌리가 정말 줄어든 것이다.\n  \
             [정말 줄었으면] 없어진 것을 위 실측 주석에 적고 이 값을 내려라. \
             ★ 총수 하한만 고쳐서는 안 된다 — 총수는 이 뿌리를 삼킨다."
        );
    }
    assert!(
        c.mutations >= MIN_MUTATIONS,
        "test 맥락 env/cwd 변형을 {} 곳만 집었다(하한 {MIN_MUTATIONS}) — 변형 판정 또는 \
         cfg(test) 판정이 죽었을 수 있다.\n  \
         [판별식] 이 수는 **두 판정의 곱**이라(무엇이 변형인가 × 그것이 test 맥락인가) \
         떨어졌다는 사실만으로는 어느 쪽인지 안 갈린다. 유닛이 두 축을 따로 쥐고 있으니 \
         부르면 갈린다 — `cargo test -p tasty-doc-guards --lib env_isolation`: \
         `a_bare_set_var_in_test_code_is_caught` 와 `set_current_dir_is_also_covered` 가 \
         변형 축, `a_production_env_mutation_is_out_of_scope` 와 \
         `a_mutation_in_a_test_only_file_is_in_scope` 가 맥락 축이다. \
         전부 초록인데 이 수가 줄었으면 두 판정 다 살아 있고 그런 코드가 정말 준 것이다.\n  \
         ★ 어느 축이 죽었는지 안 가르고 이 값만 내리지 마라.\n  \
         [정말 줄었으면] 어느 자리가 없어졌는지 적고 값을 내려라 — 이 수가 주는 것은 \
         대개 좋은 방향이다(테스트가 프로세스 전역을 덜 만진다는 뜻).",
        c.mutations
    );
    assert!(
        c.serialized >= MIN_SERIALIZED,
        "직렬화로 통과한 자리가 {} 곳뿐이다(하한 {MIN_SERIALIZED}) — 직렬화 인식이 죽으면 \
         이 수가 떨어지고 그 자리들이 거짓 위반이 된다.\n  \
         [판별식] **보존식으로 먼저 갈라라**: `serialized + bare = mutations` 다. \
         이 수가 줄었는데 바로 아래 `bare` 가 그만큼 늘었으면 인식이 죽어 정상 자리가 \
         거짓 위반이 된 것이고(그때는 실판정이 시끄러워지므로 이 하한까지 올 일이 \
         드물다), 이 수와 `mutations` 가 **함께** 줄고 `bare` 가 여전히 비었으면 그 \
         자리들이 정말 없어진 것이다.\n  \
         그다음 인식 자체는 유닛이 쥔다 — `--lib env_isolation` 의 \
         `a_lock_in_scope_passes` · `a_marker_comment_passes` · \
         `a_single_test_containment_marker_passes`, 그리고 음성 쪽 \
         `a_marker_inside_a_string_does_not_count`. 마지막 것이 특히 중요하다: \
         그것이 죽으면 인식이 **너무 많이** 통과시켜 이 수가 오히려 커진다.\n  \
         ★ 보존식을 안 맞춰 보고 이 값만 내리지 마라.\n  \
         [정말 줄었으면] 없어진 자리를 적고 값을 내려라.",
        c.serialized
    );

    // ── 실판정: 직렬화 증거 없는 test env/cwd 변형은 0 이어야 한다 ────────────────
    assert!(
        c.bare.is_empty(),
        "직렬화 증거 없이 프로세스 전역(env/cwd)을 만지는 테스트 자리가 {} 곳 있다.\n\
         병렬 cargo test 에서 서로의 상태를 덮어 순서 의존 flake 를 낳는다(ADR-0129 형태 A).\n\
         직렬화 락을 그 함수에서 쥐거나, RAII 가드로 감싸 그 자리 주석에 직렬화 조건을 밝혀라:\n{}",
        c.bare.len(),
        c.bare
            .iter()
            .map(|s| format!("  {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
