//! 도움말 키의 유일성, TOML 구조, 명령·인자 설명 누락을 검사한다.

use clap::CommandFactory;
use std::collections::BTreeMap;

fn slots() -> Vec<tasty_cli::help_i18n::Slot> {
    tasty_cli::help_i18n::slots(&tasty_cli::Cli::command())
}

/// 순회 누락을 감지하는 하한. 정상적인 명령 수 증감에는 여유를 둔다.
const MIN_SLOTS: usize = 400;

#[test]
fn the_walk_finds_a_credible_number_of_help_slots() {
    let n = slots().len();
    assert!(
        n >= MIN_SLOTS,
        "도움말 {n}개가 하한 {MIN_SLOTS}개보다 적다. 순회 누락을 확인한다."
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
        "하나의 번역 키가 서로 다른 도움말을 가리킨다: {clashes:#?}"
    );
}

#[test]
fn no_slot_carries_an_empty_string() {
    let empty: Vec<String> = slots()
        .into_iter()
        .filter(|s| s.english.trim().is_empty())
        .map(|s| s.key)
        .collect();
    assert!(empty.is_empty(), "빈 도움말에 번역 키가 붙었다: {empty:?}");
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
    // TOML 키가 값이면서 하위 테이블일 수 없으므로 about/long/help로 끝나야 한다.
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
        "TOML 키가 문자열 값과 하위 테이블을 동시에 가리킨다:\n{}",
        bad.join("\n")
    );
}

// 기존 문자열 순회로는 설명 자체가 없는 명령·인자를 찾을 수 없어 별도로 검사한다.

/// 설명이 없는 **인자** 자리를 모은다. `help` · `version` 은 clap 내장이라 뺀다.
fn arg_holes(cmd: &clap::Command, path: &str, seen: &mut usize, out: &mut Vec<String>) {
    for arg in cmd.get_arguments() {
        let id = arg.get_id().as_str();
        if id == "help" || id == "version" {
            continue;
        }
        *seen += 1;
        if arg.get_help().is_none() {
            // 소스의 필드명과 사용자가 입력하는 플래그명을 함께 표시한다.
            let shown = match arg.get_long() {
                Some(long) if long != id => format!("--{long} ({id})"),
                Some(long) => format!("--{long}"),
                None => format!("<{id}>"),
            };
            out.push(format!("{path} {shown}"));
        }
    }
    for sub in cmd.get_subcommands() {
        if sub.get_name() == "help" {
            continue;
        }
        let here = if path.is_empty() {
            sub.get_name().to_string()
        } else {
            format!("{path} {}", sub.get_name())
        };
        arg_holes(sub, &here, seen, out);
    }
}

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

/// 빈 순회가 누락 없음으로 통과하지 않도록 명령 수 하한을 확인한다.
const MIN_SUBCOMMANDS: usize = 200;

#[test]
fn every_subcommand_carries_an_about() {
    let cmd = tasty_cli::Cli::command();
    let mut seen = 0usize;
    let mut holes = Vec::new();
    about_holes(&cmd, "", &mut seen, &mut holes);

    assert!(
        seen >= MIN_SUBCOMMANDS,
        "명령 {seen}개가 하한 {MIN_SUBCOMMANDS}개보다 적다. 순회 누락을 확인한다."
    );
    assert!(
        holes.is_empty(),
        "설명이 없는 명령: {holes:#?}\ndoc 주석이 해당 variant 바로 위에 있는지 확인한다."
    );
}

/// 같은 이유로 인자 수에도 하한을 둔다.
const MIN_ARGS: usize = 400;

/// 설명 없는 인자 수를 고정한다. 증가하면 설명을 보완하고 감소하면 이 값도 낮춘다.
/// 실패를 없애려고 값을 올리지 않는다.
const ARG_HOLE_CAP: usize = 0;

#[test]
fn arguments_without_help_do_not_increase() {
    let cmd = tasty_cli::Cli::command();
    let mut seen = 0usize;
    let mut holes = Vec::new();
    arg_holes(&cmd, "", &mut seen, &mut holes);
    holes.sort();

    assert!(
        seen >= MIN_ARGS,
        "인자 {seen}개가 하한 {MIN_ARGS}개보다 적다. 순회 누락을 확인한다."
    );

    // 상한이 0이어도 증가·감소를 모두 검사한다. 두 부등식은 한쪽이 항상 참이므로 Ordering을 쓴다.
    let n = holes.len();
    match n.cmp(&ARG_HOLE_CAP) {
        std::cmp::Ordering::Greater => panic!(
            "설명 없는 인자가 {n}개로 상한 {ARG_HOLE_CAP}개보다 {}개 많다:\n{holes:#?}\n각 필드 바로 위에 도움말 주석을 작성한다.",
            n - ARG_HOLE_CAP
        ),
        std::cmp::Ordering::Less => panic!(
            "설명 없는 인자가 {n}개로 줄었다. ARG_HOLE_CAP({ARG_HOLE_CAP})도 {n}으로 낮춰 다시 늘어나는 것을 검출한다."
        ),
        std::cmp::Ordering::Equal => {}
    }
}

// 설명 존재 여부만으로는 두 항목의 doc 주석이 이어 붙은 경우를 찾지 못한다.
// 같은 첫 단어로 다시 시작하는 첫 문단을 후보로 찾는다.

/// 종결부호 뒤 공백 또는 부호 없이 대문자+소문자가 시작하는 곳에서 문장을 나눈다.
/// 후자는 clap이 여러 doc 주석 줄을 공백으로 이어 붙인 경우를 찾기 위한 규칙이다.
fn sentences(text: &str) -> Vec<&str> {
    let text = text.trim();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut i = 0usize;
    while i < chars.len() {
        if !chars[i].1.is_whitespace() {
            i += 1;
            continue;
        }
        let ws = i;
        while i < chars.len() && chars[i].1.is_whitespace() {
            i += 1;
        }
        if ws == 0 || i >= chars.len() {
            continue;
        }
        let prev = chars[ws - 1].1;
        let next = chars[i].1;
        let after_next = chars.get(i + 1).map(|c| c.1);
        let ends_sentence = matches!(prev, '.' | '!' | '?');
        let glued = !matches!(prev, '.' | '!' | '?' | ':' | ';' | ',')
            && next.is_ascii_uppercase()
            && after_next.is_some_and(|c| c.is_ascii_lowercase());
        if ends_sentence || glued {
            out.push(text[start..chars[ws].0].trim());
            start = chars[i].0;
        }
    }
    let tail = text[start..].trim();
    if !tail.is_empty() {
        out.push(tail);
    }
    out
}

/// 문장의 첫 낱말(소문자로). 앞에 붙은 따옴표·괄호·별표는 벗긴다. 낱말로 시작하지
/// 않으면(`--flag` 처럼) `None` — 세지 않는다.
fn first_word(sentence: &str) -> Option<String> {
    let s = sentence.trim_start_matches(['`', '"', '\'', '(', '[', '*']);
    if !s.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let end = s
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '/' || c == '-'))
        .unwrap_or(s.len());
    Some(s[..end].to_ascii_lowercase())
}

/// 첫 문단의 뒤 문장이 첫 문장과 같은 단어로 시작하면 반환한다.
/// 모든 문장 쌍을 비교하면 The/With 같은 정상 반복까지 걸린다.
/// 작성자가 나눈 둘째 문단은 검사하지 않는다. clap이 빈 줄 없이 이어 붙이는 경우만 찾는다.
fn restarts_with_its_opening_word(text: &str) -> Option<String> {
    let text = text.split("\n\n").next().unwrap_or(text);
    let parts = sentences(text);
    let opening = first_word(parts.first()?)?;
    parts
        .iter()
        .skip(1)
        .find_map(|s| first_word(s).filter(|w| *w == opening))
}

#[test]
fn no_help_slot_restarts_with_its_opening_word() {
    let slots = slots();
    assert!(
        slots.len() >= MIN_SLOTS,
        "도움말 {}개가 하한 {MIN_SLOTS}개보다 적다. 순회 누락을 확인한다.",
        slots.len()
    );

    let mut hits: Vec<String> = slots
        .iter()
        .filter_map(|s| {
            restarts_with_its_opening_word(&s.english).map(|w| {
                format!(
                    "  {} — `{w}` 로 두 번 시작한다\n      {:?}",
                    s.key, s.english
                )
            })
        })
        .collect();
    hits.sort();

    assert!(
        hits.is_empty(),
        "다른 항목의 doc 주석이 이어 붙었을 수 있다:\n{}\n주석의 소속을 확인하고 잘못 붙은 설명을 옮기거나 삭제한다. 단순한 문장 반복도 후보가 될 수 있다.",
        hits.join("\n")
    );
}

/// 이어 붙은 설명과 정상 설명을 같은 판정 함수로 대조한다.
#[test]
fn the_attribution_predicate_fires_on_glue_and_stays_quiet_on_prose() {
    // 종결부호가 있는 경우와 없는 경우를 모두 검사한다.
    let glued = [
        (
            "set cwd",
            "Set a global hook (timer-based) Set the working directory a remote surface \
             reports to the host",
        ),
        (
            "list theme",
            "Show queue status (count + preview of pending messages) Show the resolved \
             global theme snapshot (colors, font sizes, ui scale)",
        ),
        (
            "telemetry record-batch",
            "Record a single metric event. Record several events in one call — they share \
             one timestamp, so their order is preserved.",
        ),
        // 문단이 뒤에 붙어 있어도 잔여는 첫 문단 안에 있으므로 그대로 걸려야 한다.
        (
            "잔여 + 정상 둘째 문단",
            "Set a global hook (timer-based) Set the working directory a remote surface \
             reports to the host.\n\nThe value is what the surface last reported.",
        ),
    ];
    for (label, text) in glued {
        assert!(
            restarts_with_its_opening_word(text).is_some(),
            "이어 붙은 설명을 검출하지 못했다 ({label}): {text:?}"
        );
    }

    let prose = [
        (
            "기능어 반복",
            "Attach using a saved tasty-attach profile name. The profile is resolved from \
             the profile file. The profile's values replace the inline ones.",
        ),
        (
            "두 문장, 다른 시작",
            "Set a read mark on a surface. Later reads return only what came after it.",
        ),
        ("한 문장", "Remove the hook after it fires once"),
        (
            "둘째 문단이 첫 낱말을 다시 쓴다",
            "Secret memory store. CLI acts as `_host` owner.\n\nSecret means \"another \
             owner cannot read it\", and nothing beyond that.",
        ),
    ];
    for (label, text) in prose {
        assert!(
            restarts_with_its_opening_word(text).is_none(),
            "정상 설명을 잘못 검출했다 ({label}): {text:?}"
        );
    }
}
