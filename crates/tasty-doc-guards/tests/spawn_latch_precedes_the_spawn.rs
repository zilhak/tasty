//! 프로세스를 만드는 get_or_init 클로저가 spawn 전에 재시도 방지 래치를 거는지 확인한다.
//! OnceLock 초기화가 panic하면 다음 호출이 다시 초기화를 시도하므로, 부팅 실패 때
//! 프로세스 생성과 대기가 반복될 수 있다. 래치를 spawn 뒤에 두면 두 번째 생성은 막지 못한다.
//! 실제 부팅을 실행하는 검사가 아니라 마스킹한 소스에서 표지의 순서를 비교한다.

use tasty_doc_guards::source_text::mask_non_code;

const SPAWN_MARK: &str = "::spawn()";

/// 공용 SpawnOnceLatch와 인라인 AtomicBool 래치의 표지를 모두 인정한다.
const LATCH_MARKS: &[&str] = &[".entering(", ".swap(true,"];

/// 2026-09-08에 두 하네스를 측정했다. 한 하네스가 검사에서 빠지지 않도록 하한2로 둔다.
const MIN_INIT_CLOSURES: usize = 2;

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    /// 래치가 `spawn()` 앞에 있다.
    LatchFirst,
    /// 래치가 아예 없다.
    NoLatch,
    /// 래치가 있는데 `spawn()` 뒤다 — 두 번째 프로세스는 이미 떴다.
    LatchAfterSpawn,
}

/// 주석·문자열을 지운 코드에서 get_or_init 클로저를 추출한다.
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
        "프로세스를 만드는 초기화 클로저를 {seen}개만 찾았다(하한 {MIN_INIT_CLOSURES}, 2026-09-08 측정2개). 하네스와 표지 {SPAWN_MARK}를 확인한다. 검사 누락을 하한 변경으로 숨기지 않는다."
    );
    assert!(
        problems.is_empty(),
        "프로세스 생성 전에 래치를 걸지 않은 초기화 클로저다:\n{}\nOnceLock 초기화가 panic하면 다음 테스트가 다시 시도한다. 실패 후 프로세스를 반복 생성하지 않도록 래치를 spawn 앞에 둔다.",
        problems.join("\n")
    );
}

/// 실제 하네스와 독립된 합성 코드로 래치 없음·앞·뒤를 구분하는지 확인한다.
#[test]
fn the_predicate_separates_order_from_presence() {
    let ok = "let x = CELL.get_or_init(|| { L.entering(\"h\"); Inst::spawn() });";
    assert_eq!(verdicts(ok), vec![Verdict::LatchFirst]);

    let after = "let x = CELL.get_or_init(|| { let i = Inst::spawn(); L.entering(\"h\"); i });";
    assert_eq!(
        verdicts(after),
        vec![Verdict::LatchAfterSpawn],
        "spawn 뒤의 래치는 프로세스 재생성을 막지 못한다"
    );

    let none = "let x = CELL.get_or_init(|| { Inst::spawn() });";
    assert_eq!(verdicts(none), vec![Verdict::NoLatch]);

    let inline =
        "let x = CELL.get_or_init(|| { assert!(!F.swap(true, O::SeqCst)); Inst::spawn() });";
    assert_eq!(
        verdicts(inline),
        vec![Verdict::LatchFirst],
        "인라인 AtomicBool 래치도 인정해야 한다"
    );

    let unrelated = "let x = CELL.get_or_init(|| { compute() });";
    assert!(
        verdicts(unrelated).is_empty(),
        "프로세스를 안 띄우는 클로저는 이 판정의 대상이 아니다"
    );
}

#[test]
fn the_body_scan_survives_a_nested_block() {
    let nested =
        "CELL.get_or_init(|| { if c { touch(); } L.entering(\"h\"); Inst::spawn() }); after();";
    assert_eq!(verdicts(nested), vec![Verdict::LatchFirst]);
}
