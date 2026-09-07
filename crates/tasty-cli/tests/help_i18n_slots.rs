//! 도움말 키가 트리와 **집합 동등**인가, 그리고 키가 유일한가.
//!
//! 이 가드가 답하는 것은 "번역이 좋은가" 가 아니다 — 그건 기계가 못 본다. 답하는 것은
//! **자리마다 키가 하나 있고, 키가 서로 다른 자리를 가리키는가**다. 그 물음은 판정된다.

use clap::CommandFactory;
use std::collections::BTreeMap;

fn slots() -> Vec<tasty_cli::help_i18n::Slot> {
    tasty_cli::help_i18n::slots(&tasty_cli::Cli::command())
}

/// 모수가 비면 아래 대조가 전부 공짜로 통과한다. 하한은 실측보다 넉넉히 낮게 잡아
/// 정상적인 증감에 안 걸리게 하되, **순회가 깨진 것**은 잡는다.
const MIN_SLOTS: usize = 400;

#[test]
fn the_walk_finds_a_credible_number_of_help_slots() {
    let n = slots().len();
    assert!(
        n >= MIN_SLOTS,
        "도움말 자리를 {n} 개만 걸었다(하한 {MIN_SLOTS}) — 순회가 깨졌다. \
         이 상태의 '누락 0' 은 아무것도 안 본 0 이다"
    );
}

#[test]
fn every_key_names_exactly_one_slot() {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut clashes = Vec::new();
    for s in slots() {
        if let Some(prev) = seen.insert(s.key.clone(), s.english.clone())
            && prev != s.english
        {
            clashes.push(format!("{} — {prev:?} vs {:?}", s.key, s.english));
        }
    }
    assert!(
        clashes.is_empty(),
        "한 키가 서로 다른 두 자리를 가리킨다: {clashes:#?}\n\
         키가 겹치면 한쪽 번역이 다른 쪽에 새어 나가고, 그 오작동은 **번역을 넣은 뒤에야** \
         보인다 — 영어에서는 두 자리가 각자 원문을 쓰므로 아무 증상이 없다."
    );
}

#[test]
fn no_slot_carries_an_empty_string() {
    let empty: Vec<String> = slots()
        .into_iter()
        .filter(|s| s.english.trim().is_empty())
        .map(|s| s.key)
        .collect();
    assert!(
        empty.is_empty(),
        "빈 도움말 문자열에 키가 붙었다: {empty:?}\n\
         번역자가 채울 것이 없는 키는 목록의 소음이고, parity 가드에서는 결함으로 세어진다."
    );
}

#[test]
fn the_root_and_a_nested_subcommand_are_both_reachable() {
    let keys: Vec<String> = slots().into_iter().map(|s| s.key).collect();
    assert!(
        keys.iter().any(|k| k == "cli.help._root.about"),
        "루트 about 의 키가 없다 — 순회가 루트를 건너뛴다"
    );
    assert!(
        keys.iter().any(|k| k.matches('.').count() >= 4),
        "중첩 서브커맨드의 키가 하나도 없다 — 순회가 한 층만 내려간다"
    );
    assert!(
        keys.iter().any(|k| k.contains(".arg.")),
        "인자 키가 하나도 없다 — 인자 순회가 빠졌다"
    );
}

#[test]
fn no_key_is_a_prefix_of_another() {
    // 언어 카탈로그는 TOML 이다. **한 키가 값이면서 동시에 하위 테이블일 수 없다** —
    // `a.b` 를 문자열로 쓰면서 `a.b.c` 를 그 아래 두는 것은 파서가 거부한다. 그래서 값이
    // 놓이는 자리에는 항상 잎 마디(`about`/`long`/`help`)를 붙인다. 이 시험이 그 규율을
    // 지킨다 — 어기면 `lang/*.toml` 이 **파싱 단계에서** 깨지고, 그때는 도움말이 아니라
    // 번역 전체가 안 올라온다.
    let keys: Vec<String> = slots().into_iter().map(|s| s.key).collect();
    let mut bad = Vec::new();
    for a in &keys {
        for b in &keys {
            if a != b && b.starts_with(&format!("{a}.")) {
                bad.push(format!("  {a}  ⊂  {b}"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "한 키가 다른 키의 앞머리다 — TOML 로 표현되지 않는다:\n{}\n\
         서브커맨드나 인자 이름이 잎 마디와 같으면 이 일이 난다.",
        bad.join("\n")
    );
}
