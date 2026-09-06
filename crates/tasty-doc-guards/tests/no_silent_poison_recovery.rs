//! **락 poison 을 보고 없이 복구하는 자리가 새로 생기지 않는가** 를 워크스페이스 전역에서
//! 본다. 판정 규칙과 근거는 [`tasty_doc_guards::poison_recovery`] 모듈 주석에, 방침은
//! 저장소의 `docs/dev-guide/error-handling.md` "락 poison" 절에 있다.
//!
//! ## 왜 여기(의존 0 크레이트)에 사는가
//!
//! 이 판정은 소스 텍스트만 읽는다(컴파일 불필요). 본체 패키지에 두면 그 유일한 자동
//! 채널은 `check-headless` 인데, 소스만 바뀐 push 에서도 수백 크레이트를 컴파일해야
//! 돈다. 의존 0 인 여기 두면 콜드 빌드가 1 초 미만이라 경로 필터 없이 매 push 에 돈다
//! (`doc-guards.yml`). 배경은 `crate::lib` 주석과 ADR-0138.
//!
//! ## 0 을 통과로 만들지 않는다
//!
//! 스캔이 조용히 죽으면(경로 오타·순회 중단) 모수가 0 이 되고 0 은 언제나 초록이다
//! (ADR-0133). 그래서 위반 목록이 비었다는 단언 **앞에** 훑은 파일 수·집은 복구 자리
//! 수·cfg 로 뺀 수·test-only 로 뺀 수·보고로 통과한 수의 **하한**을 둔다. 어느 하나가
//! 무너지면 검출기의 한 축이 죽은 것이라 실패한다.
//!
//! 하한은 래칫이 아니다 — 늘어도 통과한다. 실측(2026-09-05, main 6b… 이후 회차)으로
//! 잡은 값에 여유를 두고, 그 실측을 옆에 적어 다음 사람이 낡음을 판정할 좌표로 삼는다.

use tasty_doc_guards::poison_recovery::census;
use tasty_doc_guards::repo_root;

const SCAN_ROOTS: &[&str] = &["src", "crates"];

// 실측(2026-09-05): files=1186 · poison_sites=38 · cfg_gated=21 · test_only=6 · reported=11.
// 하한은 그 아래로, 검출기 한 축이 무너질 때만 걸리게 둔다.
//
// ★ 다섯은 **연기 검사**다. 값이 하한 밑으로 내려갔을 때 세계가 둘이고
// (모수가 정말 줄었다 / 수집이 깨졌다), 가장 싼 수선이 **값을 내리는 것**이라
// 가르는 법을 안 적으면 언제나 앞쪽으로 읽힌다. 그래서 각 메시지가 **그 축의
// 판별식**을 싣는다 — 다섯 축이 서로 다른 술어를 쓰므로 판별식도 다섯이 다르다.
// 규율 전문은 docs/dev-guide/guard-population.md 의 "하한에는 판별식이 붙어야 한다".
const MIN_FILES: usize = 1000;
const MIN_POISON_SITES: usize = 30;
const MIN_CFG_GATED: usize = 15;
const MIN_TEST_ONLY: usize = 4;
const MIN_REPORTED: usize = 8;

#[test]
fn no_shipping_lock_is_recovered_without_a_report() {
    let root = repo_root();
    let c = census(&root, SCAN_ROOTS);

    // ── 자기-공허 방지: 검출기가 실제로 무언가를 봤는가 ──────────────────────────
    assert!(
        c.files_scanned >= MIN_FILES,
        "훑은 파일이 {} 개뿐이다(하한 {MIN_FILES}) — 순회가 죽었으면 아래 초록은 \
         거짓이다.\n  \
         [판별식] `git ls-files -- src crates | grep -c '\\.rs$'` 를 세어 이 수의 \
         **움직임**과 맞춰 봐라(두 수는 원래 같지 않다 — 저쪽은 추적되는 파일만 센다). \
         함께 줄었으면 레포가 정말 줄어든 것이고, 저쪽은 그대로인데 이 수만 줄었으면 \
         `SCAN_ROOTS` 가 낡았거나 순회가 죽은 것이다 — 뿌리 이름이 바뀌면 예외가 \
         아니라 **0 개 디렉터리**가 되어 조용히 빠진다.\n  \
         ★ 판별식을 밟지 않고 이 값만 내리지 마라.\n  \
         [정말 줄었으면] 어느 뿌리가 은퇴했는지 위 실측 주석에 적고 값을 내려라.",
        c.files_scanned
    );
    assert!(
        c.poison_sites >= MIN_POISON_SITES,
        "poison 복구를 {} 곳만 집었다(하한 {MIN_POISON_SITES}) — 술어가 죽었을 수 \
         있다.\n  \
         [판별식] 술어가 죽었는지는 추측하지 말고 **부른다**: \
         `cargo test -p tasty-doc-guards --lib poison_recovery`. 그 유닛들이 \
         `into_inner` 인식의 양극을 픽스처로 고정하고 있다(주석 안의 언급은 안 세고, \
         파라미터 바인더는 범위 밖이라는 것까지). 유닛이 초록인데 이 수가 줄었으면 \
         인식은 살아 있고 코드에서 그 형태가 정말 없어진 것이다.\n  \
         ★ 유닛을 안 돌려 보고 이 값만 내리지 마라.\n  \
         [정말 줄었으면] 없어진 자리를 실측 주석에 적고 값을 내려라 — 이 수가 주는 \
         것은 대개 좋은 방향이다(락을 안 쓰게 됐다는 뜻).",
        c.poison_sites
    );
    assert!(
        c.cfg_gated >= MIN_CFG_GATED,
        "cfg(test) 로 뺀 자리가 {} 곳뿐이다(하한 {MIN_CFG_GATED}) — 줄 단위 cfg 판정이 \
         죽으면 이 수가 0 으로 떨어지고 그 자리들이 거짓 위반이 된다.\n  \
         [판별식] 이 축과 바로 아래 `test_only` 축은 **서로 다른 판정**이다 \
         (이쪽은 줄 단위 cfg, 저쪽은 파일 단위 선언). 그러니 둘을 함께 봐라: \
         이 수만 떨어지고 `test_only` 는 그대로면 줄 단위 판정이 깨진 것이고, \
         **둘이 같이** 떨어졌으면 그 자리들이 shipping 으로 옮겨간 것이다 — 뒤쪽이면 \
         아래 실판정이 그 자리를 위반으로 물어야 정상이다. 안 물었다면 그것이 사고다.\n  \
         ★ 두 수를 함께 보지 않고 이 값만 내리지 마라.\n  \
         [정말 줄었으면] 어느 자리가 cfg 를 잃었는지 적고 값을 내려라.",
        c.cfg_gated
    );
    assert!(
        c.test_only_sites >= MIN_TEST_ONLY,
        "test-only 파일로 뺀 자리가 {} 곳뿐이다(하한 {MIN_TEST_ONLY}) — 선언 기반 판정이 \
         죽었을 수 있다.\n  \
         [판별식] 이 축은 이 파일이 아니라 **공용 판정기**가 답한다 \
         (`tasty_doc_guards::shipping_scope::test_only_files`, ADR-0180 이 그것을 정본으로 \
         박았다). 그러니 여기서 흉내 내지 말고 그 판정기의 유닛을 부른다: \
         `cargo test -p tasty-doc-guards --lib shipping_scope`. 그것이 초록인데 이 수가 \
         줄었으면 판정은 살아 있고 test-only 선언이 정말 줄어든 것이다. 그것이 빨가면 \
         이 하한이 아니라 **거기**를 고쳐야 한다 — 이 값을 내리면 공용 판정기의 고장이 \
         이 자리에서 초록이 된다.\n  \
         ★ 공용 판정기를 안 돌려 보고 이 값만 내리지 마라.\n  \
         [정말 줄었으면] 선언이 사라진 파일을 적고 값을 내려라.",
        c.test_only_sites
    );
    assert!(
        c.reported >= MIN_REPORTED,
        "보고로 통과한 자리가 {} 곳뿐이다(하한 {MIN_REPORTED}) — 보고 판정이 죽으면 \
         자기보고 자리가 거짓 위반이 된다.\n  \
         [판별식] 보고 인식의 양극은 유닛이 쥐고 있다 — \
         `a_closure_with_a_report_is_not_silent` 와 `a_multiline_logged_arm_is_reported` \
         (`cargo test -p tasty-doc-guards --lib poison_recovery`). 둘이 초록인데 이 수만 \
         줄었으면 인식은 살아 있다. 그리고 이 축은 `silent` 와 **합이 보존된다**: \
         보고가 준 만큼 조용한 자리가 늘었으면 아래 실판정이 그것을 물어야 한다. \
         이 수만 줄고 `silent` 도 비어 있으면 그 자리들이 아예 사라진 것이다.\n  \
         ★ 유닛을 안 돌려 보고 이 값만 내리지 마라.\n  \
         [정말 줄었으면] 보고를 잃은(또는 없어진) 자리를 적고 값을 내려라.",
        c.reported
    );

    // ── 실판정: shipping 코드의 조용한 복구는 0 이어야 한다 ───────────────────────
    assert!(
        c.silent.is_empty(),
        "락 poison 을 보고 없이 복구하는 자리가 {} 곳 있다. 각 자리는 헬퍼\n\
         (`tasty_utils::poison::recover_*`)를 거치거나 복구 arm 에서 직접 보고해야 한다.\n\
         근거는 docs/dev-guide/error-handling.md \"락 poison\" ②축:\n{}",
        c.silent.len(),
        c.silent
            .iter()
            .map(|s| format!("  {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
