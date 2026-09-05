//! Rust 소스의 **어휘 마스킹** — 주석·문자열·문자 리터럴을 공백으로 덮어, 텍스트를
//! 훑는 가드가 "코드가 아닌 것" 을 코드로 세지 않게 한다. 줄 수와 열 위치는 보존한다.
//!
//! 이 모듈이 있는 이유는 같은 마스커가 레포에 여러 벌 적혀 있었기 때문이다. 통합
//! 테스트끼리는 서로를 import 할 수 없어(각자 독립 바이너리) `mod` 로 함께 쓴다 —
//! `cfg_span` 과 같은 형태다. 사본이 둘이면 갈리고, 갈린 쪽은 조용하다.
//!
//! **두 함수가 있는 이유**: 판정이 두 종류다.
//!
//! - "여기 코드에 X 가 있나" → [`mask_non_code`]. 주석도 문자열도 코드가 아니다.
//! - "여기 **주석**이 달려 있나" → [`mask_literals`]. 문자열만 지우고 주석은 남긴다.
//!   문자열 속 `//`(URL 이 대표적)를 주석으로 오인하는 것을 막는 유일한 방법이다.
//!
//! 하나로 합칠 수 없다 — 주석을 지우면 주석의 유무를 물을 수 없고, 안 지우면 주석 속
//! 코드 형태가 코드로 세어진다. 두 물음은 서로의 답을 지운다.
#![allow(dead_code)]

/// 주석 · 문자열 · 문자 리터럴을 전부 공백으로. **줄 구조는 보존한다.**
pub fn mask_non_code(src: &str) -> String {
    mask(src, true)
}

/// 문자열 · 문자 리터럴만 공백으로 — **주석은 원문 그대로 남긴다.**
///
/// 이 결과에 `//` 가 있으면 그것은 진짜 주석이다. 원문에는 있는데 여기 없으면
/// 그 `//` 는 문자열 안에 있었다는 뜻이다.
pub fn mask_literals(src: &str) -> String {
    mask(src, false)
}

fn mask(src: &str, blank_comments: bool) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0usize;
    let keep = |ch: char, out: &mut String| out.push(if ch == '\n' { '\n' } else { ' ' });
    // 주석 구간의 한 글자 — 지울지 남길지가 호출자의 선택이다.
    let com = |ch: char, out: &mut String| {
        if blank_comments {
            keep(ch, out);
        } else {
            out.push(ch);
        }
    };

    while i < c.len() {
        match (c[i], c.get(i + 1)) {
            ('/', Some('/')) => {
                while i < c.len() && c[i] != '\n' {
                    com(c[i], &mut out);
                    i += 1;
                }
            }
            ('/', Some('*')) => {
                let mut depth = 1usize;
                com(c[i], &mut out);
                com(c[i + 1], &mut out);
                i += 2;
                while i < c.len() && depth > 0 {
                    if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                        depth += 1;
                        com(c[i], &mut out);
                        i += 1;
                    } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                        depth -= 1;
                        com(c[i], &mut out);
                        i += 1;
                    }
                    if i < c.len() {
                        com(c[i], &mut out);
                        i += 1;
                    }
                }
            }
            ('r', _) | ('b', _) if raw_string_at(&c, i).is_some() => {
                let hashes = raw_string_at(&c, i).expect("바로 위에서 확인했다");
                // `r` (또는 `br`) + `#`*n + `"` 까지 지우고, 닫는 `"` + `#`*n 을 찾는다.
                let open_len = if c[i] == 'b' { 2 } else { 1 } + hashes + 1;
                for _ in 0..open_len {
                    keep(c[i], &mut out);
                    i += 1;
                }
                while i < c.len() {
                    if c[i] == '"' && (1..=hashes).all(|k| c.get(i + k) == Some(&'#')) {
                        for _ in 0..=hashes {
                            keep(c[i], &mut out);
                            i += 1;
                        }
                        break;
                    }
                    keep(c[i], &mut out);
                    i += 1;
                }
            }
            ('\'', _) if char_literal_len(&c, i).is_some() => {
                let n = char_literal_len(&c, i).expect("바로 위에서 확인했다");
                for _ in 0..n {
                    keep(c[i], &mut out);
                    i += 1;
                }
            }
            ('"', _) => {
                keep(c[i], &mut out);
                i += 1;
                while i < c.len() {
                    if c[i] == '\\' {
                        keep(c[i], &mut out);
                        i += 1;
                        if i < c.len() {
                            keep(c[i], &mut out);
                            i += 1;
                        }
                        continue;
                    }
                    let done = c[i] == '"';
                    keep(c[i], &mut out);
                    i += 1;
                    if done {
                        break;
                    }
                }
            }
            (ch, _) => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

/// `i` 에서 char 리터럴이 시작하면 그 길이를 준다. 라이프타임(`'a`)은 닫는 따옴표가 없어
/// `None` 이다.
///
/// **이게 없으면 `'"'` 같은 리터럴이 문자열 모드를 열어 그 뒤가 통째로 어긋난다.**
fn char_literal_len(c: &[char], i: usize) -> Option<usize> {
    if c.get(i) != Some(&'\'') {
        return None;
    }
    if c.get(i + 1) == Some(&'\\') {
        // `'\n'` · `'\''` · `'\u{1F600}'` — 닫는 따옴표를 짧은 창 안에서 찾는다.
        return (2..12).find(|k| c.get(i + k) == Some(&'\'')).map(|k| k + 1);
    }
    (c.get(i + 2) == Some(&'\'')).then_some(3)
}

/// `i` 에서 raw string 이 시작하면 `#` 의 개수를 준다. `r"..."` 는 0, `r#"..."#` 는 1.
/// `b` 접두(byte string)도 같이 본다.
fn raw_string_at(c: &[char], i: usize) -> Option<usize> {
    let mut j = i;
    if c.get(j) == Some(&'b') {
        j += 1;
    }
    if c.get(j) != Some(&'r') {
        return None;
    }
    // `r` 앞이 식별자 문자면 `r` 은 이름의 일부다 (`for` 의 `r` 등).
    if i > 0 && (c[i - 1].is_ascii_alphanumeric() || c[i - 1] == '_') {
        return None;
    }
    j += 1;
    let mut hashes = 0usize;
    while c.get(j) == Some(&'#') {
        hashes += 1;
        j += 1;
    }
    (c.get(j) == Some(&'"')).then_some(hashes)
}
