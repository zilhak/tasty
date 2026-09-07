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

// ========== 번역 이전의 결함 — 설명이 아예 없는 자리 ==========
//
// 슬롯 걷기는 **있는 문자열**만 센다. 그래서 `about` 이 없는 서브커맨드는 이 파일의
// 다른 술어 어디에도 안 걸린다 — 빈 문자열이 아니라 **자리 자체가 없기** 때문이다.
// 그 조용함이 실물로 새어 나온 적이 있다: `tasty list` 의 `queue` 는 doc 주석이 한 칸
// 위 variant 에 붙어 있어서 `--help` 에 설명이 빈칸으로 나왔고, 같은 실수로 `theme` 은
// 남의 설명까지 이어 붙여 내보냈다. 번역은 그것을 못 고친다 — 번역할 원문이 없다.

/// `about` 이 없는 서브커맨드 자리를 모은다. `help` 는 clap 내장이라 뺀다.
fn about_holes(cmd: &clap::Command, path: &str, seen: &mut usize, out: &mut Vec<String>) {
    for sub in cmd.get_subcommands() {
        let name = sub.get_name();
        if name == "help" {
            continue;
        }
        let here = if path.is_empty() {
            name.to_string()
        } else {
            format!("{path} {name}")
        };
        *seen += 1;
        if sub.get_about().is_none() {
            out.push(here.clone());
        }
        about_holes(sub, &here, seen, out);
    }
}

/// 모수 고정 — 걷기가 깨져 0 자리를 보면 이 술어는 공짜로 초록이다(ADR-0133).
/// 하한이다: 명령은 늘 수 있고, 줄면 그때 이 줄이 먼저 말한다.
const MIN_SUBCOMMANDS: usize = 200;

#[test]
fn every_subcommand_carries_an_about() {
    let cmd = tasty_cli::Cli::command();
    let mut seen = 0usize;
    let mut holes = Vec::new();
    about_holes(&cmd, "", &mut seen, &mut holes);

    assert!(
        seen >= MIN_SUBCOMMANDS,
        "서브커맨드를 {seen} 개만 봤다(하한 {MIN_SUBCOMMANDS}) — 걷기가 깨졌다. \
         이 술어는 볼 것이 없으면 공짜로 초록이다."
    );
    assert!(
        holes.is_empty(),
        "설명(`about`) 이 없는 서브커맨드가 있다: {holes:#?}\n\
         `--help` 목록에서 그 줄은 이름만 나오고 설명 칸이 빈다. 흔한 원인은 doc 주석이 \
         **바로 아래 항목**에 귀속된다는 것을 놓치고 한 칸 위 variant 에 붙인 것이다 — \
         그러면 위 항목은 남의 설명까지 이어 붙여 내보내고 아래 항목은 빈다. \
         번역으로는 못 고친다: 원문이 없으면 슬롯도 없고, 슬롯이 없으면 번역할 자리가 없다."
    );
}
