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

use tasty_doc_guards::env_isolation::census;
use tasty_doc_guards::repo_root;

const SCAN_ROOTS: &[&str] = &["src", "crates"];

// 실측(2026-09-05): files=1196 · mutations=25 · serialized=25 · bare=0.
const MIN_FILES: usize = 1000;
const MIN_MUTATIONS: usize = 15;
const MIN_SERIALIZED: usize = 15;

#[test]
fn every_test_env_mutation_is_serialized() {
    let root = repo_root();
    let c = census(&root, SCAN_ROOTS);

    // ── 자기-공허 방지: 갈래마다 선다 ──────────────────────────────────────────
    assert!(
        c.files_scanned >= MIN_FILES,
        "훑은 파일이 {} 개뿐이다(하한 {MIN_FILES}) — 순회가 죽었으면 아래 초록은 거짓이다",
        c.files_scanned
    );
    assert!(
        c.mutations >= MIN_MUTATIONS,
        "test 맥락 env/cwd 변형을 {} 곳만 집었다(하한 {MIN_MUTATIONS}) — 변형 판정 또는 \
         cfg(test) 판정이 죽었을 수 있다",
        c.mutations
    );
    assert!(
        c.serialized >= MIN_SERIALIZED,
        "직렬화로 통과한 자리가 {} 곳뿐이다(하한 {MIN_SERIALIZED}) — 직렬화 인식이 죽으면 \
         이 수가 떨어지고 그 자리들이 거짓 위반이 된다",
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
