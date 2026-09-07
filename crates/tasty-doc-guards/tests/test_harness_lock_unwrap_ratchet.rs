//! **통합 테스트 하네스가 공유 락을 생 `.unwrap()` 으로 잡는 자리**를 양방향으로 고정한다.
//!
//! ## 왜 (실측된 기전)
//!
//! 락을 `panic!` 인자 안에서 잡으면 임시 가드가 그 statement 끝까지 — 되감기가 끝날 때까지 —
//! 살아 있어 `Mutex` 가 오염된다. 그러면 같은 바이너리의 다른 스레드/테스트가 다음 `lock()`
//! 에서 죽는다. 즉 **실패 하나가 나머지를 만들어 낸다.** 실측(2026-09-06~07, `rustc` 단독 대조):
//! 가드가 그 statement 안에 살아 있는 팔은 `is_poisoned=true`, 값을 지역 변수로 뺀 팔과
//! 패닉이 안 나는 팔은 둘 다 `false`.
//!
//! 그 결과는 두 방향으로 나뉜다 — **관측된 실패 수가 진짜 실패 수가 아니게 되거나**(가짜 F 가
//! 부풀어 오른다), **진단이 조용히 사라진다**(오염되는 것이 stderr 시계라 drain 스레드가 죽고
//! 이후 실패의 tail 이 없어진다. F 는 그대로다 — 이쪽이 알아채기 어렵다).
//!
//! ## 형제와 무엇이 다른가 (같은 물음에 답을 둘로 만들지 않는다)
//!
//! [`tasty_doc_guards::poison_recovery`] 는 **복구 쪽**을 본다 — poison 을 `into_inner()` 로
//! 복구하면서 보고가 없는 자리이고, 뿌리는 `src`·`crates` 다. 이 가드는 **획득 쪽**을 보고
//! 뿌리는 루트 `tests/` 다. 물음도 뿌리도 겹치지 않는다. 합치지 마라.
//!
//! ## 술어를 좁게 잡으면 0 이 나온다
//!
//! `lock()` 만 세면 `RwLock` 의 `read()`·`write()` 를 통째로 놓친다. 그러면 수가 작아지고
//! "별 것 없다" 로 끝난다 — 이 축을 처음 세울 때 실제로 그랬다. 그래서 획득 토큰을 셋 다 센다.
//! 주석·문자열 안의 언급은 [`tasty_doc_guards::source_text::mask_non_code`] 가 덮는다(레포의
//! 정본 판정기다). 실측: 마스킹이 없으면 `tests/spawn_diag/mod.rs` 의 **문서 주석 안** 예시가
//! 자리로 세어진다.
//!
//! ## 이 가드가 보지 않는 것 — 그 아래에서 무엇이 죽는지 같이 적는다
//!
//! 뿌리는 루트 `tests/` 뿐이다. `crates/*/tests/` 의 하네스는 **안 본다.**
//! 그 아래에서 죽는 것: 새 공유 하네스가 크레이트 안에 생기면 이 가드는 조용하다.
//! 지금 그것을 안 넣는 이유는 뿌리를 넓히면 크레이트 유닛의 락 사용이 대량으로 딸려 와
//! 술어가 다른 물음(프로덕션 락 규율)에 답하게 되기 때문이다 — 그건 형제 가드의 물음이다.
//! **넓히려면 술어를 먼저 갈라라.** 뿌리만 넓히면 이 가드는 잡음이 된다.
//!
//! ## 면제가 자라는 것 자체가 실패다
//!
//! 이 설계의 진짜 위험은 **시끄러움 → 면제 확대 → 조용함**이다. 시끄러운 쪽은 스스로 보이지만
//! 그 소음을 끄는 자연스러운 방법이 면제를 넓히는 것이고, 넓어진 면제는 다시 조용하다. 즉
//! 종착점이 조용함이라 **큰 소리로 죽는 방법이 없다.** 그래서 면제 수에 예산을 두고, 예산을
//! 넘기면 그 사실을 실패 메시지가 말한다 — *면제가 늘었다 = 이 가드가 보는 구간이 줄었다.*
//!
//! 수는 초록일 때도 남긴다:
//!
//! ```text
//! cargo test -p tasty-doc-guards --test test_harness_lock_unwrap_ratchet -- --nocapture
//! ```

use tasty_doc_guards::repo_root;
use tasty_doc_guards::source_text::{mask_non_code, rust_sources};

/// 훑는 뿌리. 위 "보지 않는 것" 절이 이 하나를 고른 이유를 적는다.
const SCAN_ROOTS: &[&str] = &["tests"];

/// 공유 락을 잡는 토큰 셋. `lock()` 만 세면 `RwLock` 을 통째로 놓친다.
const ACQUIRE: &[&str] = &[".lock()", ".read()", ".write()"];

/// 자리 단위 면제 — (레포 상대 경로, 그 줄의 특징 조각, 사유).
///
/// 파일 통째 면제는 쓰지 않는다. 그 파일이 **다른** 형태의 위반을 새로 들이면 그건 잡혀야 한다.
/// 각 항목은 **실재해야 한다** — 아무 줄에도 안 맞는 면제는 죽은 면제이고, 죽은 면제는 그 자리가
/// 사라진 뒤에도 명부를 부풀려 다음 사람이 예산을 올리게 만든다.
///
/// ★ 여기 오르는 자리는 전부 **스캔 뿌리 안**이다. 그래서 "0 개 맞음" 은 "뿌리 밖이라 안 보였다"
/// 가 아니라 **"그 자리가 없어졌다"** 로 읽는 것이 맞다. 뿌리 밖 항목을 이 명부에 올리지 마라 —
/// 두 종류가 섞이면 0 의 처방이 반대가 된다(사라진 자리는 지워야 하고, 뿌리 밖 자리는 뿌리를 넓혀야 한다).
///
/// 사유는 전부 **같은 축 하나**다: *락을 쥔 범위 안에서 패닉하지 않는다.* 그게 이 결함을
/// 가르는 기준이고(이름도, 락 종류도, 파일도 아니다), 여기 오른 자리들은 락 안에서
/// iterate·clone·대입만 한다.
/// ★ **"안전" 이 아니라 "가해자가 아니다" 로 읽어라.** 이 자리들은 생 `.unwrap()` 이라
/// **남이 오염시키면 그대로 죽는다**(실측: 오염된 뒤 `into_inner` 로는 읽히고 생 `unwrap` 은
/// 죽는다). 지금 사는 이유는 이 레포에 그들을 오염시킬 자리가 남아 있지 않아서다.
const EXEMPT: &[(&str, &str, &str)] = &[
    (
        "tests/shared_instance_harness.rs",
        "let mut observed = OBSERVED.lock().unwrap();",
        "관찰 기록에 밀어 넣기만 한다. 단정은 락 밖으로 나갔다 — 그 이동이 이 축의 첫 수선이었다.",
    ),
    (
        "tests/common/mod.rs",
        "let ring = ring.lock().unwrap();",
        "stderr tail 읽기 — 쥔 채 iterate·clone 만 한다. 단정도 `panic!` 도 없다.",
    ),
    (
        "tests/common/mod.rs",
        "*last_at.lock().unwrap() = Some(Instant::now());",
        "drain 스레드의 시계 대입. 쥔 범위가 대입 하나다.",
    ),
    (
        "tests/common/mod.rs",
        "let mut ring = ring.lock().unwrap();",
        "drain 스레드의 ring 적재. 쥔 채 push 만 한다.",
    ),
    (
        "tests/gui_common/mod.rs",
        "let ring = ring.lock().unwrap();",
        "stderr tail 읽기 — 위 형제와 같은 모양.",
    ),
    (
        "tests/gui_common/mod.rs",
        "*last_at.lock().unwrap() = Some(Instant::now());",
        "drain 스레드의 시계 대입.",
    ),
    (
        "tests/gui_common/mod.rs",
        "let mut ring = ring.lock().unwrap();",
        "drain 스레드의 ring 적재.",
    ),
    (
        "tests/webhook_common/mod.rs",
        "*last_at.lock().unwrap() = Some(Instant::now());",
        "drain 스레드의 시계 대입.",
    ),
    (
        "tests/webhook_common/mod.rs",
        "let mut ring = ring.lock().unwrap();",
        "drain 스레드의 ring 적재.",
    ),
    (
        "tests/webhook_common/mod.rs",
        "let ring = self.stderr_ring.lock().unwrap();",
        "stderr tail 읽기(인스턴스 메서드 쪽).",
    ),
    (
        "tests/webhook_common/mod.rs",
        "let ring = ring.lock().unwrap();",
        "stderr tail 읽기(자유 함수 쪽).",
    ),
];

/// 면제 예산. **늘리는 것 자체가 이 가드가 보는 구간을 줄이는 일이다.**
///
/// 지금 값은 위 명부의 크기와 같다 — 여유를 두지 않는다. 여유를 두면 새 면제가 **아무 흔적
/// 없이** 들어오고, 그게 바로 이 가드의 퇴화 경로다. 면제를 하나 더 넣으려면 이 수도 같은
/// 커밋에서 올라가야 하고, 그러면 그 결정이 diff 에 남는다.
const EXEMPT_BUDGET: usize = 11;

/// 획득 자리 수의 하한. **왜 양방향인가** — 상한만 두면 술어가 깨졌을 때(토큰 오타, 마스킹
/// 실패, 뿌리 소실) 수가 0 으로 떨어지고 0 은 언제나 상한 아래다. 형제
/// `check-allow-reason`·`check-shared-walk-ratchet` 이 같은 이유로 양방향이고, 그쪽 문장은
/// "남는 여유가 곧 안 보는 구간이다" 다. **여기서는 이유가 하나 더 있다**: 이 가드의 분자
/// (생 `.unwrap()` 자리)는 지금 0 이라 상한만으로는 술어의 생사가 전혀 안 보인다. 분모가
/// 유일한 생존 신호다.
///
/// 실측(2026-09-07): 루트 `tests/` 의 `.rs` 59 파일에서 획득 **26** 자리. 값은 그 아래로
/// 넉넉히 잡았다 — 하네스가 정리되며 몇 자리가 줄어드는 것은 정상이고, 술어가 죽으면 이 수는
/// 몇이 아니라 **0 근처로** 떨어지기 때문이다. 이 하한은 래칫이 아니라 **술어 생존 검사**다.
const MIN_ACQUISITIONS: usize = 18;

/// 한 파일의 소스에서 (획득 자리, 생 `.unwrap()` 자리) 를 센다. 좌표는 1 기반 줄이다.
fn classify(masked: &str) -> (Vec<usize>, Vec<usize>) {
    let mut acquired = Vec::new();
    let mut bare = Vec::new();
    for (i, line) in masked.lines().enumerate() {
        for tok in ACQUIRE {
            if let Some(pos) = line.find(tok) {
                acquired.push(i + 1);
                let rest = &line[pos + tok.len()..];
                if rest.trim_start().starts_with(".unwrap()") {
                    bare.push(i + 1);
                }
                break;
            }
        }
    }
    (acquired, bare)
}

#[test]
fn a_bare_unwrap_on_a_shared_lock_is_caught() {
    // 양성 대조 — 픽스처는 **합성 이름**만 쓴다. 실재 경로·실재 타깃 이름을 쓰면 그 파일의
    // 소유자가 옮겼을 때 없는 것을 예시로 든 채 초록이 된다(문자열 판정이라 안 운다).
    let src = "fn probe() {\n    let g = SHARED.lock().unwrap();\n}\n";
    let (acq, bare) = classify(&mask_non_code(src));
    assert_eq!(
        acq,
        vec![2],
        "획득 자리를 못 집으면 이 가드는 아무것도 안 본다"
    );
    assert_eq!(
        bare,
        vec![2],
        "생 `.unwrap()` 을 못 집으면 실판정이 공허해진다"
    );
}

#[test]
fn the_fixed_shape_is_not_flagged() {
    // 음성 대조 — 수선된 모양(poison 을 이어받는다)은 걸리면 안 된다.
    let src = "fn probe() {\n    let g = SHARED.lock().unwrap_or_else(|e| e.into_inner());\n}\n";
    let (acq, bare) = classify(&mask_non_code(src));
    assert_eq!(
        acq,
        vec![2],
        "이 모양도 **획득**이긴 하다 — 분모에는 들어와야 한다"
    );
    assert!(
        bare.is_empty(),
        "수선된 모양을 위반으로 세면 고친 사람이 벌을 받는다"
    );
}

#[test]
fn a_mention_in_a_comment_is_not_a_site() {
    // 마스킹이 죽으면 이 대조가 먼저 넘어진다.
    let src = "fn probe() {\n    // let g = SHARED.lock().unwrap();\n}\n";
    let (acq, bare) = classify(&mask_non_code(src));
    assert!(
        acq.is_empty() && bare.is_empty(),
        "주석 안의 언급은 자리가 아니다"
    );
}

#[test]
fn test_harness_locks_do_not_unwrap_bare() {
    let root = repo_root();
    let sources = rust_sources(&root, SCAN_ROOTS);

    let mut acquisitions = 0usize;
    let mut sites: Vec<String> = Vec::new();
    let mut exempt_hits = vec![0usize; EXEMPT.len()];

    for (rel, raw) in &sources {
        let rel_s = rel.display().to_string();
        let masked = mask_non_code(raw);
        let lines: Vec<&str> = masked.lines().collect();
        let (acq, bare) = classify(&masked);
        acquisitions += acq.len();
        for ln in bare {
            let text = lines.get(ln - 1).map(|s| s.trim()).unwrap_or("");
            let mut exempted = false;
            for (i, (path, needle, _why)) in EXEMPT.iter().enumerate() {
                if &rel_s == path && text.contains(needle) {
                    exempt_hits[i] += 1;
                    exempted = true;
                }
            }
            if !exempted {
                sites.push(format!("{rel_s}:{ln}: {text}"));
            }
        }
    }

    // 단정보다 **앞**에 둔다 — 빨간 경로에서도 모수가 남아야 한다.
    eprintln!(
        "[harness-lock] 파일 {} · 획득 {} · 면제 {}/{} (예산 {}) · 생 unwrap {}",
        sources.len(),
        acquisitions,
        exempt_hits.iter().filter(|n| **n > 0).count(),
        EXEMPT.len(),
        EXEMPT_BUDGET,
        sites.len()
    );

    // ── 자기-공허 방지 ────────────────────────────────────────────────────────
    assert!(
        !sources.is_empty(),
        "뿌리 `{:?}` 아래에서 `.rs` 를 하나도 못 찾았다 — 아래 초록은 '위반이 없다' 가 아니라 \
         '아무것도 안 봤다' 다.",
        SCAN_ROOTS
    );
    assert!(
        acquisitions >= MIN_ACQUISITIONS,
        "공유 락을 잡는 자리를 {acquisitions} 곳만 집었다(하한 {MIN_ACQUISITIONS}).\n  \
         [판별식] 이 수는 **술어의 생존 신호**다. 위반 수가 0 이라 상한만으로는 술어가 죽은 \
         것과 깨끗한 것이 구분되지 않는다. 이 수가 떨어졌으면 셋 중 하나다 — 획득 토큰이 \
         바뀌었거나(`.lock()`/`.read()`/`.write()`), 마스킹이 죽었거나, 뿌리가 사라졌다. \
         유닛 셋(`a_bare_unwrap_on_a_shared_lock_is_caught` · `the_fixed_shape_is_not_flagged` \
         · `a_mention_in_a_comment_is_not_a_site`)이 앞의 둘을 갈라 준다.\n  \
         [정말 줄었으면] 무엇이 없어졌는지 상수 옆에 적고 값을 내려라."
    );

    // ── 면제 명부의 건강 ──────────────────────────────────────────────────────
    let dead: Vec<String> = EXEMPT
        .iter()
        .zip(&exempt_hits)
        .filter(|(_, n)| **n == 0)
        .map(|((path, needle, _), _)| format!("{path} :: {needle}"))
        .collect();
    assert!(
        dead.is_empty(),
        "아무 줄에도 안 맞는 면제가 있다: {dead:?}\n  \
         이 명부의 항목은 전부 **스캔 뿌리 안**이므로 0 개 맞음은 '뿌리 밖이라 안 보였다' 가 \
         아니라 **'그 자리가 없어졌다'** 다. 처방은 뿌리를 넓히는 것이 아니라 **그 항목을 \
         지우는 것**이고, 지울 때 예산도 같이 내려라."
    );
    assert!(
        EXEMPT.len() <= EXEMPT_BUDGET,
        "면제가 {} 개로 예산 {EXEMPT_BUDGET} 을 넘었다.\n  \
         **면제가 늘었다 = 이 가드가 보는 구간이 줄었다.** 이 게이트의 퇴화 경로가 정확히 \
         그것이다 — 시끄러움 → 면제 확대 → 조용함. 종착점이 조용함이라 이 가드는 큰 소리로 \
         죽는 방법이 없고, 그래서 그 한 걸음을 여기서 멈춘다.\n  \
         정말 면제해야 하면 예산을 **같은 커밋에서** 올리고 왜 그 자리가 안전한지 항목의 \
         사유 칸에 적어라. 사유 없는 면제는 다음 사람에게 '원래 그런 것' 으로 읽힌다.",
        EXEMPT.len()
    );

    // ── 실판정 ────────────────────────────────────────────────────────────────
    assert!(
        sites.is_empty(),
        "공유 락을 생 `.unwrap()` 으로 잡는 자리가 {} 곳 있다.\n\
         그 자리가 락을 쥔 채 패닉하면 Mutex 가 오염되고, 같은 바이너리의 다른 스레드·테스트가 \
         다음 `lock()` 에서 죽는다 — 실패 하나가 나머지를 만들어 낸다.\n\
         고치는 법: 값을 먼저 지역 변수로 빼서 가드를 패닉 **전에** 떨어뜨리고, 잡을 때는 \
         `unwrap_or_else(|e| e.into_inner())` 로 poison 을 이어받아라. 그 자리 주석에 왜 \
         이어받아도 값이 옳은지(보호 대상이 무엇인지) 적어라.\n\
         ★ 조건 쪽에 락을 새로 들이지 마라 — 한 statement 에서 두 번 잡으면 `Mutex` 는 \
         재진입이 아니라 거기서 멈춘다(실측: 오염 대신 교착이 났다).\n{}",
        sites.len(),
        sites
            .iter()
            .map(|s| format!("  {s}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}
