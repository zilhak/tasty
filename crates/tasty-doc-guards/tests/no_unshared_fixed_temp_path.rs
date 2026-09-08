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
use tasty_doc_guards::source_text::mask_non_code;
use tasty_doc_guards::temp_path::{Census, census, discriminator_chains};

// 스캔 범위. `src`·`crates` 에 더해 **레포 루트 `tests/`(통합 테스트)** 를 본다 — 이
// 가드가 겨냥하는 "프로세스 전역 자원을 직렬화 없이 공유" 가 가장 잘 나는 곳이 통합
// 테스트다(공유 인스턴스·격리 HOME·포트 파일). 이 셋 밖(`benches`·`examples`·루트
// 아닌 스크립트)은 flaky 테스트가 살지 않아 제외한다 — 누락이 아니라 범위 결정이다.
// (이전엔 `["src","crates"]` 로 근거 없이 루트 tests/ 를 빠뜨려 사거리 구멍이었다.)
const SCAN_ROOTS: &[&str] = &["src", "crates", "tests"];

// 실측(2026-09-05, 루트 tests/ 편입 후): files=1256 · sites=55 · uniquified=49 · reasoned=6.
//
// ★ 아래 넷은 **연기 검사**다. 값이 하한 밑으로 내려갔을 때 세계가 둘이고
// (모수가 정말 줄었다 / 수집이 깨졌다), 가장 싼 수선이 **값을 내리는 것**이라
// 가르는 법을 안 적으면 언제나 앞쪽으로 읽힌다. 그래서 각 실패 메시지가
// **그 자리의 판별식**을 싣는다 — 넷이 서로 다르다.
// 규율 전문은 docs/dev-guide/guard-population.md 의 "하한에는 판별식이 붙어야 한다".
const MIN_FILES: usize = 1100;
const MIN_SITES: usize = 40;
const MIN_UNIQUIFIED: usize = 35;
const MIN_REASONED: usize = 4;

/// 창(`JOIN_WINDOW`) 안에 `.join(` 이 없어 **자리로도 안 세어진** `temp_dir()` 줄의 수.
/// 하한이 아니라 **양방향 래칫**이다 — 실측 2026-09-06: 4.
///
/// 왜 이 칸이 필요한가: 창을 넘겨 경로를 짓는 자리는 위반으로 잡히지 않고 **조용히
/// 빠진다.** 창을 넓혀 잡으려 하면 안 된다 — 실측(계단 11 점)으로 6~24 는 완전히
/// 평평하고, 40 과 400 에서 붙는 넷은 전부 **가짜 짝**이다(`valid.join(", ")` 같은
/// 문자열 join, 28 줄 아래 다른 테스트의 `.join(`). 400 에서는 `reasoned` 가 6 → 2 로
/// 무너진다. 그래서 창은 그대로 두고 **빠지는 수가 움직이는 것**을 본다.
///
/// `check-allow-reason` 의 래칫과 **극성은 같다** — 늘면 새로 안 보는 구간이 생긴
/// 것이고, 줄었는데 값을 안 내리면 **남는 여유가 곧 안 보는 구간**이 된다. 다만
/// **같은 것은 극성뿐이다**: 저쪽은 억제의 총량을 세고 이쪽은 검사에서 빠진 자리를
/// 센다. 그리고 저쪽 실패 메시지가 갖고 있던 금지 문구("상한을 올려서 통과시키지
/// 마라")를 이쪽은 안 옮겨 놓고 "같다" 고만 적어 두었었다 — 인용이 보증처럼 읽혔다.
/// 아래 메시지가 그 금지를 직접 싣는다.
/// 오늘 이 4 는 전부 정당한 읽기 전용 전달로 보이지만, 그 판단은 소스로 못 굳힌다 —
/// 두 부류가 한 칸에 섞여 있다. 그래서 값이 아니라 **움직임**을 지킨다.
const UNPAIRED_RATCHET: usize = 4;

/// **같은 프로세스의 재호출을 못 가르는** 자리의 수. 하한이 아니라 **양방향 래칫**이다.
///
/// **그 38 을 낳는 술어 — 수가 아니라 이것이 값이다.**
/// `sites` 중 경로 짓는 창(`JOIN_WINDOW` = 6, 바인딩 변수 경유 포함) 안에 `process` 축은
/// 있고 **재호출을 가르는 축(`per_call`·`counter`·`clock`)이 하나도 없는** 자리의 수.
/// 그 자리에 사유가 붙어 있으면 안 센다(`weak_only` 와 같은 물음을 같은 자리에서
/// 다시 묻는다 — `reasoned` 칸은 축이 **하나도 없을 때만** 도달하므로 축이 있는
/// 자리의 사유는 그 칸으로 안 빠진다). 실측 2026-09-08, 잰 트리 `2b0176eef`: **38**.
/// 그때 사유가 붙어 제외된 자리는 0 이었다. 그 뒤 통합에서 한 번 40 으로 짖었고
/// (다른 lane 이 이 칸에 드는 자리 둘을 새로 들였다), `aa61d2537` 이 그 둘에 사유를
/// 붙여 다시 38 이 됐다 — 즉 지금 사유로 제외되는 자리가 **2** 다.
///
/// ★ 술어를 한 번 좁혔다. 처음엔 "축이 `process` **하나뿐인** 자리" 라고 적었고 칸
///   이름도 `process_only` 였다. **그 문장이 틀렸다** — 2026-09-08 통합에서 이 래칫이
///   실측 40 으로 짖었고, 늘어난 둘이 `process::id()` **와** `thread::current().id()`
///   를 함께 쓰는 자리였다. "process 하나뿐" 이 사실이 아닌 자리가 이 칸에 있었다.
///   **좌변은 안 바뀌었다** — 스레드 id 는 재호출을 못 가르므로 그 둘은 그대로 여기
///   든다. 바뀐 것은 이름과 문장이다(`Axes::blind_to_recall`).
///
/// ## 38 을 갈랐다 (실측 2026-09-08, `integration-94` `6dcf6b7db`) — 잔여 0
///
/// 한때 여기 "**38 은 아직 안 가른 수다**… 다음 회차의 첫 일이 그 분할이다" 라고 적혀
/// 있었다. 갈랐다. 전수이고 미분류 0 이다:
///
/// | 부류 | 수 | 같은 프로세스의 재호출이 있나 | 그 **자리에서** 읽히나 |
/// |---|---|---|---|
/// | (가1) `#[test]` 본문 직속 · 루프 없음 · 접두 고유 | 30 | 없다 | **읽힌다** |
/// | (가2) 호출자 판별자(`tag`/`suffix`/`surface_id`)를 받는 헬퍼 | 8 | 오늘은 없다 | 안 읽힌다 |
/// | (나) 그냥 안 유일화된 자리 | **0** | — | — |
/// | 반복문 안 | **0** | — | — |
///
/// **그래서 이 38 을 "고칠 것 38 개" 로 읽으면 안 된다 — 고칠 것은 0 이다.** 이 래칫이
/// 지키는 것은 빚이 아니라 **멤버십**이다: 새 자리가 이 칸에 들어오면 그것은 위 셋 중
/// 어디에도 아직 안 속하므로 그 자리에서 판단을 받아야 한다.
///
/// 어떻게 쟀나(셋 다 소스에서 기계로 읽었다):
/// - **30**: 자리를 감싸는 `fn` 이 `#[test]` 이고 그 사이에 반복문이 없다. cargo 하네스는
///   `#[test]` 를 한 프로세스에서 **한 번** 부른다 — 그래서 재호출이 없다.
/// - **접두 고유**: 38 자리의 고정 접두를 바이너리별로 묶어 겹침을 셌다 — **고유 38 ·
///   겹침 0 · 접두를 못 읽은 자리 0**. 이 확인이 필요한 이유: 한 바이너리의 두 `#[test]`
///   가 같은 접두를 쓰면 pid 도 스레드 id 도 그 둘을 못 가른다(그 둘은 같은 프로세스다).
/// - **8**: 호출자를 전부 따라가 실제 판별자를 읽었다 — `line!()` · 루프 첨자 `{i}` ·
///   서로 다른 리터럴. **중복 0.** 두 자리는 한 단계 더 위에서 왔고 거기까지 따라갔다.
///
/// ★ **한 문장을 정정한다.** 여기 "(가)를 소스에서 못 읽는다" 고 적혀 있었다. **38 중
/// 30 은 읽힌다** — `#[test]` 라는 사실이 그 자리에 있다. 못 읽는 것은 나머지 8 이고,
/// 그것도 "읽을 수 없다" 가 아니라 **그 자리만으로는 안 되고 호출자를 다 봐야 한다** 다.
/// 호출자가 넘긴 판별자를 유일화 성분으로 안 받는 이유는 그대로다(`PROCESS_TOKENS` 의
/// `path_for` 주석) — 성분의 값이 이 파일 밖에 있다.
///
/// ★ **남은 위험은 8 쪽이고 오늘 그것을 보는 눈이 없다.** 새 호출자가 이미 쓰인 판별자를
/// 그대로 넘기면 두 자리가 같은 경로가 되는데, 이 가드는 헬퍼 **선언**만 보므로 그 충돌을
/// 못 본다. 위 「중복 0」은 손으로 따라가 잰 값이지 자동으로 지켜지는 값이 아니다.
///
/// 왜 hard-fail 이 아닌가: (가2) 8 의 답이 이 파일 밖에 있고, 그것을 위반으로 내면
/// 실재하지 않는 위반에 대한 처방이 된다. 그래서 값이 아니라 **움직임**을 지킨다.
const RECALL_BLIND_RATCHET: usize = 38;

/// 하한·래칫 값 묶음. 판정 함수의 **인자**다.
///
/// 왜 상수를 직접 읽지 않는가: 값이 함수 안에 박혀 있으면 그 판정을 발화시키려면
/// **레포를 그 값 밑으로 줄이는 수밖에 없다** — 즉 실패 문구를 합성 입력으로 못 읽는다.
/// 인자로 빼면 픽스처가 자기 하한을 들고 와서 문구를 그 자리에서 읽을 수 있다(R1072).
struct Floors {
    files: usize,
    sites: usize,
    uniquified: usize,
    unpaired: usize,
    reasoned: usize,
    recall_blind: usize,
}

const FLOORS: Floors = Floors {
    files: MIN_FILES,
    sites: MIN_SITES,
    uniquified: MIN_UNIQUIFIED,
    unpaired: UNPAIRED_RATCHET,
    recall_blind: RECALL_BLIND_RATCHET,
    reasoned: MIN_REASONED,
};

/// census 와 하한만 보고 실패 문구를 만든다 — 파일시스템을 안 읽는다.
///
/// 빈 vec 이 통과다. 갈래마다 **멈추지 않고 모은다** — 예전에는 첫 `assert!` 가
/// 죽으면 뒤 갈래가 안 재졌고, 그때 "뒤는 초록" 이 아니라 **미측정**이었다.
fn verdicts(c: &Census, f: &Floors) -> Vec<String> {
    let mut out = Vec::new();

    // ── 자기-공허 방지: 갈래마다 선다 ──────────────────────────────────────────
    if c.files_scanned < f.files {
        out.push(format!(
            "훑은 파일이 {} 개뿐이다(하한 {}) — 순회가 죽었으면 아래 초록은 거짓이다.\n  \
             [판별식] `git ls-files -- src crates tests | grep -c '\\.rs$'` 를 세어 이 수의 \
             **움직임**과 맞춰 봐라(두 수는 원래 같지 않다 — 저쪽은 추적되는 파일만 센다). 그 수도 함께 줄었으면 레포가 정말 줄어든 것이고, \
             그 수는 그대로인데 이 수만 줄었으면 `SCAN_ROOTS` 가 낡았거나 순회가 죽은 것이다.\n  \
             ★ 조용한 갈래는 **뿌리 이름이 바뀌는 것이 아니다** — 이 census 가 쓰는 순회기 \
             (`source_text::rust_sources`)는 `read_dir` 실패에 **panic** 한다. 조용한 것은 \
             `SCAN_ROOTS` 상수에서 뿌리를 **빼는** 것과 뿌리가 **비는** 것 둘이다.\n  \
             ★ 판별식을 밟지 않고 이 값만 내리지 마라. 내리면 다음번엔 더 쉽게 내려간다.\n  \
             [정말 줄었으면] 왜 줄었는지(어느 뿌리가 은퇴했는지)를 위 실측 주석에 적고 \
             값을 내려라 — 값만 바뀐 커밋은 그 자리를 다시 못 읽게 만든다.",
            c.files_scanned, f.files
        ));
    }
    if c.sites < f.sites {
        out.push(format!(
            "경로 짓는 temp_dir 자리를 {} 곳만 집었다(하한 {}) — 자리 판정이 죽었을 수 \
             있다.\n  \
             [판별식] 이 수는 `temp_dir()` 줄 중 **창 안에 `.join(` 이 있는 것**만 센다. \
             그러니 `temp_dir(` 의 **총 등장 수**를 따로 세어 함께 봐라. 총수도 줄었으면 \
             그런 코드가 정말 없어진 것이고, 총수는 그대로인데 이 수만 줄었으면 `.join(` \
             인식이나 창 판정이 깨진 것이다. 그때는 바로 아래 `UNPAIRED_RATCHET` 도 함께 \
             움직인다 — 두 수가 같은 방향으로 움직이는지가 갈림점이다.\n  \
             ★ 판별식을 밟지 않고 이 값만 내리지 마라.\n  \
             [정말 줄었으면] 없어진 자리를 실측 주석에 적고 값을 내려라.",
            c.sites, f.sites
        ));
    }
    if c.uniquified < f.uniquified {
        out.push(format!(
            "유니크화된 자리가 {} 곳뿐이다(하한 {}) — 유니크화 인식이 죽으면 \
             이 수가 떨어지고 그 자리들이 거짓 위반이 된다.\n  \
             [판별식] 인식이 죽었는지는 추측하지 말고 **부르면 된다**: \
             `cargo test -p tasty-doc-guards --lib every_recognized_uniquifier_is_actually_recognized`. \
             그 테스트는 `UNIQ_TOKENS` 의 성분마다 리터럴 조각 하나를 갖고 있어, 성분이 \
             빠지거나 철자가 틀리면 **그 성분의 이름을 대고** 죽는다. 그것이 초록인데 이 \
             수가 줄었으면 인식은 살아 있고 코드가 정말 바뀐 것이다.\n  \
             ★ 그 조각 테스트를 안 돌려 보고 이 값만 내리지 마라.\n  \
             [정말 줄었으면] 어느 자리가 유니크화를 잃었는지 적고 값을 내려라.",
            c.uniquified, f.uniquified
        ));
    }
    if c.unpaired != f.unpaired {
        out.push(format!(
            "창 안에 `.join(` 이 없어 검사에서 빠지는 `temp_dir()` 줄이 {} 개다 \
             (래칫 {}). 늘었으면 새로 안 보는 자리가 생긴 것이다 — \
             그 자리가 경로를 짓는다면 `temp_dir().join(..)` 을 붙여 쓰거나 창 안으로 \
             옮겨라. ★ 그냥 이 값만 올려서 통과시키지 마라 — 올리는 것은 래칫을 \
             푸는 것이고, 늘어난 그 줄이 경로를 짓는 줄이면 보호는 그 자리에서 \
             사라진다. 올리려면 늘어난 자리마다 `temp_dir()` 의 반환이 경로 조립에 \
             안 쓰인다는 것을 확인하고 그 자리를 이 상수의 주석에 적어라. \
             줄었으면 값을 같이 내려라 — \
             남는 여유가 곧 안 보는 구간이다.",
            c.unpaired, f.unpaired
        ));
    }
    // ── 등급이 판정에 들어간다 ──────────────────────────────────────────────
    // 시계가 프로세스-내 유일성을 홀로 지는 자리는 그 선택을 그 자리에 밝혀야 한다.
    //
    // ★ 이 자리의 첫 판은 "걸리는 자리 0 곳" 을 재고 "이미 지켜지던 규율을 값으로
    //   고정할 뿐" 이라고 적었다. **그 0 이 틀렸다** — 규칙이 "강한 성분이 하나라도
    //   있으면 강함" 이라 `process::id()` + 시계 조합을 강함으로 읽었고, 그것이 바로
    //   2026-09-08 macOS 사고를 낸 형태다. 축으로 다시 물었더니 여섯이 나왔고
    //   (전부 `process::id()` 를 이미 갖고 있었다) 전부 단조 카운터로 고쳤다.
    //   지금 0 은 **축을 보고 난 0** 이다.
    if !c.weak_only.is_empty() {
        out.push(format!(
            "시계가 프로세스-내 유일성을 **홀로** 지는 temp 경로가 {} 곳이다. 그 창의 \
             크기는 **플랫폼이 정한다**(시계 해상도) — 한 OS 에서 초록인 것이 다른 OS 에서 \
             깨지고, 그때 그 실패는 순회나 픽스처의 결함처럼 보인다.\n  \
             [처방] 시계 항을 **단조 카운터**로 바꿔라 — 함수 안에 \
             `static NEXT: AtomicUsize` 를 두고 `NEXT.fetch_add(1, Ordering::Relaxed)` \
             를 키에 넣는다. 해상도가 없는 값이라 플랫폼을 안 읽는다. `TempDir` 도 된다.\n  \
             ★ `std::process::id()` 를 **더하는 것은 처방이 아니다.** 그것은 프로세스 \
             **간** 축이고 여기서 무너지는 것은 프로세스 **내** 축이다 — 실측 2026-09-08: \
             걸린 자리 여섯이 전부 이미 `process::id()` 를 갖고 있었다.\n  \
             시계가 **의도된 선택**이면(예: 같은 프로세스가 회차마다 다른 경로를 원한다) \
             그 자리에 `이유:` 를 붙여라 — 그러면 이 판정은 그것을 의도로 읽고 넘긴다.\n  \
             ★ 이 판정을 지우거나 `CLOCK_TOKENS` 를 비워서 통과시키지 마라. 그러면 등급이 \
             다시 '적혀만 있고 판정에 안 들어가는' 자리로 돌아간다.\n{}",
            c.weak_only.len(),
            c.weak_only.join("\n")
        ));
    }

    // ── 재호출 축 ──────────────────────────────────────────────────────────
    // 정체성 축만 진 자리는 같은 프로세스의 두 호출을 못 가른다. 이쪽은 hard-fail 이
    // 아니라 **양방향 래칫**이다 — 일부러 프로세스당 하나인 자리를 소스에서 못 가르기
    // 때문이다(`RECALL_BLIND_RATCHET` 주석).
    if c.recall_blind.len() != f.recall_blind {
        out.push(format!(
            "프로세스-내 축이 비어 **같은 프로세스의 재호출을 못 가르는** temp 경로가 \
             {} 곳이다(래칫 {}). `std::process::id()` 는 프로세스 **간**만 가른다 — \
             같은 자리를 두 번 부르면 같은 경로가 나오고, 뒤 호출이 앞 호출의 트리를 \
             조용히 지운다.\n  \
             [줄이는 법] 두 갈래뿐이다. (1) 키에 **단조 카운터**를 넣어라 — 함수 안에 \
             `static NEXT: AtomicUsize` 를 두고 `NEXT.fetch_add(1, Ordering::Relaxed)`. \
             (2) 그 자리가 **일부러 프로세스당 하나**라면 그 자리에 사유를 적어라 \
             (`이유:` / `reason:` / `사유:`). 소스만 보고는 둘을 못 가르므로 그 판단은 \
             그 자리에서만 할 수 있다.\n  \
             ★ 이 값을 올려서 통과시키지 마라. 늘었다는 것은 재호출을 못 가르는 자리가 \
             새로 들어왔다는 뜻이고, 값을 올리면 그 자리는 영영 안 보인다.\n  \
             ★ 줄었으면 값을 같이 내려라 — 남는 여유가 곧 안 보는 구간이다.\n  \
             ★ `thread::current().id()` 는 이 물음의 답이 아니다. 그것은 같은 프로세스의 \
             **다른 시험**을 가르지, 같은 자리의 **재호출**을 안 가른다 — 아래 [ㄱ+ㄴ] \
             줄에 그 자리들이 따로 나온다. 그 자리에 스레드 id 를 더 넣어도 이 수는 \
             안 줄어든다.\n  \
             [ㄱ+ㄴ — 스레드 id 도 쓰는 자리] {}\n  \
             [자리]\n{}",
            c.recall_blind.len(),
            f.recall_blind,
            if c.recall_blind_with_thread.is_empty() {
                "없다".to_string()
            } else {
                format!(
                    "{} 곳\n{}",
                    c.recall_blind_with_thread.len(),
                    c.recall_blind_with_thread.join("\n")
                )
            },
            c.recall_blind.join("\n")
        ));
    }
    if c.reasoned < f.reasoned {
        out.push(format!(
            "사유로 통과한 자리가 {} 곳뿐이다(하한 {}) — 사유 인식이 죽었을 수 \
             있다.\n  \
             [판별식] 위와 같은 방식으로 **부른다**: \
             `cargo test -p tasty-doc-guards --lib temp_path::tests` 의 사유 마커 조각들 \
             (`a_reason_at_the_top_of_the_attached_block_counts` · \
             `a_korean_sayu_marker_also_passes`)이 초록인가. `REASON_TOKENS` 의 마커 하나가 \
             빠지면 그 조각이 이름을 대고 죽는다. 조각이 초록인데 이 수만 줄었으면 사유를 \
             단 자리가 정말 줄어든 것이다 — 그건 대개 **좋은 방향**이다(사유 대신 \
             유니크화로 옮겼다는 뜻이면 `uniquified` 가 같이 올라간다. 두 수를 함께 봐라).\n  \
             ★ 조각을 안 돌려 보고 이 값만 내리지 마라.\n  \
             [정말 줄었으면] 옮겨간 자리를 적고 값을 내려라.",
            c.reasoned, f.reasoned
        ));
    }

    // ── 실판정: 유니크화도 사유도 없는 고정 이름은 0 이어야 한다 ──────────────────
    if !c.silent.is_empty() {
        out.push(format!(
            "공유 temp 아래 고정 이름 임시 경로가 {} 곳 있다. 인스턴스/완주가 동시에 살면\n\
             같은 파일을 truncate 하거나 서로의 디렉터리를 지운다(ADR-0129 형태 B).\n\
             유니크화(pid·`tempfile`·`TempDir`)하거나, 공유가 의도라면 그 자리에 `이유:` 를 적어라:\n{}",
            c.silent.len(),
            c.silent
                .iter()
                .map(|s| format!("  {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    out
}

#[test]
fn every_temp_path_is_uniquified_or_reasoned() {
    let root = repo_root();
    let c = census(&root, SCAN_ROOTS);
    let v = verdicts(&c, &FLOORS);
    assert!(v.is_empty(), "{}", v.join("\n\n"));
}

/// 호출을 이 파일 안에서 하나도 못 찾은 판별자 사슬의 수 — **양방향 래칫**.
///
/// **모수**: 아래 시험이 도는 자리 전부 — `SCAN_ROOTS` 아래 `.rs` 에서 나온
/// `discriminator_chains` 의 사슬. 실측 2026-09-08, 잰 트리 `integration-94`
/// `0911d0113`: 사슬 **10** 중 이 값이 **1** (`DiskScrollback::new` — 호출자가
/// `crates/tasty-terminal/src/scrollback.rs` 에 있어 파일 경계 밖이다).
///
/// **사본**: 어느 줄이 호출인가는 `mask_non_code` 사본이 정하고(주석·문자열 속 호출
/// 모양을 안 세려고), 인자식 텍스트는 **원문**에서 읽는다(판별자가 대부분 문자열
/// 리터럴이라 마스킹 사본에서는 지워진다).
///
/// 통과가 아니라 **미측정**이다. 그 자리가 안전한지 이 판정은 답을 안 낸 것이고,
/// 늘어나면 새로 안 보는 자리가 생겼다는 뜻이다.
const CHAINS_NOT_SEEN_RATCHET: usize = 1;

/// **호출자가 서로 다른 판별자를 주는가.** 잔여 0 hard-fail.
///
/// `recall_blind` 의 (가2) 부류는 유일화 성분을 호출자에게서 받는다
/// (`fn tmp_pack(tag: &str)` → `tasty-fontpack-{tag}-{pid}`). 그 자리만 보면 안전한지
/// 알 수 없고, **두 호출이 같은 판별자를 넘기면 두 자리가 같은 경로를 낸다** — pid 도
/// 스레드 id 도 그 둘을 못 가른다(같은 프로세스다).
///
/// **모수**: 위 상수와 같다(사슬 10). 실측 겹침 **0**. 그 0 은 손으로 여덟 자리의
/// 호출자를 끝까지 따라가 확인한 값이었고(`01b3385a8`), **이 시험이 그것을 기계로
/// 옮긴 것**이다 — 그 전까지 이 성질은 아무 채널도 안 지켰다.
///
/// ★ 0 이라 이 시험은 오늘 아무것도 안 잡는다. 그래서 아래 픽스처 둘이 이 판정이
/// 살아 있음을 대신 보인다(R1080).
#[test]
fn no_two_callers_hand_the_same_discriminator_to_a_temp_path_helper() {
    let root = repo_root();
    let mut hits = Vec::new();
    let mut not_seen = Vec::new();
    for (rel, raw) in tasty_doc_guards::source_text::rust_sources(&root, SCAN_ROOTS) {
        let masked = mask_non_code(&raw);
        let code: Vec<&str> = masked.lines().collect();
        let rawl: Vec<&str> = raw.lines().collect();
        if code.len() != rawl.len() {
            continue;
        }
        for ch in discriminator_chains(&code, &rawl) {
            if ch.no_call_seen {
                not_seen.push(format!("{}: {}", rel.display(), ch.callee));
            }
            if !ch.duplicates.is_empty() {
                hits.push(format!(
                    "{}: {} 에 같은 판별자가 두 번 넘어간다 — {}",
                    rel.display(),
                    ch.callee,
                    ch.duplicates.join(" · ")
                ));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "임시 경로 헬퍼에 **같은 판별자**가 두 번 넘어간다. 두 호출이 같은 경로를 짓고, \
         뒤 호출이 앞 호출의 트리를 조용히 지운다 — `std::process::id()` 도 \
         `thread::current().id()` 도 그 둘을 못 가른다(같은 프로세스다).\n  \
         [줄이는 법] 두 갈래이고 서로 다르다. (1) 두 호출이 **다른 자리**면 판별자를 \
         서로 다르게 지어라 — 그것이 이 헬퍼가 판별자를 받는 이유다. (2) **같은 자리를 \
         정말 두 번** 불러야 하면 판별자로는 못 가른다. 키에 단조 카운터를 넣어라 \
         (`static NEXT: AtomicUsize` 와 `NEXT.fetch_add(1, Ordering::Relaxed)`).\n  \
         ★ 사유를 적어서 넘기지 마라 — 이건 \"일부러 프로세스당 하나\" 가 아니라 \
         한 프로세스 안의 두 경로가 실제로 겹치는 것이다.\n  \
         [자리]\n{}",
        hits.join("\n")
    );
    assert_eq!(
        not_seen.len(),
        CHAINS_NOT_SEEN_RATCHET,
        "판별자 사슬 중 **호출을 이 파일에서 못 찾은 것**이 {} 이다(래칫 {}). 통과가 \
         아니라 **미측정**이다 — 그 자리의 호출자가 파일 경계 밖에 있어 이 판정이 답을 \
         안 냈다.\n  \
         ★ 늘었으면 새로 안 보는 자리가 생긴 것이다. 그 헬퍼의 호출자를 한 파일로 \
         옮기거나, 그 자리에서 판별자를 안 받게(호출마다 유일한 성분을 그 자리에서 짓게) \
         고쳐라.\n  \
         ★ 줄었으면 값을 같이 내려라 — 남는 여유가 곧 안 보는 구간이다.\n  \
         [자리]\n{}",
        not_seen.len(),
        CHAINS_NOT_SEEN_RATCHET,
        not_seen.join("\n")
    );
}

/// **양성 대조 — 겹치는 판별자를 심으면 잡히는가.**
///
/// 위 시험이 오늘 0 이라 그 자체로는 살아 있는지 안 보인다. 이 조각은 레포의 실제
/// 모양(`src/boot/locale_font.rs` 의 `tmp_pack(tag)`)에서 **인자 하나만** 겹치게 바꾼
/// 것이다 — 가드의 상수에서 짓지 않았다(R1078).
#[test]
fn a_repeated_discriminator_is_caught() {
    let src = "fn tmp_pack(tag: &str) -> PathBuf {\n    \
               let dir = std::env::temp_dir().join(format!(\"tasty-fontpack-{tag}-{}\", std::process::id()));\n    \
               dir\n}\n\
               #[test]\nfn a() { let dir = tmp_pack(\"valid\"); }\n\
               #[test]\nfn b() { let dir = tmp_pack(\"valid\"); }\n";
    let masked = mask_non_code(src);
    let code: Vec<&str> = masked.lines().collect();
    let rawl: Vec<&str> = src.lines().collect();
    let chains = discriminator_chains(&code, &rawl);
    let dups: Vec<&String> = chains.iter().flat_map(|c| &c.duplicates).collect();
    assert_eq!(dups.len(), 1, "겹치는 판별자를 못 잡았다 — 사슬 {chains:?}");
    assert!(dups[0].contains("valid"), "{dups:?}");
}

/// **음성 대조 — 판별자가 다르면 안 잡힌다.** 위 조각에서 **한 글자만** 바꾼다.
///
/// 이 짝이 없으면 위 시험이 "무엇이든 잡는다" 여도 초록이다.
#[test]
fn distinct_discriminators_are_not_caught() {
    let src = "fn tmp_pack(tag: &str) -> PathBuf {\n    \
               let dir = std::env::temp_dir().join(format!(\"tasty-fontpack-{tag}-{}\", std::process::id()));\n    \
               dir\n}\n\
               #[test]\nfn a() { let dir = tmp_pack(\"valid\"); }\n\
               #[test]\nfn b() { let dir = tmp_pack(\"builtin\"); }\n";
    let masked = mask_non_code(src);
    let code: Vec<&str> = masked.lines().collect();
    let rawl: Vec<&str> = src.lines().collect();
    let chains = discriminator_chains(&code, &rawl);
    assert!(
        chains.iter().all(|c| c.duplicates.is_empty()),
        "다른 판별자를 겹친다고 했다 — {chains:?}"
    );
    assert!(!chains.is_empty(), "사슬 자체를 못 찾았다 — 좌변이 비었다");
}

/// 일곱 판정의 **문구** 양성 대조. 발화 여부가 아니라 *어느 문구가 나왔는가* 를 잰다.
///
/// ## 왜 가드의 상수를 안 쓰는가 (R1078)
///
/// 아래 `F` 는 `FLOORS` 가 아니라 **이 모듈이 정한 리터럴**이다. `MIN_FILES` 로 지으면
/// 그 상수를 바꾸는 변이에 픽스처의 입력도 같이 움직여 **항진명제**가 된다 — 상수를
/// 반으로 줄여도 조용하다. 픽스처는 *메커니즘*(어떤 조건이 어떤 문구를 내는가)만 재고,
/// 상수의 값이 옳은지는 위 실측 주석과 본 시험(`every_temp_path_is_uniquified_or_reasoned`)
/// 이 잰다. 두 축을 갈라 둔다.
///
/// ## 왜 문구 전체를 안 단정하는가
///
/// 문장을 다듬을 때마다 죽는 단정은 문서를 얼린다. 그래서 **처방을 가르는 낱말**만
/// 짚는다 — 일곱 문구의 처방이 서로 다르고(값을 내려라 / 창을 넓히지 마라 / pid 를
/// 섞어라 / 사유를 붙여라 …), 그 갈림이 낱말 하나에 실려 있다.
#[cfg(test)]
mod wording {
    use super::{Floors, verdicts};
    use tasty_doc_guards::temp_path::Census;

    const F: Floors = Floors {
        files: 10,
        sites: 5,
        uniquified: 4,
        unpaired: 2,
        reasoned: 3,
        recall_blind: 1,
    };

    /// 모든 값이 하한과 **같은** census — 경계가 통과 쪽인지도 함께 잰다.
    fn healthy() -> Census {
        Census {
            files_scanned: 10,
            sites: 5,
            uniquified: 4,
            reasoned: 3,
            unpaired: 2,
            silent: Vec::new(),
            weak_only: Vec::new(),
            recall_blind: vec![
                "a/b.rs:9: let p = temp_dir().join(format!(\"x-{}\", process::id()));".into(),
            ],
            recall_blind_with_thread: Vec::new(),
        }
    }

    /// 문구가 하나만 나오는지까지 본다 — 판정끼리 새면 여기서 잡힌다.
    fn only(c: &Census) -> String {
        let v = verdicts(c, &F);
        assert_eq!(v.len(), 1, "문구가 하나가 아니다: {v:#?}");
        v.into_iter()
            .next()
            .expect("바로 위에서 길이 1 을 단정했다")
    }

    #[test]
    fn a_census_at_the_floor_says_nothing() {
        assert!(verdicts(&healthy(), &F).is_empty());
    }

    #[test]
    fn a_short_file_count_hands_over_the_ls_files_discriminator() {
        let c = Census {
            files_scanned: 9,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("훑은 파일이"), "{m}");
        assert!(m.contains("git ls-files"), "{m}");
    }

    #[test]
    fn a_short_site_count_points_at_the_join_recognition_not_at_the_repo() {
        let c = Census {
            sites: 4,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("경로 짓는 temp_dir 자리를"), "{m}");
        assert!(m.contains("총 등장 수"), "{m}");
    }

    #[test]
    fn a_short_uniquified_count_names_the_token_fixture_to_run() {
        let c = Census {
            uniquified: 3,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("유니크화된 자리가"), "{m}");
        assert!(
            m.contains("every_recognized_uniquifier_is_actually_recognized"),
            "{m}"
        );
    }

    #[test]
    fn a_grown_unpaired_count_forbids_raising_the_ratchet() {
        let c = Census {
            unpaired: 3,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("검사에서 빠지는"), "{m}");
        assert!(m.contains("이 값만 올려서 통과시키지 마라"), "{m}");
    }

    /// **처방이 둘이라는 것이 이 문구의 값이다.** 카운터를 붙이거나, 일부러 프로세스당
    /// 하나라면 그 자리에 사유를 적는다. 소스만 보고 둘을 못 가르므로 문구가 한쪽만
    /// 말하면 나머지 부류는 기계적으로 잘못 고쳐진다.
    #[test]
    fn a_grown_recall_blind_count_offers_both_branches_and_forbids_raising() {
        let c = Census {
            recall_blind: vec!["a/b.rs:9: x".into(), "c/d.rs:2: y".into()],
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("재호출을 못 가르는"), "{m}");
        assert!(m.contains("단조 카운터"), "처방 (1) 이 없다: {m}");
        assert!(m.contains("사유를 적어라"), "처방 (2) 가 없다: {m}");
        assert!(
            m.contains("이 값을 올려서 통과시키지 마라"),
            "래칫을 푸는 것을 금지하는 문장이 없다: {m}"
        );
        assert!(m.contains("c/d.rs:2"), "어느 자리인지 안 적었다: {m}");
    }

    /// **문구가 스레드 id 를 답으로 오해하게 두면 안 된다.**
    ///
    /// 2026-09-08 통합에서 이 래칫이 짖은 두 자리가 pid + `thread::current().id()` 였다.
    /// 그 형태를 보고 "정체성 성분을 하나 더 넣었으니 됐다" 고 읽으면, 그 자리는 다른
    /// 시험과만 갈린 채 이 칸에 그대로 남고 수도 안 줄어든다. 문구가 그 갈래를 이름으로
    /// 지목하고, ㄱ+ㄴ 자리를 따로 찍어 다음 회차의 분할을 시작한다.
    #[test]
    fn the_message_names_the_thread_split_and_denies_it_as_the_cure() {
        let c = Census {
            recall_blind: vec!["a/b.rs:9: x".into(), "c/d.rs:2: y".into()],
            recall_blind_with_thread: vec!["c/d.rs:2: y".into()],
            ..healthy()
        };
        let m = only(&c);
        assert!(
            m.contains("thread::current().id()") && m.contains("이 수는 안 줄어든다"),
            "스레드 id 가 이 물음의 답이 아니라는 말이 없다: {m}"
        );
        assert!(m.contains("[ㄱ+ㄴ"), "분할 줄이 없다: {m}");
        assert!(
            m.contains("1 곳"),
            "ㄱ+ㄴ 자리 수가 안 찍혔다 — 분할의 첫 줄이 안 나온다: {m}"
        );
    }

    /// ㄱ+ㄴ 이 없으면 그 줄은 "없다" 로 나간다 — 빈 목록을 0 곳으로 찍으면 읽는 사람이
    /// 세다 만 것인지 정말 없는 것인지 못 가른다.
    #[test]
    fn the_thread_split_line_says_none_when_there_is_none() {
        let c = Census {
            recall_blind: vec!["a/b.rs:9: x".into(), "c/d.rs:2: y".into()],
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("[ㄱ+ㄴ — 스레드 id 도 쓰는 자리] 없다"), "{m}");
    }

    /// 줄어도 발화한다 — 남는 여유는 곧 안 보는 구간이다.
    #[test]
    fn a_shrunk_recall_blind_count_also_fires() {
        let c = Census {
            recall_blind: Vec::new(),
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("값을 같이 내려라"), "{m}");
    }

    /// 래칫은 양방향이다 — 줄어도 발화하고, 그때 처방은 "값을 같이 내려라" 다.
    #[test]
    fn a_shrunk_unpaired_count_also_fires() {
        let c = Census {
            unpaired: 1,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("검사에서 빠지는"), "{m}");
        assert!(m.contains("남는 여유가 곧 안 보는 구간"), "{m}");
    }

    /// 처방은 카운터다 — pid 를 더하라는 말이 **아니라는 것**까지 문구가 말해야 한다.
    /// 실측 2026-09-08: 걸린 여섯 자리가 전부 이미 `process::id()` 를 갖고 있었다.
    #[test]
    fn a_clock_only_site_is_told_to_use_a_counter_not_to_add_a_pid() {
        let c = Census {
            weak_only: vec!["a/b.rs:7: let p = temp_dir().join(format!(\"x-{nanos}\"));".into()],
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("시계가 프로세스-내"), "{m}");
        assert!(m.contains("fetch_add"), "카운터 처방이 없다: {m}");
        assert!(
            m.contains("더하는 것은 처방이 아니다"),
            "pid 를 더하는 것이 처방이 아니라는 말이 없다: {m}"
        );
        assert!(m.contains("a/b.rs:7"), "좌표가 문구에 안 실렸다: {m}");
    }

    #[test]
    fn a_short_reasoned_count_names_the_marker_fixtures_to_run() {
        let c = Census {
            reasoned: 2,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("사유로 통과한 자리가"), "{m}");
        assert!(m.contains("a_korean_sayu_marker_also_passes"), "{m}");
    }

    #[test]
    fn a_silent_fixed_name_gets_the_two_way_prescription_and_its_coordinate() {
        let c = Census {
            silent: vec!["c/d.rs:3: let p = temp_dir().join(\"tasty-cache\");".into()],
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("고정 이름 임시 경로가"), "{m}");
        assert!(m.contains("ADR-0129"), "{m}");
        assert!(m.contains("c/d.rs:3"), "좌표가 문구에 안 실렸다: {m}");
    }
}

/// 그 시계 항이 축을 안 진다는 것을 **경계로 박아 둔다** — 지울지 말지는 소유자의 몫.
///
/// `crates/tasty-host-plugin/src/handle_channel.rs` 의 socket 경로는 pid·시계·카운터
/// 셋을 함께 든다. 이 회차에 그 시계 항을 재 봤다:
///
/// - 지는 축이 있다 — pid 는 재사용되고, sticky `/tmp` 에서는 남의 잔재를 unlink 도
///   못 한다. 그때 경로를 가르는 것이 이 항이다.
/// - **그런데 그 축을 재는 시험이 하나도 없다.** 이 항을 상수 `0u64` 로 바꾸고
///   `cargo test -p tasty-host-plugin` 을 돌리면 **rc 0, 202 passed** 다(실측).
///   한 프로세스 안에서는 이 축이 안 보이기 때문이다.
///
/// 즉 그 항은 지금 **판정도 시험도 안 지킨다.** 지우면 아무도 안 죽는다.
/// 지울 것인지, 두되 이유를 자리에 적을 것인지는 **그 파일을 소유한 쪽**의 판단이라
/// 여기서 정하지 않는다. 대신 지금 상태를 값으로 고정해, 누가 손대면 그 결정을
/// 하도록 강제한다 — 이 시험이 죽는 것이 그 신호다.
///
/// 창 안에서 세 성분을 찾을 때 **코드만 본다**(`mask_non_code`). 지금 창 안 주석에
/// 그 세 낱말은 하나도 없다(실측 0 · 0 · 0) — 그러니 이 마스킹은 오늘 아무것도
/// 안 바꾼다. 그래도 코드만 보는 이유는, 그 자리 주석이 **바로 이 축을 서술하고
/// 있어서** 낱말이 언제든 들어오기 때문이다. 들어온 뒤에 원문에서 세면 코드에서
/// 지워도 통과한다 — 그때는 이 시험이 자기가 지키려던 것을 놓친다.
#[test]
fn the_handle_socket_key_still_carries_all_three_components() {
    // 이 시험의 창·앵커는 전부 자기 리터럴이다 — 가드 상수를 빌리면 그 상수에 대해
    // 항진명제가 된다(R1078).
    const OWNER: &str = "crates/tasty-host-plugin/src/handle_channel.rs";
    let path = repo_root().join(OWNER);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("읽을 수 없다: {} — {e}", path.display()));
    let masked = mask_non_code(&text);
    let lines: Vec<&str> = masked.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.contains("let socket_path"))
        .unwrap_or_else(|| {
            panic!(
                "{OWNER} 에서 `let socket_path` 을 못 찾았다 — 그 함수가 옮겨졌거나 \
                 이름이 바뀌었다. 이 시험은 그 자리의 **유일성 키 성분**을 지킨다. \
                 좌표가 낡았으면 앵커를 고쳐라, 시험을 지우지 마라"
            )
        });
    let lo = at.saturating_sub(16);
    let hi = (at + 4).min(lines.len());
    let win = &lines[lo..hi];
    let has = |t: &str| win.iter().any(|l| l.contains(t));

    assert!(
        has("process::id"),
        "{OWNER}: socket 키에서 pid 성분이 사라졌다 — 프로세스 간 축이 무너진다"
    );
    assert!(
        has("fetch_add"),
        "{OWNER}: socket 키에서 단조 카운터가 사라졌다 — 같은 프로세스의 동시 bind 가 \
         겹친다. 이 축은 시계로 대신할 수 없다(해상도가 플랫폼의 성질이다)"
    );
    assert!(
        has("SystemTime"),
        "{OWNER}: socket 키에서 시계 항이 사라졌다.\n  \
         [판단이 필요하다 — 이 시험이 대신 정하지 않는다] 그 항이 지던 축은 \
         **재사용된 pid 로 도는 다음 프로세스를 옛 잔재와 가르는 것**이고, sticky \
         `/tmp` 에서는 바로 아래 `remove_file` 이 남의 파일을 못 지운다. 그래서 쓸모가 \
         있다.\n  \
         그런데 **그 축을 재는 시험이 없다** — 이 항을 상수로 바꿔도 그 크레이트의 \
         시험은 전부 초록이다(실측 202 passed). 즉 지워도 아무도 안 죽는다.\n  \
         일부러 지웠으면 이 단정을 함께 지우고 **왜 그 축이 필요 없는지**를 그 자리에 \
         적어라. 실수로 지웠으면 되돌려라. 둘 중 하나를 고르게 하려고 이 시험이 있다."
    );
}
