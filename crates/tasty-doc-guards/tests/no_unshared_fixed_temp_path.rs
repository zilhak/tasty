//! **공유 temp 아래 고정 이름 임시 경로가 새로 생기지 않는가** 를 워크스페이스 전역에서
//! 본다(ADR-0129 형태 B). 판정 규칙·극성·유니크화 인정 기준·R16 사각은
//! [`tasty_doc_guards::temp_path`] 모듈 주석에 있다.
//!
//! ## 극성과 사유 — 왜 명부가 아닌가
//!
//! "이 이름은 유일해야 하는가" 는 의도를 읽어야 답하므로 소스만으로 못 푼다. 그래서
//! **기본값을 "유니크화돼야 한다"** 로 두고, 의도된 공유는 **그 자리에 사유**(`이유:`/
//! `reason:`)로 적는다 — `check-allow-reason` 의 마커 관례를 그대로 빌린다(그 스크립트는
//! `#[allow]` 만 보므로 스캐너 자체는 재사용 못 한다, 관례만 빌린다). 명부를 안 쓰는 이유:
//! 명부는 자기 대상을 이름으로 지목해 "쓰이는 것" 으로 만들고(R395), 정당한 예외가
//! 구조적인 곳에서는 지키려는 표보다 빨리 썩는다(R380).
//!
//! ## 이 부류의 창립 멤버 (R399)
//!
//! 면제(=사유) 기구를 처음 들이는 것은 **부류를 만드는 일**이다. 이 가드가 드는 첫
//! 예외는 실측(2026-09-05)으로 잡은 **다섯**이고, 전부 "의도된 공유" 다:
//! - 사용자 config 폴백 넷 — `~/.tasty/X` 우선, 홈 미해결에서만 temp. 인스턴스 격리가
//!   목적이 아니라 사용자 설정이라 공유가 옳다(caller 가 `exists=false` 로 인지).
//!   (`core/file.rs` · `core/state.rs` · `webhook/config.rs` · `webhook/persist.rs`)
//! - 클립보드 temp 디렉터리 하나 — 디렉터리는 공유하되 그 안 파일명이
//!   `paste-<millis>.png` 로 매번 다르다. 격리는 디렉터리가 아니라 파일명이 진다.
//!   (`view/main/clipboard.rs`)
//!
//! ## 0 을 통과로 만들지 않는다
//!
//! 스캔이 죽으면 모수가 0 이 되고 0 은 언제나 초록이다(ADR-0133). 위반 목록이 비었다는
//! 단언 **앞에** 훑은 파일 수·경로 짓는 자리 수·유니크화된 수·사유로 통과한 수의 하한을
//! 둔다 — 각 갈래가 죽으면 그 하나가 무너진다(하나의 총수로 세지 않는다). 하한은
//! 래칫이 아니라 여유를 둔 바닥이다.

use tasty_doc_guards::repo_root;
use tasty_doc_guards::temp_path::census;

const SCAN_ROOTS: &[&str] = &["src", "crates"];

// 실측(2026-09-05): files=1188 · sites=29 · uniquified=24 · reasoned=5.
const MIN_FILES: usize = 1000;
const MIN_SITES: usize = 20;
const MIN_UNIQUIFIED: usize = 15;
const MIN_REASONED: usize = 3;

#[test]
fn every_temp_path_is_uniquified_or_reasoned() {
    let root = repo_root();
    let c = census(&root, SCAN_ROOTS);

    // ── 자기-공허 방지: 갈래마다 선다 ──────────────────────────────────────────
    assert!(
        c.files_scanned >= MIN_FILES,
        "훑은 파일이 {} 개뿐이다(하한 {MIN_FILES}) — 순회가 죽었으면 아래 초록은 거짓이다",
        c.files_scanned
    );
    assert!(
        c.sites >= MIN_SITES,
        "경로 짓는 temp_dir 자리를 {} 곳만 집었다(하한 {MIN_SITES}) — 자리 판정이 죽었을 수 있다",
        c.sites
    );
    assert!(
        c.uniquified >= MIN_UNIQUIFIED,
        "유니크화된 자리가 {} 곳뿐이다(하한 {MIN_UNIQUIFIED}) — 유니크화 인식이 죽으면 \
         이 수가 떨어지고 그 자리들이 거짓 위반이 된다",
        c.uniquified
    );
    assert!(
        c.reasoned >= MIN_REASONED,
        "사유로 통과한 자리가 {} 곳뿐이다(하한 {MIN_REASONED}) — 사유 인식이 죽었을 수 있다",
        c.reasoned
    );

    // ── 실판정: 유니크화도 사유도 없는 고정 이름은 0 이어야 한다 ──────────────────
    assert!(
        c.silent.is_empty(),
        "공유 temp 아래 고정 이름 임시 경로가 {} 곳 있다. 인스턴스/완주가 동시에 살면\n\
         같은 파일을 truncate 하거나 서로의 디렉터리를 지운다(ADR-0129 형태 B).\n\
         유니크화(pid·`tempfile`·`TempDir`)하거나, 공유가 의도라면 그 자리에 `이유:` 를 적어라:\n{}",
        c.silent.len(),
        c.silent
            .iter()
            .map(|s| format!("  {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
