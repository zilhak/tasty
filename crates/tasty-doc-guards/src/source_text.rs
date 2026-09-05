//! 소스를 **텍스트로 읽는** 가드들이 공유하는 두 가지: 코드가 아닌 부분을 덮는 것과,
//! 스캔 루트 아래의 `.rs` 를 모으는 것.
//!
//! 여기 있는 이유는 소비자가 **다른 컴파일 단위**에 흩어져 있기 때문이다 — 본체의
//! `src/source_guards/`(단위 테스트)와 루트 `tests/`(통합 타깃)는 서로의 비공개
//! 항목을 못 본다. 각자 사본을 두면 같은 물음에 답이 둘이 되고, 갈린 쪽은 조용하다.

use std::path::PathBuf;

/// 주석·문자열·문자 리터럴을 공백으로 덮은 사본을 만든다. 줄바꿈은 그대로 두므로
/// 결과 문자열의 줄 번호는 원본과 같다. 라이프타임 틱(`'a`)은 문자 리터럴과 구분한다.
pub fn mask_non_code(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    while i < chars.len() {
        i = match chars[i] {
            '/' if chars.get(i + 1) == Some(&'/') => mask_line_comment(&chars, i, &mut out),
            '/' if chars.get(i + 1) == Some(&'*') => mask_block_comment(&chars, i, &mut out),
            'r' | 'b' if raw_string_hashes(&chars, i).is_some() => {
                mask_raw_string(&chars, i, &mut out)
            }
            '"' => mask_quoted(&chars, i, '"', &mut out),
            '\'' if is_char_literal(&chars, i) => mask_quoted(&chars, i, '\'', &mut out),
            c => {
                out.push(c);
                i + 1
            }
        };
    }
    out
}

/// 코드가 아닌 한 글자를 공백으로 덮는다 — 줄바꿈만 그대로 둬서 줄 번호를 지킨다.
fn blank(out: &mut String, c: char) {
    out.push(if c == '\n' { '\n' } else { ' ' });
}

fn mask_line_comment(chars: &[char], mut i: usize, out: &mut String) -> usize {
    while i < chars.len() && chars[i] != '\n' {
        blank(out, chars[i]);
        i += 1;
    }
    i
}

fn mask_block_comment(chars: &[char], mut i: usize, out: &mut String) -> usize {
    let mut depth = 0usize;
    while i < chars.len() {
        let opening = chars[i] == '/' && chars.get(i + 1) == Some(&'*');
        let closing = chars[i] == '*' && chars.get(i + 1) == Some(&'/');
        if opening || closing {
            depth = if opening { depth + 1 } else { depth - 1 };
            blank(out, chars[i]);
            blank(out, chars[i + 1]);
            i += 2;
            if closing && depth == 0 {
                break;
            }
        } else {
            blank(out, chars[i]);
            i += 1;
        }
    }
    i
}

fn mask_raw_string(chars: &[char], i: usize, out: &mut String) -> usize {
    let (quote, hashes) = raw_string_hashes(chars, i).expect("호출 전에 확인했다");
    // 접두사(`r` / `br` / `#`)는 코드다 — 여는 따옴표부터 덮는다.
    for c in &chars[i..quote] {
        out.push(*c);
    }
    let mut i = quote;
    blank(out, chars[i]);
    i += 1;
    while i < chars.len() {
        if chars[i] == '"' && chars[i + 1..].iter().take(hashes).all(|c| *c == '#') {
            for _ in 0..=hashes {
                if i < chars.len() {
                    blank(out, chars[i]);
                    i += 1;
                }
            }
            break;
        }
        blank(out, chars[i]);
        i += 1;
    }
    i
}

/// `terminator` 로 닫히는 리터럴(문자열·문자)을 덮는다. 역슬래시 이스케이프를 따른다.
fn mask_quoted(chars: &[char], mut i: usize, terminator: char, out: &mut String) -> usize {
    blank(out, chars[i]);
    i += 1;
    while i < chars.len() {
        if chars[i] == '\\' {
            blank(out, chars[i]);
            i += 1;
            if i < chars.len() {
                blank(out, chars[i]);
                i += 1;
            }
            continue;
        }
        let done = chars[i] == terminator;
        blank(out, chars[i]);
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

/// 스캔 루트 아래의 모든 `.rs` 를 (레포 상대 경로, LF 정규화된 내용)으로 모은다.
/// 빌드 산출물(`target/`)은 루트 밑에 없지만, 크레이트별 `target/` 이 생길 수 있어
/// 이름으로 한 번 더 뺀다.
///
/// **읽기 실패는 panic 이다.** 스캔 가드에서 조용히 건너뛰면 모수가 줄고, 줄어든 모수는
/// 언제나 초록이다.
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
                let rel = path
                    .strip_prefix(root)
                    .expect("스캔 경로는 레포 안이어야 한다")
                    .to_path_buf();
                let text = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("소스를 읽을 수 없다: {} — {e}", path.display()));
                out.push((rel, text.replace("\r\n", "\n")));
            }
        }
    }
    out
}
