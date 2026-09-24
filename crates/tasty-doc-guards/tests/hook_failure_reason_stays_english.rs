//! CLI가 직접 만드는 훅 실패 진단에 사용자 번역문이 섞이는지 확인한다.
//! 표시용 stderr는 번역할 수 있지만 진단용 영어 원문은 DiagnosticEnglish로 구분한다.
//! new_unchecked와 record 인자에서 번역 호출·번역 래퍼, 일부 비영어 문자 리터럴을 찾는다.
//! ASCII 문구의 언어까지 판별하지 못하며, 변수·프로세스 밖에서 만든 문구의 출처도 완전히 추적하지 못한다.
//! 응답한 서버나 플러그인의 오류 문구는 번역됐을 수 있어 구조화된 code로 구별해야 한다.
//! 규칙은 [i18n 가이드](../../../docs/dev-guide/i18n.md)의 하드코딩 허용 예외를 따른다.

use std::path::Path;

/// t 또는 t_로 시작하는 이름의 호출을 번역 함수로 분류한다.
fn contains_translation_call(expr: &str) -> bool {
    let bytes = expr.as_bytes();
    let mut i = 0;
    while let Some(pos) = expr[i..].find('t') {
        let at = i + pos;
        let prev_ok = at == 0 || {
            let p = bytes[at - 1];
            !(p.is_ascii_alphanumeric() || p == b'_')
        };
        if prev_ok {
            let rest = &expr[at..];
            let name_len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(rest.len());
            let name = &rest[..name_len];
            let is_t_family = name == "t" || name.starts_with("t_");
            if is_t_family && rest[name_len..].trim_start().starts_with('(') {
                return true;
            }
        }
        i = at + 1;
    }
    false
}

/// 본문에 번역 호출이 있는 함수를 수집해 직접 번역 호출뿐 아니라 래퍼 호출도 찾는다.
/// 함수 결과가 실제로 번역문인지의 데이터 흐름은 분석하지 않는 보수적인 판정이다.
fn translation_wrappers(files: &[std::path::PathBuf]) -> std::collections::BTreeSet<String> {
    let mut names = std::collections::BTreeSet::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let mut from = 0;
        while let Some(pos) = text[from..].find("fn ") {
            let at = from + pos;
            from = at + 3;
            let prev_ok = at == 0 || {
                let p = text.as_bytes()[at - 1];
                !(p.is_ascii_alphanumeric() || p == b'_')
            };
            if !prev_ok {
                continue;
            }
            let rest = &text[at + 3..];
            let name_len = rest
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(0);
            if name_len == 0 {
                continue;
            }
            let name = &rest[..name_len];
            let Some(body_open) = rest.find('{') else {
                continue;
            };
            let mut depth = 0usize;
            let mut body_end = None;
            for (offset, ch) in rest[body_open..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            body_end = Some(body_open + offset);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let Some(end) = body_end else { continue };
            if contains_translation_call(&rest[body_open..end]) {
                names.insert(name.to_string());
            }
        }
    }
    names
}

fn taints_with_translation(expr: &str, wrappers: &std::collections::BTreeSet<String>) -> bool {
    if contains_translation_call(expr) {
        return true;
    }
    wrappers.iter().any(|name| {
        let needle = format!("{name}(");
        expr.match_indices(&needle).any(|(at, _)| {
            at == 0 || {
                let p = expr.as_bytes()[at - 1];
                !(p.is_ascii_alphanumeric() || p == b'_')
            }
        })
    })
}

/// 텍스트의 괄호 깊이로 인자 범위를 찾는다.
fn call_args(src: &str, call_start: usize) -> String {
    let open = match src[call_start..].find('(') {
        Some(o) => call_start + o,
        None => return String::new(),
    };
    let mut depth = 0usize;
    for (offset, ch) in src[open..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return src[open + 1..open + offset].to_string();
                }
            }
            _ => {}
        }
    }
    String::new()
}

fn rust_sources(root: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        panic!("스캔 대상 디렉토리를 열 수 없다: {}", root.display());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// CLI와 본체 소스를 수집하고 누락 여부를 확인한다.
fn sources() -> Vec<std::path::PathBuf> {
    let root = tasty_doc_guards::repo_root();
    let mut files = Vec::new();
    for rel in ["crates/tasty-cli/src", "src"] {
        let dir = root.join(rel);
        assert!(dir.is_dir(), "스캔 루트가 사라졌다: {}", dir.display());
        rust_sources(&dir, &mut files);
    }
    assert!(
        files.len() > 50,
        "스캔 결과가 비정상적으로 적다({}) — 루트가 옮겨졌는지 확인해라",
        files.len()
    );
    files
}

#[test]
fn diagnostic_reasons_are_never_built_from_translations() {
    let files = sources();
    let wrappers = translation_wrappers(&files);
    assert!(
        wrappers.contains("localize"),
        "번역 래퍼 수집에서 port_file::localize를 찾지 못했다. 수집 범위와 함수 판독을 확인한다."
    );
    let mut violations = Vec::new();
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let mut from = 0;
        while let Some(pos) = text[from..].find("new_unchecked") {
            let at = from + pos;
            let args = call_args(&text, at);
            if taints_with_translation(&args, &wrappers) {
                let line = text[..at].matches('\n').count() + 1;
                violations.push(format!("{}:{line}  new_unchecked({args})", path.display()));
            }
            from = at + "new_unchecked".len();
        }
    }
    assert!(
        violations.is_empty(),
        "CLI가 직접 만드는 훅 실패 reason에 번역 호출이 있다. 사용자 문구는 stderr로 보내고 진단에는 영어 원문을 사용한다:\n{}",
        violations.join("\n")
    );
}

/// 번역 함수를 거치지 않고 직접 쓴 일부 비영어 문자도 검사한다.
/// 변수의 런타임 내용은 추적하지 않고 is_locale_specific과 mask_literals로 리터럴 문자만 확인한다.
/// 주석의 한국어는 이 검사의 대상이 아니다.
#[test]
fn diagnostic_reasons_never_contain_locale_specific_literals() {
    use tasty_doc_guards::source_text::{is_locale_specific, mask_literals};

    let files = sources();
    let mut violations = Vec::new();
    let mut scanned_calls = 0;
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let mut from = 0;
        while let Some(pos) = text[from..].find("new_unchecked") {
            let at = from + pos;
            from = at + "new_unchecked".len();
            let args = call_args(&text, at);
            if args.is_empty() {
                continue;
            }
            scanned_calls += 1;
            // 마스킹은 문자 수를 유지하지만 바이트 수는 달라질 수 있어 char 단위로 대조한다.
            let masked = mask_literals(&args);
            let inside_literal = args
                .chars()
                .zip(masked.chars())
                .any(|(orig, m)| is_locale_specific(orig) && m == ' ');
            if inside_literal {
                let line = text[..at].matches('\n').count() + 1;
                violations.push(format!("{}:{line}  new_unchecked({args})", path.display()));
            }
        }
    }
    assert!(
        scanned_calls > 0,
        "new_unchecked 호출을 찾지 못했다. 이름과 수집 범위를 확인한다."
    );
    assert!(
        violations.is_empty(),
        "진단 생성 인자의 리터럴에 언어별 문자를 찾았다. 직접 작성하는 진단은 영어 원문을 사용하고 번역문은 사용자 출력으로 보낸다:\n{}",
        violations.join("\n")
    );
}

/// record의 타입 계약 외에도 직접 번역 호출이 들어가는 형태를 검사한다.
#[test]
fn record_is_never_called_with_a_translated_reason() {
    let files = sources();
    let wrappers = translation_wrappers(&files);
    let mut violations = Vec::new();
    let mut record_calls = 0;
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let mut from = 0;
        while let Some(pos) = text[from..].find("hook_failure::record") {
            let at = from + pos;
            record_calls += 1;
            let args = call_args(&text, at);
            if taints_with_translation(&args, &wrappers) {
                let line = text[..at].matches('\n').count() + 1;
                violations.push(format!("{}:{line}  record({args})", path.display()));
            }
            from = at + "hook_failure::record".len();
        }
    }
    assert!(
        record_calls > 0,
        "hook_failure::record 호출을 찾지 못했다. 모듈 이동과 수집 범위를 확인한다."
    );
    assert!(
        violations.is_empty(),
        "record 의 reason 은 로케일 무관 영어다:\n{}",
        violations.join("\n")
    );
}

#[test]
fn the_translation_call_detector_recognises_the_forms_it_must_catch() {
    assert!(contains_translation_call(r#"&t("cli.port_file.invalid")"#));
    assert!(contains_translation_call(r#"&t_fmt("k", &p)"#));
    assert!(contains_translation_call(
        r#"tasty_i18n::t_fmt2("k", &a, &b)"#
    ));
    assert!(!contains_translation_call("format!(\"{port}\")"));
    assert!(!contains_translation_call("e.to_string()"));
    assert!(!contains_translation_call("self.source.to_string()"));
}
