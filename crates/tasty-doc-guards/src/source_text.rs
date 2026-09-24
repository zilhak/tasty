//! 소스 검사가 공유하는 주석·리터럴 마스킹과 Rust 파일 수집 함수.

use std::path::PathBuf;

/// 주석·문자열·문자 리터럴을 공백으로 바꾼다. 줄바꿈과 문자 수는 유지한다.
/// 라이프타임의 작은따옴표는 문자 리터럴과 구분한다.
pub fn mask_non_code(src: &str) -> String {
    mask(src, Fate::Blank, Fate::Blank, Fate::Blank)
}

/// mask_non_code와 같은 범위를 가리되 UTF-8 바이트 위치도 보존한다.
/// 구조를 마스킹한 사본에서 찾고 같은 바이트 구간의 값을 원문에서 읽을 때 사용한다.
pub fn mask_non_code_aligned(src: &str) -> String {
    mask(src, Fate::BlankBytes, Fate::BlankBytes, Fate::BlankBytes)
}

/// 주석만 가리고 리터럴은 남긴다. UTF-8 바이트 위치를 보존한다.
/// mask_non_code_aligned와 비교하면 리터럴과 공백/주석을 구별할 수 있다.
pub fn mask_comments_aligned(src: &str) -> String {
    mask(src, Fate::BlankBytes, Fate::Keep, Fate::Keep)
}

/// 문자열·문자 리터럴만 가리고 주석을 남긴다. 사유 주석을 검사할 때 사용한다.
/// 결과에 남은 //는 실제 주석이며 문자열 안의 URL 표기는 제거된다.
pub fn mask_literals(src: &str) -> String {
    mask(src, Fate::Keep, Fate::Blank, Fate::Blank)
}

/// 주석만 가리고 문자열·문자 리터럴을 남긴다. cfg의 문자열 값처럼 리터럴도 검사할 때 사용한다.
pub fn mask_comments(src: &str) -> String {
    mask(src, Fate::Blank, Fate::Keep, Fate::Keep)
}

/// 문자 리터럴 안의 큰따옴표만 바꾸고 나머지 내용은 보존한다.
/// tokei가 이를 문자열 시작으로 오독하지 않게 하는 SLOC 측정용 처리다.
/// 서로 다른 문자 리터럴이 같아질 수 있어 내용 비교에는 사용하지 않는다.
pub fn neutralize_char_literal_quotes(src: &str) -> String {
    mask(src, Fate::Keep, Fate::Keep, Fate::QuoteSafe)
}

/// 같은 렉서로 구분한 각 영역의 출력 방식.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fate {
    /// 공백으로 덮는다(줄바꿈은 남긴다).
    Blank,
    /// 공백으로 덮되 글자의 바이트 수만큼 채운다(줄바꿈은 남긴다).
    BlankBytes,
    /// 원문 그대로 둔다.
    Keep,
    /// 원문 그대로 두되 `"` 만 안전한 글자로 바꾼다.
    QuoteSafe,
}

fn mask(src: &str, comments: Fate, strings: Fate, char_literals: Fate) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    while i < chars.len() {
        i = match chars[i] {
            '/' if chars.get(i + 1) == Some(&'/') => {
                mask_line_comment(&chars, i, &mut out, comments)
            }
            '/' if chars.get(i + 1) == Some(&'*') => {
                mask_block_comment(&chars, i, &mut out, comments)
            }
            'r' | 'b' if raw_string_hashes(&chars, i).is_some() => {
                mask_raw_string(&chars, i, &mut out, strings)
            }
            '"' => mask_quoted(&chars, i, '"', &mut out, strings),
            '\'' if is_char_literal(&chars, i) => {
                mask_quoted(&chars, i, '\'', &mut out, char_literals)
            }
            c => {
                out.push(c);
                i + 1
            }
        };
    }
    out
}

/// 선택한 출력 방식을 적용하고 줄바꿈은 보존한다.
fn emit(out: &mut String, c: char, fate: Fate) {
    match fate {
        Fate::Blank => out.push(if c == '\n' { '\n' } else { ' ' }),
        Fate::BlankBytes if c == '\n' => out.push('\n'),
        Fate::BlankBytes => out.extend(std::iter::repeat_n(' ', c.len_utf8())),
        Fate::Keep => out.push(c),
        // 문자열 시작으로 오인되지 않는 같은 폭의 문자로 바꾼다.
        Fate::QuoteSafe => out.push(if c == '"' { 'x' } else { c }),
    }
}

fn mask_line_comment(chars: &[char], mut i: usize, out: &mut String, fate: Fate) -> usize {
    while i < chars.len() && chars[i] != '\n' {
        emit(out, chars[i], fate);
        i += 1;
    }
    i
}

fn mask_block_comment(chars: &[char], mut i: usize, out: &mut String, fate: Fate) -> usize {
    let mut depth = 0usize;
    while i < chars.len() {
        let opening = chars[i] == '/' && chars.get(i + 1) == Some(&'*');
        let closing = chars[i] == '*' && chars.get(i + 1) == Some(&'/');
        if opening || closing {
            depth = if opening { depth + 1 } else { depth - 1 };
            emit(out, chars[i], fate);
            emit(out, chars[i + 1], fate);
            i += 2;
            if closing && depth == 0 {
                break;
            }
        } else {
            emit(out, chars[i], fate);
            i += 1;
        }
    }
    i
}

fn mask_raw_string(chars: &[char], i: usize, out: &mut String, fate: Fate) -> usize {
    let (quote, hashes) = raw_string_hashes(chars, i).expect("호출 전에 확인했다");
    // 접두사(`r` / `br` / `#`)는 코드다 — 여는 따옴표부터 덮는다.
    for c in &chars[i..quote] {
        out.push(*c);
    }
    let mut i = quote;
    emit(out, chars[i], fate);
    i += 1;
    while i < chars.len() {
        if chars[i] == '"' && chars[i + 1..].iter().take(hashes).all(|c| *c == '#') {
            for _ in 0..=hashes {
                if i < chars.len() {
                    emit(out, chars[i], fate);
                    i += 1;
                }
            }
            break;
        }
        emit(out, chars[i], fate);
        i += 1;
    }
    i
}

/// 이스케이프를 고려해 닫는 구분자까지 출력 방식을 적용한다. 구분자도 같은 방식으로 처리한다.
fn mask_quoted(
    chars: &[char],
    mut i: usize,
    terminator: char,
    out: &mut String,
    fate: Fate,
) -> usize {
    emit(out, chars[i], fate);
    i += 1;
    while i < chars.len() {
        if chars[i] == '\\' {
            emit(out, chars[i], fate);
            i += 1;
            if i < chars.len() {
                emit(out, chars[i], fate);
                i += 1;
            }
            continue;
        }
        let done = chars[i] == terminator;
        emit(out, chars[i], fate);
        i += 1;
        if done {
            break;
        }
    }
    i
}

/// `i` 가 raw string 접두사(`r"`, `r#"`, `br"`, `br#"` …)의 시작이면 여는 `"` 의
/// 인덱스와 `#` 개수를 돌려준다.
fn raw_string_hashes(chars: &[char], i: usize) -> Option<(usize, usize)> {
    let mut j = i;
    if chars.get(j) == Some(&'b') {
        j += 1;
    }
    if chars.get(j) != Some(&'r') {
        return None;
    }
    j += 1;
    let hash_start = j;
    while chars.get(j) == Some(&'#') {
        j += 1;
    }
    if chars.get(j) == Some(&'"') {
        Some((j, j - hash_start))
    } else {
        None
    }
}

/// `'` 가 문자 리터럴의 시작인지(아니면 라이프타임 틱인지) 가른다.
/// `'\n'` 처럼 이스케이프로 시작하거나, 두 칸 뒤가 닫는 따옴표면 문자 리터럴이다.
fn is_char_literal(chars: &[char], i: usize) -> bool {
    chars.get(i + 1) == Some(&'\\') || chars.get(i + 2) == Some(&'\'')
}

/// 저장소 상대 경로의 구분자를 /로 정규화한다. Windows에서도 같은 문자열로 비교할 수 있다.
pub fn repo_relative(rel: &std::path::Path) -> PathBuf {
    let joined: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    PathBuf::from(joined.join("/"))
}

/// scan_roots 아래의 .rs를 상대 경로와 LF 정규화한 본문으로 모은다.
/// target 디렉터리는 제외하며 읽기 실패는 panic한다.
pub fn rust_sources(root: &std::path::Path, scan_roots: &[&str]) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack: Vec<PathBuf> = scan_roots.iter().map(|r| root.join(r)).collect();
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("스캔 루트를 읽을 수 없다: {} — {e}", dir.display()));
        for entry in entries {
            let entry = entry.expect("디렉터리 항목을 읽을 수 없다");
            let path = entry.path();
            let file_type = entry.file_type().expect("파일 종류를 알 수 없다");
            if file_type.is_dir() {
                if entry.file_name() == "target" {
                    continue;
                }
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let rel = repo_relative(
                    path.strip_prefix(root)
                        .expect("스캔 경로는 레포 안이어야 한다"),
                );
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("소스를 읽을 수 없다: {} — {e}", path.display()));
                out.push((rel, text.replace("\r\n", "\n")));
            }
        }
    }
    out
}

/// 마스킹한 코드에서 name! 호출을 찾는다. 식별자 경계로 println!과 eprintln!을 구분한다.
/// 주석·문자열 제거는 호출자가 먼저 수행해야 한다.
pub fn invokes_macro(code: &str, name: &str) -> bool {
    let mut from = 0;
    while let Some(pos) = code[from..].find(name) {
        let at = from + pos;
        let end = at + name.len();
        let prev = code[..at].chars().next_back();
        let boundary_before = !prev.is_some_and(|c| c.is_alphanumeric() || c == '_');
        if boundary_before && code[end..].starts_with('!') {
            return true;
        }
        from = end;
    }
    false
}

/// 한글·가나·한자 범위의 문자다. 영어 여부를 완전히 판별하는 함수는 아니다.
/// ASCII와 다른 언어 문자가 없다는 사실만으로 영어라고 판단할 수 없다.
pub fn is_locale_specific(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF   // Hangul Jamo
        | 0x3040..=0x30FF // Hiragana · Katakana
        | 0x3130..=0x318F // Hangul Compatibility Jamo
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xAC00..=0xD7A3 // Hangul Syllables
    )
}

#[cfg(test)]
mod repo_relative_tests {
    /// Windows의 구분자 정규화를 확인한다. Linux·macOS의 통과만으로 이 변환을 검증할 수는 없다.
    #[test]
    fn repo_relative_always_yields_forward_slashes() {
        let p = std::path::PathBuf::from("crates")
            .join("tasty-doc-guards")
            .join("src")
            .join("lib.rs");
        assert_eq!(
            super::repo_relative(&p).to_string_lossy(),
            "crates/tasty-doc-guards/src/lib.rs"
        );
    }
}

#[cfg(test)]
mod neutralize_tests {
    use super::{mask_literals, mask_non_code, neutralize_char_literal_quotes};

    #[test]
    fn a_quote_inside_a_char_literal_is_swapped() {
        assert_eq!(
            neutralize_char_literal_quotes("let c = '\"';"),
            "let c = 'x';"
        );
    }

    #[test]
    fn nothing_outside_a_char_literal_moves() {
        for src in [
            "let s = \"따옴표 \\\" 를 담은 문자열\";",
            "// 주석 안의 '\"' 도 그대로다",
            "let r = r#\"raw \"안\" 따옴표\"#;",
            "fn f<'a>(x: &'a str) -> &'a str { x }",
            "let c = 'a'; let n = '\\n'; let q = '\\'';",
        ] {
            assert_eq!(
                neutralize_char_literal_quotes(src),
                src,
                "밖을 건드렸다: {src}"
            );
        }
    }

    #[test]
    fn the_line_count_is_unchanged() {
        let src = "fn a() {\n    let c = '\"';\n}\n\nfn b() {}\n";
        let got = neutralize_char_literal_quotes(src);
        assert_eq!(got.split('\n').count(), src.split('\n').count());
        assert!(got.contains("fn b() {}"), "코드가 사라졌다: {got}");
    }

    #[test]
    fn the_three_modes_keep_their_own_scopes() {
        let src = "let c = '\"'; // 주석";
        assert_eq!(mask_non_code(src), "let c =    ;      ");
        assert_eq!(mask_literals(src), "let c =    ; // 주석");
        assert_eq!(neutralize_char_literal_quotes(src), "let c = 'x'; // 주석");
    }
}
