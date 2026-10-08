//! 갤러리 소스의 문자열 리터럴 가운데 `lang/en.toml` 값과 글자 단위로 같은 것을 모아
//! `lang_copies.txt` 기준과 비교한다.
//!
//! 갤러리는 본체 문구를 `tasty_gallery::i18n::t()` 로 읽을 수 있다. 아직 리터럴로 남은 사본은
//! 기준 파일에 적혀 있고, 다음 두 경우에 실패한다.
//! - 새 리터럴이 lang 값과 같아졌다(새 손 사본). `t()` 로 바꾸거나 기준에 더한다.
//! - 기준에 있던 사본이 lang 값과 더는 같지 않다(lang 문구가 바뀌었거나 갤러리 문구가 바뀌었다).
//!   갤러리 문구를 맞추거나 `t()` 로 바꾸고 기준을 고친다.
//!
//! 기준 줄은 `경로<TAB>리터럴<TAB>같은 값을 가진 키들` 이다. 리터럴의 줄바꿈과 탭은 `\n`·`\t` 로 적는다.
//! `TASTY_GALLERY_LANG_COPIES_WRITE=1` 로 실행하면 현재 상태로 기준 파일을 다시 쓴다.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use tasty_doc_guards::floored_walk::{CountedOn, Descend, Floor, walk_with_floor};

const BASELINE: &str = "tests/lang_copies.txt";
const WRITE_ENV: &str = "TASTY_GALLERY_LANG_COPIES_WRITE";

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 본체 영어 번역표를 점 키로 펼친 값 → 키 목록.
fn english_values() -> HashMap<String, BTreeSet<String>> {
    let path = manifest_dir().join("../../lang/en.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 을 읽지 못했다: {e}", path.display()));
    let value: toml::Value = toml::from_str(&text)
        .unwrap_or_else(|e| panic!("{} 을 해석하지 못했다: {e}", path.display()));
    let mut flat = HashMap::new();
    tasty_i18n::flatten_catalog_toml("", &value, &mut flat);
    let mut by_value: HashMap<String, BTreeSet<String>> = HashMap::new();
    for (key, val) in flat {
        if key.starts_with("meta.") || val.is_empty() {
            continue;
        }
        by_value.entry(val).or_default().insert(key);
    }
    by_value
}

/// Rust 소스에서 문자열 리터럴의 값을 꺼낸다. 주석·문자 리터럴·바이트 문자열은 건너뛴다.
fn string_literals(src: &str) -> Vec<String> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'/' if b.get(i + 1) == Some(&b'/') => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b'/' if b.get(i + 1) == Some(&b'*') => {
                let mut depth = 1;
                i += 2;
                while i < b.len() && depth > 0 {
                    if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                        depth += 1;
                        i += 2;
                    } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                        depth -= 1;
                        i += 2;
                    } else {
                        i += 1;
                    }
                }
            }
            b'\'' => i = skip_char_or_lifetime(b, i),
            b'r' | b'b' if !prev_is_ident(b, i) => {
                let (is_byte, mut j) = if b[i] == b'b' {
                    (true, i + 1)
                } else {
                    (false, i)
                };
                if b.get(j) == Some(&b'r') {
                    j += 1;
                    let hashes = b[j..].iter().take_while(|&&c| c == b'#').count();
                    if b.get(j + hashes) == Some(&b'"') {
                        let start = j + hashes + 1;
                        let close: Vec<u8> = std::iter::once(b'"')
                            .chain(std::iter::repeat_n(b'#', hashes))
                            .collect();
                        let end = find(b, start, &close);
                        if !is_byte {
                            out.push(src[start..end].to_string());
                        }
                        i = end + close.len();
                        continue;
                    }
                } else if is_byte && b.get(j) == Some(&b'"') {
                    i = cooked_end(b, j + 1) + 1;
                    continue;
                } else if is_byte && b.get(j) == Some(&b'\'') {
                    i = skip_char_or_lifetime(b, j);
                    continue;
                }
                i += 1;
            }
            b'"' => {
                let end = cooked_end(b, i + 1);
                out.push(unescape(&src[i + 1..end]));
                i = end + 1;
            }
            _ => i += 1,
        }
    }
    out
}

fn prev_is_ident(b: &[u8], i: usize) -> bool {
    i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_')
}

fn find(b: &[u8], from: usize, pat: &[u8]) -> usize {
    (from..b.len())
        .find(|&k| b[k..].starts_with(pat))
        .unwrap_or(b.len())
}

/// 닫는 `"` 의 위치.
fn cooked_end(b: &[u8], mut i: usize) -> usize {
    while i < b.len() {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i,
            _ => i += 1,
        }
    }
    b.len()
}

/// `'a'`·`'\n'`·`'\u{2014}'` 는 끝까지, `'a` 같은 수명은 따옴표 하나만 건너뛴다.
fn skip_char_or_lifetime(b: &[u8], i: usize) -> usize {
    if b.get(i + 1) == Some(&b'\\') {
        let mut j = i + 2;
        while j < b.len() && b[j] != b'\'' {
            j += 1;
        }
        return j + 1;
    }
    // 여러 바이트 UTF-8 문자 하나 뒤에 닫는 따옴표가 오면 문자 리터럴이다.
    let rest = std::str::from_utf8(&b[i + 1..]).ok();
    if let Some(ch) = rest.and_then(|r| r.chars().next()) {
        let after = i + 1 + ch.len_utf8();
        if b.get(after) == Some(&b'\'') {
            return after + 1;
        }
    }
    i + 1
}

fn unescape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('0') => out.push('\0'),
            Some('\\') => out.push('\\'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('\n') => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
            }
            Some('u') => {
                let hex: String = chars
                    .by_ref()
                    .skip_while(|&c| c == '{')
                    .take_while(|&c| c != '}')
                    .collect();
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some('x') => {
                let hex: String = chars.by_ref().take(2).collect();
                if let Ok(v) = u8::from_str_radix(&hex, 16) {
                    out.push(char::from(v));
                }
            }
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn escape_field(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}

/// 현재 소스의 사본 목록. 기준 파일의 줄과 같은 형식이다.
fn current_copies() -> BTreeSet<String> {
    let values = english_values();
    let root = manifest_dir();
    let floor = Floor {
        min: 160,
        measured: 174,
        measured_on: "2026-10-08",
        counted_on: CountedOn::Tree("6c156db7c — git ls-tree 로 src 아래 .rs 파일을 셌다."),
        why_this_gap: "Gallery specimens are split and merged often, so a drop of up to 14 source files is allowed; a larger drop means the walk lost most of the catalog.",
    };
    let files = walk_with_floor(
        &root.join("src"),
        &root,
        &floor,
        Descend::Everything,
        &|w| w.rel.ends_with(".rs"),
    )
    .unwrap_or_else(|why| panic!("{why}"));
    let mut copies: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    for file in files {
        let src = std::fs::read_to_string(&file.path)
            .unwrap_or_else(|e| panic!("{} 을 읽지 못했다: {e}", file.path.display()));
        let rel = file.rel;
        for lit in string_literals(&src) {
            if let Some(keys) = values.get(&lit) {
                copies
                    .entry((rel.clone(), lit))
                    .or_default()
                    .extend(keys.iter().cloned());
            }
        }
    }
    copies
        .into_iter()
        .map(|((path, lit), keys)| {
            let keys: Vec<_> = keys.into_iter().collect();
            format!("{path}\t{}\t{}", escape_field(&lit), keys.join(","))
        })
        .collect()
}

#[test]
fn gallery_copies_of_lang_text_match_the_baseline() {
    let current = current_copies();
    assert!(
        !current.is_empty(),
        "사본이 하나도 없다면 리터럴 추출이 멈춘 것인지 먼저 확인한다"
    );
    let baseline_path = manifest_dir().join(BASELINE);
    if std::env::var_os(WRITE_ENV).is_some() {
        let mut text = String::from(
            "# lang_copies.rs 가 비교하는 기준. 경로<TAB>리터럴<TAB>같은 값을 가진 키들.\n",
        );
        for line in &current {
            text.push_str(line);
            text.push('\n');
        }
        std::fs::write(&baseline_path, text)
            .unwrap_or_else(|e| panic!("{} 을 쓰지 못했다: {e}", baseline_path.display()));
        return;
    }
    let text = std::fs::read_to_string(&baseline_path)
        .unwrap_or_else(|e| panic!("{} 을 읽지 못했다: {e}", baseline_path.display()));
    let baseline: BTreeSet<String> = text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    let added: Vec<_> = current.difference(&baseline).collect();
    let gone: Vec<_> = baseline.difference(&current).collect();
    assert!(
        added.is_empty() && gone.is_empty(),
        "갤러리 리터럴과 lang/en.toml 값의 일치가 기준과 다르다.\n\
         새로 같아진 리터럴(새 손 사본이면 crate::i18n::t(\"키\") 로 읽는다):\n  {}\n\
         기준에서 사라진 사본(lang 문구나 갤러리 문구가 바뀌어 더는 같지 않다):\n  {}\n\
         갤러리 문구를 고친 뒤 {WRITE_ENV}=1 로 이 시험을 실행하면 기준을 다시 쓴다.",
        added
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  "),
        gone.iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  "),
    );
}

#[test]
fn literal_scanner_reads_rust_string_forms() {
    let src = concat!(
        "let a = \"plain\"; // \"comment\"\n",
        "/* \"block\" */ let b = r#\"raw \"q\"\"#;\n",
        "let c = '\"'; let d = b\"bytes\"; fn f<'a>(x: &'a str) {}\n",
        "let e = \"dash \\u{2014} \\\n    joined\";\n",
        "let g = '\u{2014}'; let h = \"after\";\n",
    );
    assert_eq!(
        string_literals(src),
        vec!["plain", "raw \"q\"", "dash \u{2014} joined", "after"]
    );
}
