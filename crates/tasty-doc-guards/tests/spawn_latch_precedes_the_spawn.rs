//! 프로세스를 띄우는 `get_or_init` 클로저는 **띄우기 전에** 재시도 래치를 건다.
//!
//! `OnceLock::get_or_init` 은 초기화 클로저가 panic 하면 미초기화로 남는다 — 다음
//! 테스트가 그 클로저를 그대로 다시 돈다. 부팅이 막힌 조건(디스플레이 부재 · GPU 초기화
//! 실패 · 포트 파일 미작성)에서는 **테스트 수만큼 프로세스가 더 뜨고** 각각이 상한까지
//! 기다린다. 처방은 클로저 첫머리에 래치를 걸어 두 번째를 **띄우기 전에** 막는 것이다.
//!
//! ## 왜 이 판정이 여기 있나 — 채널이 그 자리에 없다
//!
//! 래치 **타입**이 계약대로 도는지는 그 타입 옆의 시험이 본다. 없던 것은 **하네스가
//! 래치를 맞는 자리에 뒀는가** 다. 그 줄이 클로저 밖으로 나가거나 `spawn()` 뒤로 밀려도
//! 컴파일되고 단위 시험도 초록이다 — 클로저 밖에서는 `OnceLock` 이 그 자리를 한 번만
//! 부르므로 래치가 영영 안 걸리고, 그 사실은 **부팅이 막힌 환경에서 벽시계로만** 드러난다.
//! 그 조건을 만드는 자동 잡이 없다: 한쪽 하네스가 사는 `tests/gui_tests.rs` 는 어떤 자동
//! 채널도 안 돌고, 나머지는 그 환경에서 부팅에 성공해 버린다.
//!
//! 그래서 판정을 **소스의 구조**로 옮겼다. 이 축의 다른 판정기들은 비싼 것이 몇 번
//! 일어났는지를 실행 중에 세는데, 여기서는 그럴 수 없다 — 세려면 그 환경을 만들어야
//! 하고 그것을 만드는 잡이 없다. 대신 **회귀가 소스의 위치로 드러난다**(래치가 spawn
//! 앞이냐 뒤냐). 위치로 드러나는 회귀는 위치로 판정하는 것이 맞고, 이 타깃은
//! `check-headless` 가 main push 마다 돌린다.
//!
//! ## 술어 — 순서지 존재가 아니다
//!
//! "래치가 있는가" 로 물으면 `spawn()` 뒤에 있어도 통과한다. 그 배치는 아무것도 안
//! 막는다 — 두 번째 프로세스는 이미 떴다. 그래서 묻는 것은 **순서**다.

use tasty_doc_guards::source_text::mask_non_code;

/// 프로세스를 띄우는 표지. 하네스 둘 다 `<타입>::spawn()` 형태를 쓴다.
const SPAWN_MARK: &str = "::spawn()";

/// 재시도 래치의 표지 — 두 형태를 다 받는다.
///
/// 하나는 타입으로 추출돼 있고(`SpawnOnceLatch::entering`) 다른 하나는 같은 뜻의 인라인
/// 사본(`AtomicBool::swap(true, …)`)이다. **둘을 합치라고 요구하지 않는다** — 이 판정이
/// 묻는 것은 추상화의 모양이 아니라 순서다. 형태가 하나 더 생기면 여기 한 줄을 더한다.
const LATCH_MARKS: &[&str] = &[".entering(", ".swap(true,"];

/// 좌변의 하한 — 판정 대상이 0 이면 "위반 없음" 은 언제나 참이다.
///
/// 값의 근거: 2026-09-08 실측 2(`tests/gui_common/mod.rs` · `tests/common/mod.rs`).
/// 하한을 2 로 두는 것은 "지금 둘" 이라서가 아니라 **둘 중 하나가 사라지는 것이 이
/// 판정이 놓치면 안 되는 변화**이기 때문이다 — 하네스가 통째로 없어지는 것과 그 파일이
/// 스캔에서 빠지는 것을 이 하한이 갈라 준다.
const MIN_INIT_CLOSURES: usize = 2;

/// 한 클로저 본문에 대한 판정.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// 래치가 `spawn()` 앞에 있다.
    LatchFirst,
    /// 래치가 아예 없다.
    NoLatch,
    /// 래치가 있는데 `spawn()` 뒤다 — 두 번째 프로세스는 이미 떴다.
    LatchAfterSpawn,
}

/// `get_or_init(` 클로저 본문들을 뽑아 각각을 판정한다.
///
/// 입력은 **코드가 아닌 부분을 덮은 사본**이어야 한다. 안 그러면 주석에 적힌 `::spawn()`
/// 이 실물로 세어진다 — 이 파일의 머리 주석이 바로 그런 자리다.
fn verdicts(masked: &str) -> Vec<Verdict> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(pos) = masked[from..].find("get_or_init(") {
        let start = from + pos;
        from = start + "get_or_init(".len();
        let Some(body) = closure_body(masked, from) else {
            continue;
        };
        if !body.contains(SPAWN_MARK) {
            continue;
        }
        let spawn_at = body.find(SPAWN_MARK).expect("바로 위에서 확인했다");
        let latch_at = LATCH_MARKS.iter().filter_map(|m| body.find(m)).min();
        out.push(match latch_at {
            None => Verdict::NoLatch,
            Some(at) if at < spawn_at => Verdict::LatchFirst,
            Some(_) => Verdict::LatchAfterSpawn,
        });
    }
    out
}

/// `from` 이후 첫 `{` 부터 균형 잡힌 `}` 까지. 문자열·주석은 이미 덮여 있다고 본다.
fn closure_body(text: &str, from: usize) -> Option<&str> {
    let open = text[from..].find('{')? + from;
    let mut depth = 0usize;
    for (i, c) in text[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[open..open + i + 1]);
                }
            }
            _ => {}
        }
    }
    None
}

#[test]
fn every_spawning_init_closure_latches_before_it_spawns() {
    let root = tasty_doc_guards::repo_root();
    let mut seen = 0usize;
    let mut problems = Vec::new();
    for (rel, text) in
        tasty_doc_guards::source_text::rust_sources(&root, &["tests", "src", "crates"])
    {
        for (i, v) in verdicts(&mask_non_code(&text)).into_iter().enumerate() {
            seen += 1;
            let why = match v {
                Verdict::LatchFirst => continue,
                Verdict::NoLatch => "래치가 없다",
                Verdict::LatchAfterSpawn => "래치가 `spawn()` 뒤에 있다",
            };
            problems.push(format!(
                "  {} (그 파일에서 프로세스를 띄우는 {} 번째 클로저): {why}",
                rel.display(),
                i + 1
            ));
        }
    }

    assert!(
        seen >= MIN_INIT_CLOSURES,
        "프로세스를 띄우는 `get_or_init` 클로저를 {seen} 개밖에 못 찾았다(하한 \
         {MIN_INIT_CLOSURES}, 2026-09-08 실측 2).\n\
         ★ 하한을 내려서 통과시키지 마라 — 0 이면 아래 판정은 빈 집합 위에서 돌고 \
         그 결과는 위반 없음이 아니라 **무의미**다. 표지(`{SPAWN_MARK}`)의 형태가 \
         바뀌었으면 그것을 고쳐라."
    );
    assert!(
        problems.is_empty(),
        "프로세스를 띄우기 **전에** 래치를 안 거는 초기화 클로저가 있다:\n{}\n\n\
         `OnceLock::get_or_init` 은 클로저가 panic 하면 미초기화로 남아 다음 테스트가 \
         그대로 다시 돈다. 래치가 `spawn()` 뒤에 있으면 두 번째 프로세스는 **이미 뜬 \
         뒤**라 아무것도 안 막는다.\n\
         ★ 이 배치가 깨져도 컴파일과 단위 시험은 초록이다 — 드러나는 곳은 부팅이 막힌 \
         환경의 벽시계뿐이고 그 조건을 만드는 자동 잡이 없다. 그래서 이 판정이 있다.",
        problems.join("\n")
    );
}

/// 술어가 세 갈래를 실제로 가르는가 — **합성 문자열로만** 판정한다.
///
/// 양성 대조(2026-09-08, 내 트리): 술어를 순서에서 **존재**로 되돌리면
/// (`Some(at) if at < spawn_at` 을 지워 `Some(_)` 하나로 합친다) 이 시험이 rc=101 로 죽고,
/// **같은 파일의 실물 판정은 초록으로 남는다** — 지금 레포의 클로저가 전부 `LatchFirst`
/// 라 그 회귀가 실물에는 안 나타나기 때문이다. 이 픽스처가 더하는 커버리지가 정확히
/// 그 구간이다.
///
/// 레포의 실제 하네스를 픽스처로 쓰면 그 하네스에 대해 항진명제가 된다(하네스가 바뀌면
/// 픽스처도 따라 바뀌어 아무것도 안 잰다). 여기서 재는 것은 **메커니즘**이고, 실물이
/// 그 메커니즘을 통과하는지는 위 시험이 따로 잰다.
#[test]
fn the_predicate_separates_order_from_presence() {
    let ok = "let x = CELL.get_or_init(|| { L.entering(\"h\"); Inst::spawn() });";
    assert_eq!(verdicts(ok), vec![Verdict::LatchFirst]);

    let after = "let x = CELL.get_or_init(|| { let i = Inst::spawn(); L.entering(\"h\"); i });";
    assert_eq!(
        verdicts(after),
        vec![Verdict::LatchAfterSpawn],
        "래치가 뒤에 있는 것은 없는 것과 같다 — 존재가 아니라 순서를 물어야 한다"
    );

    let none = "let x = CELL.get_or_init(|| { Inst::spawn() });";
    assert_eq!(verdicts(none), vec![Verdict::NoLatch]);

    let inline =
        "let x = CELL.get_or_init(|| { assert!(!F.swap(true, O::SeqCst)); Inst::spawn() });";
    assert_eq!(
        verdicts(inline),
        vec![Verdict::LatchFirst],
        "인라인 사본도 같은 뜻이다 — 이 판정은 추상화의 모양을 요구하지 않는다"
    );

    let unrelated = "let x = CELL.get_or_init(|| { compute() });";
    assert!(
        verdicts(unrelated).is_empty(),
        "프로세스를 안 띄우는 클로저는 이 판정의 대상이 아니다"
    );
}

/// 중첩된 블록이 있어도 클로저 끝을 옳게 찾는가.
///
/// 양성 대조(2026-09-08, 내 트리): 중첩에서 깊이를 안 올리게 하면(첫 `}` 가 본문을 끊는다)
/// 이 시험만 rc=101 로 죽는다 — 형제 둘은 초록이다.
///
/// ★ 처음 고른 변이는 **무효였다.** 깊이 계산을 통째로 지우면 `-D unused-assignments` 가
/// 컴파일에서 막아 `test result:` 줄 자체가 안 나온다 — rc 는 0 이 아닌데 내 시험은 아무
/// 말도 안 한 것이다. 그래서 깊이가 계속 쓰이는 형태로 다시 지었다.
#[test]
fn the_body_scan_survives_a_nested_block() {
    let nested =
        "CELL.get_or_init(|| { if c { touch(); } L.entering(\"h\"); Inst::spawn() }); after();";
    assert_eq!(verdicts(nested), vec![Verdict::LatchFirst]);
}
