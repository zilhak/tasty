//! 사용자 가이드의 CLI 입문·참조·진단 페이지에 실린 `tasty …` 예시를 실제 clap 트리로 파싱한다.
//! 명령 경로·필수 인자·옵션 이름이 맞는지만 본다. 문장의 정확성과 실행 결과는 보장하지 않는다.
//! plugin 명령, 자리 표시(`<ID>`·`…`), 셸 치환·파이프가 있는 줄은 이 검사가 판정하지 않고 건너뛴다.

use clap::Parser;
use std::path::PathBuf;
use tasty_cli::Cli;

const PAGES: &[&str] = &[
    "agents/cli.md",
    "agents/cli-reference.md",
    "help/cli-diagnostics.md",
];

/// plugin이 더하는 최상위 명령. 코어 parser에 없으므로 건너뛴다.
const PLUGIN_TOP: &[&str] = &[
    "claude",
    "codex",
    "markdown",
    "html",
    "image",
    "agent-stream",
];

fn site_content() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../site/content")
}

/// 작은따옴표·큰따옴표만 아는 단어 분리. 따옴표 밖의 `#` 부터는 주석이다.
fn words(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for ch in line.chars() {
        match quote {
            Some(q) if ch == q => quote = None,
            Some(_) => cur.push(ch),
            None if ch == '\'' || ch == '"' => {
                quote = Some(ch);
                started = true;
            }
            None if ch == '#' && !started => break,
            None if ch.is_whitespace() => {
                if started {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            None => {
                cur.push(ch);
                started = true;
            }
        }
    }
    if started {
        out.push(cur);
    }
    out
}

fn judged(line: &str) -> bool {
    let skip = ["<", "…", "...", "$(", "|", "$'", "\\", "&&", ";"];
    line.starts_with("tasty ") && !skip.iter().any(|s| line.contains(s))
}

/// sh 코드 블록의 판정 대상 줄을 (페이지, 줄) 로 모은다.
fn examples(rel: &str) -> Vec<String> {
    let text = std::fs::read_to_string(site_content().join(rel)).unwrap();
    let mut out = Vec::new();
    let mut in_sh = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("```") {
            in_sh = !in_sh && t == "```sh";
            continue;
        }
        if in_sh && judged(t) {
            out.push(t.to_string());
        }
    }
    out
}

#[test]
fn guide_examples_parse_with_the_real_command_tree() {
    let mut checked = 0;
    let mut failures = Vec::new();
    for lang in ["", "en/"] {
        for page in PAGES {
            let rel = format!("{lang}{page}");
            for line in examples(&rel) {
                let argv = words(&line);
                if argv
                    .get(1)
                    .is_some_and(|w| PLUGIN_TOP.contains(&w.as_str()))
                {
                    continue;
                }
                checked += 1;
                match Cli::try_parse_from(&argv) {
                    Ok(_) => {}
                    // `tasty help <명령>` 은 도움말을 출력하는 정상 경로다.
                    Err(e) if e.kind() == clap::error::ErrorKind::DisplayHelp => {}
                    Err(e) => failures.push(format!("{rel}: {line}\n  {}", e.kind())),
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    // 2026-10-15 실측 판정 수는 193건이다. 가이드에서 예시가 크게 줄면 판정 범위부터 확인한다.
    assert!(checked >= 150, "판정한 예시가 {checked}건뿐이다");
}

#[test]
fn the_parser_rejects_paths_the_guides_must_not_use() {
    for line in [
        "tasty surface list",
        "tasty close workspace",
        "tasty read since-mark --cursor 1 --stream",
        "tasty pty state",
    ] {
        assert!(Cli::try_parse_from(words(line)).is_err(), "{line}");
    }
}

#[test]
fn words_keeps_quoted_text_and_drops_comments() {
    assert_eq!(
        words(r#"tasty send text "ls\r" --surface 57   # 보내기"#),
        ["tasty", "send", "text", r"ls\r", "--surface", "57"]
    );
    assert_eq!(words("tasty notify 'a # b'"), ["tasty", "notify", "a # b"]);
}
