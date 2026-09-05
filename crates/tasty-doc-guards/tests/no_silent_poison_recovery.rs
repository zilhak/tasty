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
        "훑은 파일이 {} 개뿐이다(하한 {MIN_FILES}) — 순회가 죽었으면 아래 초록은 거짓이다",
        c.files_scanned
    );
    assert!(
        c.poison_sites >= MIN_POISON_SITES,
        "poison 복구를 {} 곳만 집었다(하한 {MIN_POISON_SITES}) — 술어가 죽었을 수 있다",
        c.poison_sites
    );
    assert!(
        c.cfg_gated >= MIN_CFG_GATED,
        "cfg(test) 로 뺀 자리가 {} 곳뿐이다(하한 {MIN_CFG_GATED}) — 줄 단위 cfg 판정이 \
         죽으면 이 수가 0 으로 떨어지고 그 자리들이 거짓 위반이 된다",
        c.cfg_gated
    );
    assert!(
        c.test_only_sites >= MIN_TEST_ONLY,
        "test-only 파일로 뺀 자리가 {} 곳뿐이다(하한 {MIN_TEST_ONLY}) — 선언 기반 판정이 \
         죽었을 수 있다",
        c.test_only_sites
    );
    assert!(
        c.reported >= MIN_REPORTED,
        "보고로 통과한 자리가 {} 곳뿐이다(하한 {MIN_REPORTED}) — 보고 판정이 죽으면 \
         자기보고 자리가 거짓 위반이 된다",
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
