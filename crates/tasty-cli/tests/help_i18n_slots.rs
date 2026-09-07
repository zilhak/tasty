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

/// 설명이 없는 **인자** 자리를 모은다. `help` · `version` 은 clap 내장이라 뺀다.
fn arg_holes(cmd: &clap::Command, path: &str, seen: &mut usize, out: &mut Vec<String>) {
    for arg in cmd.get_arguments() {
        let id = arg.get_id().as_str();
        if id == "help" || id == "version" {
            continue;
        }
        *seen += 1;
        if arg.get_help().is_none() {
            // 자리 이름은 clap **id**(필드명)이고 사용자가 치는 것은 long 플래그다 —
            // `workspace_id` 와 `--workspace-id` 처럼 갈린다. 고치러 갈 사람은 소스의
            // 필드를 찾아야 하고 그 화면을 보는 사람은 플래그를 찾으므로 둘 다 적는다.
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

/// 인자 모수 고정 — 서브커맨드 쪽과 같은 이유(ADR-0133). 하한이다.
const MIN_ARGS: usize = 400;

/// 설명이 없는 인자의 **상한**. 지금은 **0** — 잔여 0 이다. 처음 쟀을 때 204 였고
/// 계열 단위로 닫아 내려온 값이라, 여기까지는 잔여 0 이 아니라 **양방향 래칫**으로
/// 왔다: 늘어도 실패하고 **줄어도 실패한다**(줄였으면 이 수를 같이 내려라 — 남는
/// 여유가 곧 안 보는 구간이다). `check-allow-reason` 과 같은 형태다.
///
/// 0 에서는 아래쪽 갈래가 다시 터질 일이 없지만 그 문장을 지우지 않는다. 이 수가
/// 언젠가 올라가면 그때 돌아올 자리이고, 상한이 0 이라는 사실 자체가 그 갈래가
/// 살아 있어서 유지되는 것이기 때문이다.
///
/// ★ 이 수를 올려서 통과시키지 마라. 올리는 것은 `--help` 에 설명 없는 줄을 하나 더
/// 들이는 것을 승인하는 조작이다.
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
        "인자를 {seen} 개만 봤다(하한 {MIN_ARGS}) — 걷기가 깨졌다. \
         이 술어는 볼 것이 없으면 공짜로 초록이다."
    );

    // 두 갈래를 `assert!` 둘로 쓰면 상한이 0 일 때 clippy 가 막는다
    // (`absurd_extreme_comparisons` — `n <= 0` 과 `n >= 0` 은 usize 에서 한쪽이 항상
    // 참이다). 상한이 0 인 것은 이 게이트가 다 닫혔다는 뜻이지 갈래 하나를 버려도
    // 된다는 뜻이 아니라서, 비교를 `Ordering` 으로 한 번만 하고 갈래는 그대로 둔다.
    let n = holes.len();
    match n.cmp(&ARG_HOLE_CAP) {
        std::cmp::Ordering::Greater => panic!(
            "설명이 없는 인자가 {n} 개다(상한 {ARG_HOLE_CAP}) — {} 개 늘었다.\n{holes:#?}\n\
             `--help` 의 옵션 목록에서 그 줄은 이름만 나오고 설명 칸이 빈다. 흔한 원인은 \
             doc 주석이 **바로 아래 항목**에 귀속된다는 것을 놓친 것이다 — 그러면 위 항목은 \
             남의 설명까지 이어 붙여 내보내고 아래 항목은 빈다. \
             번역으로는 못 고친다: 원문이 없으면 슬롯도 없고, 슬롯이 없으면 번역할 자리가 없다.",
            n - ARG_HOLE_CAP
        ),
        std::cmp::Ordering::Less => panic!(
            "설명이 없는 인자가 {n} 개로 줄었다(상한 {ARG_HOLE_CAP}). 잘 고쳤다 — \
             `ARG_HOLE_CAP` 을 {n} 으로 같이 내려라. 상한을 안 내리면 그만큼이 \
             **안 보는 구간**으로 남아서, 다음에 다시 늘어도 이 시험이 침묵한다."
        ),
        std::cmp::Ordering::Equal => {}
    }
}

// ── 귀속: 한 자리의 설명은 **한 항목**의 것인가 ─────────────────────────

// 위의 `every_subcommand_carries_an_about` 은 설명이 **있는가**만 묻는다. 그 술어가
// 잡은 세 자리를 고칠 때 위쪽 항목에 이어 붙어 있던 잔여 줄 하나가 남았고, 이어 붙은
// 설명은 "있다" 이므로 그 술어에 정상으로 보였다. **채움 술어는 잘못 채워진 것을
// 통과시킨다** — 그리고 이 부류는 자리 수를 안 바꾸므로 어떤 계수 래칫에도 안 걸린다.
//
// 잔여의 모양은 정해져 있다. doc 주석은 **바로 아래 항목**에 귀속되므로 남의 줄은
// 언제나 그 항목 설명의 **맨 앞**에 온다. 그리고 한 enum 의 형제들은 같은 동사를
// 공유한다(`set hook` · `set cwd` · `set global-hook` 은 다 "Set" 로 시작한다).
// 그래서 잔여가 붙으면 설명이 **자기 첫 단어로 다시 시작한다**.

/// 문장 경계로 나눈다. **두 종류**를 경계로 본다:
///
/// 1. 종결부호(`.` `!` `?`) 뒤의 공백 — 보통의 문장 경계.
/// 2. **아무 부호 없이** 공백이 오고 다음이 `대문자+소문자` 로 시작하는 자리 — clap 이
///    문단이 안 끊긴 `///` 줄들을 공백 하나로 이어 붙인 자국이다.
///
/// 2 번이 없으면 잔여 줄이 마침표 없이 끝난 경우(`Set a global hook (timer-based)`)가
/// 한 문장으로 보여서 안 걸린다. 1 번이 없으면 마침표로 끝난 경우가 안 걸린다.
/// **두 종류를 다 봐야 세 건이 다 걸린다** — 아래 대조군이 그것을 고정한다.
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

/// 설명이 **자기 첫 낱말로 다시 시작**하면 그 낱말을 낸다.
///
/// 첫 문장과만 비교한다 — 잔여는 언제나 맨 앞에 오기 때문이다(doc 주석이 아래 항목에
/// 귀속된다는 성질에서 나온다). 아무 문장 쌍이나 비교하면 `The …` · `With …` 처럼
/// 정상적으로 반복되는 기능어에 걸린다(실측: 그렇게 재면 이 트리에서 7 자리가 울리고
/// 전부 정상이다).
///
/// **첫 문단만 본다.** clap 은 빈 줄이 없는 `///` 줄들을 공백 하나로 이어 붙이므로,
/// 위 항목에서 흘러온 잔여는 **반드시 첫 문단 안에** 들어간다 — 잔여가 문단을 새로
/// 여는 일은 구조상 없다. 반대로 `long_about` 의 둘째 문단은 글쓴이가 일부러 연
/// 것이고, 그 문단이 첫 낱말을 다시 쓰는 것은 흔한 정상이다(`Secret memory store.`
/// 다음의 `Secret means …`). 문단 경계를 안 보면 그 정상이 전부 거짓 양성이 된다.
///
/// 좁히는 방향이라 **거짓 음성 쪽으로만 움직인다** — 그런데 그 방향으로 새로
/// 놓치는 것은 "문단을 새로 열어 놓인 잔여" 뿐이고, 그것은 clap 의 이어 붙이기
/// 규칙상 만들어질 수 없다.
fn restarts_with_its_opening_word(text: &str) -> Option<String> {
    // 첫 문단 = 첫 빈 줄 앞. `about` 이 되는 부분이고 잔여가 살 수 있는 유일한 자리다.
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
        "도움말 자리를 {} 개만 걸었다(하한 {MIN_SLOTS}) — 순회가 깨졌다. \
         이 상태의 '위반 0' 은 아무것도 안 본 0 이다",
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
        "도움말 한 자리가 **두 항목분의 설명**을 담고 있다:\n{}\n\
         doc 주석은 **바로 아래 항목**에 귀속되므로, 위 항목에 남은 줄은 아래 항목 설명의 \
         맨 앞에 이어 붙는다. clap 은 문단이 안 끊긴 `///` 줄들을 공백 하나로 이어 붙이므로 \
         `--help` 에는 두 설명이 한 문장처럼 나간다.\n\
         ★ 이 부류는 **수로는 안 보인다** — 자리 수도 슬롯 수도 안 바뀌고, \
         `every_subcommand_carries_an_about` 은 설명이 **있는가**만 묻지 그것이 **한 항목의 \
         것인가**는 안 묻는다.\n\
         고치는 법: 남의 줄을 제 항목 위로 옮기거나 지운다. 값을 이 술어에 맞추려고 \
         문장을 고쳐 쓰지 마라 — 그것은 결함을 남기고 신호만 끄는 것이다.",
        hits.join("\n")
    );
}

/// 이 술어가 **울릴 것에 울리고 안 울릴 것에 안 우는가** — 양방향 대조군.
///
/// 위 시험은 잔여 0 이라 그것만으로는 술어가 죽었는지 트리가 깨끗한지 안 갈린다.
/// 왼쪽은 실제로 있었던 셋이고(전부 고쳐졌다), 오른쪽은 문장이 여럿인 정상 설명이다.
#[test]
fn the_attribution_predicate_fires_on_glue_and_stays_quiet_on_prose() {
    // 실제로 `--help` 로 나갔던 이어 붙은 설명 셋. 부호 없이 이어진 것 둘과
    // 마침표로 끝난 것 하나 — 문장 나누기의 두 갈래를 각각 고정한다.
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
            "이어 붙은 설명(`{label}`)에 술어가 안 울린다 — 술어가 죽었다: {text:?}"
        );
    }

    // 정상인데 문장이 여럿인 것. 여기서 울리면 술어가 산문을 문다.
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
            "정상 설명(`{label}`)에 술어가 울린다 — 산문을 물고 있다: {text:?}"
        );
    }
}
