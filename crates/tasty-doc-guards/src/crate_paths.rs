//! 마스킹한 코드에서 crate/super/외부 크레이트 경로와 중괄호 import를 읽는다.
//! 주석·리터럴은 호출 전에 source_text::mask_non_code로 제거한다.

use crate::cfg_predicate::cfg_gated_lines;
use crate::source_text::mask_non_code;

/// 파일 경로로 계산한 모듈 깊이 — 크레이트 루트 바로 아래 모듈이 1.
/// `src/core/mod.rs` → 1 · `src/core/attach.rs` → 2 · `src/core/agent/mod.rs` → 2 ·
/// `src/core/agent/task.rs` → 3.
pub fn module_depth(rel: &str) -> usize {
    let parts: Vec<&str> = rel.trim_start_matches("src/").split('/').collect();
    let last = parts.last().copied().unwrap_or("");
    if last == "mod.rs" {
        parts.len() - 1
    } else {
        parts.len()
    }
}

fn is_ident_byte(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// `i` 에서 공백을 건너 `::` 가 오면 그 뒤(공백까지 건넌) 위치.
fn eat_path_sep(b: &[u8], i: usize) -> Option<usize> {
    let j = skip_ws(b, i);
    b[j..].starts_with(b"::").then(|| skip_ws(b, j + 2))
}

/// `i` 에서 시작하는 식별자의 끝. 식별자가 아니면 `None`.
fn ident_end(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    while j < b.len() && is_ident_byte(b[j]) {
        j += 1;
    }
    (j > i).then_some(j)
}

/// 경로와 중괄호 import를 각각의 경로 목록으로 편다.
/// 항목 시작 오프셋과 경로를 out에 넣고 읽은 끝을 반환한다. 공백·줄바꿈을 허용한다.
pub fn expand_tree(
    code: &str,
    mut i: usize,
    prefix: &[&str],
    out: &mut Vec<(usize, Vec<String>)>,
) -> usize {
    let b = code.as_bytes();
    i = skip_ws(b, i);
    if b.get(i) == Some(&b'{') {
        i += 1;
        loop {
            i = skip_ws(b, i);
            match b.get(i) {
                None => return i,
                Some(b'}') => return i + 1,
                Some(b',') => i += 1,
                Some(_) => {
                    let next = expand_tree(code, i, prefix, out);
                    if next == i {
                        // 읽을 수 없는 항목(`*` 등) — 한 바이트 건너 무한 반복을 막는다.
                        i += 1;
                    } else {
                        i = next;
                    }
                }
            }
        }
    }
    let Some(end) = ident_end(b, i) else {
        return i;
    };
    let mut path: Vec<&str> = prefix.to_vec();
    path.push(&code[i..end]);
    match eat_path_sep(b, end) {
        Some(j) if b.get(j) == Some(&b'{') || ident_end(b, j).is_some() => {
            let before = out.len();
            let after = expand_tree(code, j, &path, out);
            // 하위 항목의 오프셋은 그 항목 자리다. 한 줄짜리 경로는 첫 마디 자리로 맞춘다.
            if b.get(j) != Some(&b'{') {
                for p in &mut out[before..] {
                    p.0 = p.0.min(i);
                }
            }
            after
        }
        _ => {
            out.push((i, path.iter().map(|s| s.to_string()).collect()));
            end
        }
    }
}

/// 크레이트 루트에서 시작하는 경로들 — `crate::…`(`$crate::…` 포함)과, 모듈 깊이와 같은
/// 길이의 `super::` 사슬. 중괄호 import 는 항목마다, 줄을 넘는 경로는 이어서 읽는다.
pub fn root_paths(code: &str, depth: usize) -> Vec<(usize, Vec<String>)> {
    let b = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let boundary = i == 0 || !is_ident_byte(b[i - 1]);
        if !boundary || !is_ident_byte(b[i]) {
            i += 1;
            continue;
        }
        let end = ident_end(b, i).unwrap_or(i + 1);
        let word = &code[i..end];
        if word == "crate" {
            if let Some(j) = eat_path_sep(b, end) {
                expand_tree(code, j, &[], &mut out);
            }
        } else if word == "super" {
            // 사슬의 머리만 센다 — 앞에 `::` 가 붙은 `super` 는 사슬의 중간이다.
            let before = code[..i].trim_end();
            if !before.ends_with("::") {
                let mut k = 1;
                let mut j = end;
                while let Some(n) = eat_path_sep(b, j) {
                    match ident_end(b, n) {
                        Some(e) if &code[n..e] == "super" => {
                            k += 1;
                            j = e;
                        }
                        _ => {
                            j = n;
                            break;
                        }
                    }
                }
                if k == depth && j > end {
                    expand_tree(code, j, &[], &mut out);
                }
            }
        }
        i = end;
    }
    out
}

/// 표의 항목(`file::dispatch`)이 경로(크레이트 루트부터의 마디들)의 **앞마디와 통째로**
/// 같은가. `file::dispatch` 는 `file::dispatch::X` 를 잡고 `file::format::X` 는 안 잡는다.
pub fn path_is_under(entry: &str, path: &[String]) -> bool {
    let segs: Vec<&str> = entry.split("::").collect();
    segs.len() <= path.len() && segs.iter().zip(path).all(|(a, b)| *a == b)
}

/// 한 파일의 test 조건으로 제외되지 않은 코드가 부르는 크레이트 루트 항목. `(1-기준 줄번호, 이름, 그 줄 원문)`.
///
/// `classify` 가 경로를 받아 표의 이름을 돌려주면 그 자리가 잡힌다. 경로를 파일 전체에서
/// 읽고(줄을 넘는 경로 · 여러 줄 중괄호 import) 줄 번호는 경로 항목이 시작한 오프셋으로
/// 환산한다. 인라인 `#[cfg(test)]` 줄은 뺀다. 한 줄에 같은 항목이 여러 번이면 한 자리로 센다.
pub fn shipped_references<'a>(
    rel: &str,
    text: &str,
    classify: impl Fn(&[String]) -> Option<&'a str>,
) -> Vec<(usize, &'a str, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let gated = cfg_gated_lines(&lines, "test");
    let masked = mask_non_code(text);
    let mut out: Vec<(usize, &'a str, String)> = Vec::new();
    for (off, path) in root_paths(&masked, module_depth(rel)) {
        let line = masked[..off].matches('\n').count();
        if gated.get(line).copied().unwrap_or(false) {
            continue;
        }
        let Some(name) = classify(&path) else {
            continue;
        };
        if out.iter().any(|(l, n, _)| *l == line + 1 && *n == name) {
            continue;
        }
        let raw = lines.get(line).map(|l| l.trim()).unwrap_or("");
        out.push((line + 1, name, raw.to_string()));
    }
    out
}

/// **외부 크레이트**에서 시작하는 경로들 — `roots` 의 이름이 경로의 첫 마디인 자리.
///
/// 잡는 형태: `egui::Context` · `::egui::Context`(절대 경로) · `use egui::{A, b::C}`(여러 줄
/// 중괄호 포함 — 항목마다 편다) · `use {egui::A, winit::B}` · `use egui as e;` · `use egui;` ·
/// `extern crate egui;`. 마디가 하나뿐인 형태(`as` 별칭 · `use egui;`)는 경로 `[egui]` 로 싣는다.
///
/// 안 잡는 것: 앞에 다른 마디가 붙은 경로(`crate::image::x` · `self::png` · `foo::egui`)와
/// 필드·메서드(`.egui`). 같은 이름의 **지역 모듈**을 `use` 없이 `image::x` 로 부르는 것은
/// 텍스트로 크레이트와 안 갈린다 — 그런 자리가 생기면 소비자의 목록에 오른다.
pub fn external_paths(code: &str, roots: &[&str]) -> Vec<(usize, Vec<String>)> {
    let b = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let boundary = i == 0 || !is_ident_byte(b[i - 1]);
        if !boundary || !is_ident_byte(b[i]) {
            i += 1;
            continue;
        }
        let end = ident_end(b, i).unwrap_or(i + 1);
        let word = &code[i..end];
        if roots.contains(&word) && starts_a_path(code, i) {
            if eat_path_sep(b, end).is_some() {
                expand_tree(code, i, &[], &mut out);
            } else if names_the_crate_alone(code, i, end) {
                out.push((i, vec![word.to_string()]));
            }
        }
        i = end;
    }
    out
}

/// `i` 의 식별자가 경로의 **첫 마디**인가 — 앞이 `.` 이 아니고, 앞이 `::` 이면 그 앞에 마디가
/// 없다(`::egui` 는 절대 경로의 첫 마디, `crate::egui` 는 아니다). `::` 앞의 낱말이
/// 키워드(`use ::egui` · `-> impl ::egui::X`)면 마디가 아니다.
fn starts_a_path(code: &str, i: usize) -> bool {
    let before = code[..i].trim_end();
    if before.ends_with('.') {
        return false;
    }
    let Some(head) = before.strip_suffix("::") else {
        return true;
    };
    let head = head.trim_end();
    let hb = head.as_bytes();
    match hb.last() {
        // `-> ::egui::X` 는 반환 타입의 절대 경로, `<T as X>::egui` 는 한정 경로의 마디다.
        Some(b'>') => head.ends_with("->"),
        Some(c) if is_ident_byte(*c) => {
            let mut s = hb.len();
            while s > 0 && is_ident_byte(hb[s - 1]) {
                s -= 1;
            }
            PATH_PRECEDING_KEYWORDS.contains(&&head[s..])
        }
        _ => true,
    }
}

/// `::` 바로 앞에 와도 경로 마디가 아닌 키워드 — 그 뒤의 `::x` 는 절대 경로다.
const PATH_PRECEDING_KEYWORDS: &[&str] = &[
    "use", "as", "dyn", "impl", "return", "in", "mut", "ref", "where", "let", "move", "const",
    "static", "type", "else", "match", "if", "while", "for", "pub", "break",
];

/// 뒤에 `::` 가 없는 크레이트 이름이 **크레이트를 부르는** 자리인가 — `use egui;` ·
/// `use egui as e;` · `extern crate egui;` · 중괄호 안의 `egui as e`. 중괄호 안의 맨 이름
/// (`{ image, png }`)은 구조체 필드 축약과 텍스트로 안 갈려 안 잡는다.
fn names_the_crate_alone(code: &str, i: usize, end: usize) -> bool {
    let before = code[..i].trim_end();
    let prev_word_is = |w: &str| {
        before.ends_with(w)
            && before.as_bytes()[..before.len() - w.len()]
                .last()
                .is_none_or(|c| !is_ident_byte(*c))
    };
    if prev_word_is("use") || prev_word_is("crate") {
        return true;
    }
    let b = code.as_bytes();
    let j = skip_ws(b, end);
    let followed_by_as = code[j..].starts_with("as") && ident_end(b, j) == Some(j + 2);
    followed_by_as && (before.ends_with('{') || before.ends_with(','))
}

/// 한 파일의 test 조건으로 제외되지 않은 코드가 부르는 외부 크레이트 경로 — `(1-기준 줄번호, 경로, 그 줄 원문)`.
/// 경로는 `::` 로 이은 전체다(`egui::Context`). 인라인 `#[cfg(test)]` 줄은 빼고, 다른 cfg
/// (`feature = "gui"` 등)는 **안 뺀다** — 다른 빌드 조건에서는 사용될 수 있다. 한 줄의 같은 경로는 한 자리다.
pub fn shipped_external_references(text: &str, roots: &[&str]) -> Vec<(usize, String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let gated = cfg_gated_lines(&lines, "test");
    let masked = mask_non_code(text);
    let mut out: Vec<(usize, String, String)> = Vec::new();
    for (off, path) in external_paths(&masked, roots) {
        let line = masked[..off].matches('\n').count();
        if gated.get(line).copied().unwrap_or(false) {
            continue;
        }
        let joined = path.join("::");
        if out.iter().any(|(l, p, _)| *l == line + 1 && *p == joined) {
            continue;
        }
        let raw = lines.get(line).map(|l| l.trim()).unwrap_or("");
        out.push((line + 1, joined, raw.to_string()));
    }
    out
}
