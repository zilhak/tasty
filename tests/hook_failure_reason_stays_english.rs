//! `hook-failures.log` 의 `reason` 이 사용자 로케일을 타지 않도록 막는 소스 가드.
//!
//! **왜 이 파일이 필요한가.** 진입점은 `run_dynamic_client` 하나뿐이라 지점은 좁지만,
//! 좁다는 이유로 규약이 지켜지지는 않았다 — 서로 모르는 두 변경이 각각 그 자리에
//! 번역문을 흘려 넣었고, 둘 다 자기 lane 안에서는 옳았다. 사용자 표시 문구와 진단
//! 산출물이 **같은 값에서 갈라져 나오는 구조**라 한쪽을 고치면 다른 쪽이 따라 움직인다.
//!
//! 1 차 방어선은 타입이다: `hook_failure::record` 는 `&DiagnosticEnglish` 만 받으므로
//! `t()` 결과를 그냥 넘길 수 없다. 이 파일은 그 타입의 **탈출구**를 막는다 —
//! `DiagnosticEnglish::new_unchecked` 는 호출자의 보증에 기대는 생성자라, 그 인자에
//! 번역 호출이 들어가면 타입은 통과시키고 규약만 깨진다.
//!
//! ## 한 줄에 실리는 값은 세 종류이고, 지키는 수단이 각각 다르다
//!
//! `hook-failures.log` 한 줄에는 성질이 다른 값이 섞여 있다. 하나의 검사로 셋을 다
//! 지킬 수 없어서 수단을 갈라 둔다.
//!
//! - **기계가 파싱하는 좌표** — `method=` · `event=` · `code=` · `surface=`. 프로토콜
//!   값이거나 요청에 이미 있던 식별자라 애초에 문구가 아니다. 지키는 수단은 타입이다
//!   (`code` 는 `Option<i32>`, `event` 는 요청에서 뽑은 토큰). 이 파일이 볼 것이 없다.
//! - **개발자·에이전트가 읽는 진단 산문** — `reason=`. 로케일 무관 영어라는 규약이
//!   붙고, 이 파일의 세 테스트가 그 규약을 본다.
//! - **사용자에게 보이는 메시지** — 로그가 아니라 stderr 로 나가고, **번역문이 맞다**.
//!   그쪽은 `t()` 를 **요구하는** 가드(`no_hardcoded_ui_strings`)가 지킨다. 두 가드는
//!   같은 실패에 대해 정반대를 요구하므로, 어느 산출물로 나가는 값인지를 먼저 갈라야
//!   한다. 그 갈라짐이 `DiagnosticEnglish` 라는 타입의 존재 이유다.
//!
//! ## 술어는 규약보다 좁다 — 좁은 만큼 두 개다
//!
//! 규약은 "로케일 무관 영어" 인데, 그것을 통째로 재는 술어는 없다. 그래서 값이 아니라
//! **출처**를 보는 검사(번역 호출·래퍼가 인자에 섞였나)를 1 번으로 두고, 출처만으로는
//! 못 잡는 형태를 2 번이 값으로 받는다 — `t()` 를 안 거치고 **소스에 직접 박은** 비영어
//! 리터럴이다. 실제로 그 형태는 1 번을 그냥 통과했다(그 측정이 2 번을 만든 계기다).
//!
//! 2 번도 규약 전체는 못 잰다: 스페인어·터키어 번역문은 전부 ASCII 라 값만으로는
//! 영어와 구별되지 않는다. **그것은 이 파일의 결함이 아니라 값 검사의 한계**이고,
//! 그 갈래는 여전히 1 번(출처)이 잡는다. 둘 다 통과하고도 규약을 깨는 경우가 남는다 —
//! `t()` 를 안 거친 스페인어 리터럴 — 는 가드가 아니라 리뷰의 몫이다.
//!
//! ## 이 가드가 덮지 못하는 갈래 — 원리적으로 그렇다
//!
//! 실패 지점 셋 중 **셋째(요청은 닿았는데 오류 응답이 온 경우)의 문구는 CLI 가 만들지
//! 않는다.** 답한 쪽이 자기 프로세스에서 만들어 보낸 것이고, 그 쪽이 plugin 이면 앱
//! 언어를 탄다. 이 파일은 CLI 소스를 스캔하므로 그 자리에는 볼 `t()` 가 없다 — 스캔
//! 범위를 넓히는 문제가 아니라 **문구가 이 프로세스 밖에서 만들어진다**는 문제다.
//!
//! 그래서 그 갈래의 로케일 무관성은 산문이 아니라 `code=` 필드가 진다. 이 가드가
//! 지키는 것은 **CLI 가 영어 원본을 쥐고 있는 두 갈래**(포트 파일 부재 · connect 실패)이고,
//! 거기서는 표시용 번역문과 진단용 영어를 실제로 갈라 놓을 수 있다.
//!
//! 왜 `hook-failures.log` 만 이런 대접을 받는가: 셸 래퍼가 `|| true` 로 exit code 를
//! 버리기 때문에 hook 전달 실패는 **이 파일 말고 흔적이 없다**. 읽는 주체가 에이전트일
//! 수 있으니 알려진 실패 패턴과 대조 가능해야 하고, 그러려면 문구가 흔들리면 안 된다.
//! 배경은 [`docs/dev-guide/i18n.md`] "하드코딩 허용 예외".
//!
//! [`docs/dev-guide/i18n.md`]: ../docs/dev-guide/i18n.md

use std::path::Path;

/// 번역 호출로 볼 형태. `t(` / `t_fmt(` / `t_fmt2(` / `t_ns(` 등 접두어가 `t` 인 것 전부.
fn contains_translation_call(expr: &str) -> bool {
    let bytes = expr.as_bytes();
    let mut i = 0;
    while let Some(pos) = expr[i..].find('t') {
        let at = i + pos;
        // 식별자 중간의 `t` 는 건너뛴다(`format!`, `port` 등).
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

/// 본문에 번역 호출이 있는 함수 이름들 — **번역 래퍼**.
///
/// `t(` 만 찾으면 한 겹만 감싸도 뚫린다. 실제로 변이 검증에서
/// `new_unchecked(port_file::localize(&e))` 가 통과했다 — `localize` 는 `t()` 를
/// 부르지만 호출 지점에는 `t` 가 안 보인다. 그래서 "무엇이 번역문을 만드는가" 를
/// 소스에서 먼저 모으고, 그 이름들도 번역 호출과 같이 취급한다.
///
/// 보수적으로 잡는 쪽을 택한다: 번역을 조금이라도 하는 함수의 결과는 진단 로그에
/// 실릴 값이 아니다. 진단문이 필요하면 그 함수 말고 영어 원본에서 만든다.
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
            // 본문 = 시그니처 뒤 첫 `{` 부터 균형 잡힌 닫는 `}` 까지.
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

/// 인자 식에 번역 호출이나 [`translation_wrappers`] 호출이 있는가.
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

/// `needle(` 뒤의 균형 잡힌 인자 목록을 잘라낸다.
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

/// 스캔 루트. 비어 있으면 **크게 실패**한다 — 가드의 조용한 미스캔은 위양성보다 나쁘다.
fn sources() -> Vec<std::path::PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
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

/// `DiagnosticEnglish::new_unchecked(...)` 의 인자에 번역 호출이 있으면 실패.
#[test]
fn diagnostic_reasons_are_never_built_from_translations() {
    let files = sources();
    let wrappers = translation_wrappers(&files);
    assert!(
        wrappers.contains("localize"),
        "번역 래퍼 수집이 깨졌다 — `port_file::localize` 조차 못 찾았다면 스캔이 헛돈 것이다"
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
        "`hook-failures.log` 의 reason 은 로케일 무관 영어다. 번역문을 실으려면 그 문구를 \
         stderr 로 내고, 로그에는 영어 원본을 넘겨라:\n{}",
        violations.join("\n")
    );
}

/// `DiagnosticEnglish::new_unchecked(...)` 의 인자에 **로케일 고정 문자를 담은 리터럴**이
/// 있으면 실패.
///
/// 1 번(`diagnostic_reasons_are_never_built_from_translations`)은 `t()` 를 **거쳐서** 들어온
/// 번역문을 잡는다. 이 테스트는 그것을 **안 거치고** 소스에 직접 박힌 비영어 문구를 잡는다 —
/// 실측으로 그 형태가 1 번을 통과했다: `new_unchecked(format!("알 수 없는 오류: {}", msg))` 를
/// 심어도 이 파일도, `no_hardcoded_ui_strings` 도, 워크스페이스 전체 테스트도 초록이었다.
///
/// **런타임 값은 대상이 아니다.** `e.to_string()` · `msg.clone()` 처럼 답한 쪽이 만들어 보낸
/// 문구는 CLI 가 언어를 고를 수 없는 값이라 애초에 보증의 대상이 아니고(위 모듈 doc), 여기서도
/// 리터럴만 보므로 걸리지 않는다. 즉 이 검사가 덮는 것은 **CLI 가 자기 소스에서 문구를 만드는
/// 갈래** 하나다.
///
/// 판정은 두 공유 도구를 그대로 쓴다 — 문자 술어는 `is_locale_specific`, "리터럴 안인가" 는
/// `mask_literals` 다. 후자는 리터럴만 공백으로 덮고 주석은 남기므로, **원문이 비영어인데 덮인
/// 자리** = 리터럴 안이다. 주석에 한글을 쓰는 것은 이 레포의 정상이라 그쪽은 걸리면 안 된다.
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
            // 두 사본은 **글자 수**가 같다(덮기는 한 글자를 한 칸으로 바꾼다). 바이트 수는
            // 다르므로 반드시 char 단위로 짝지어 본다.
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
        "`new_unchecked` 호출을 하나도 못 찾았다 — 이름이 바뀌었으면 이 가드는 조용히 죽는다"
    );
    assert!(
        violations.is_empty(),
        "`hook-failures.log` 의 reason 은 로케일 무관 영어다. 소스에 박은 비영어 문구는 \
         진단 채널이 아니라 사용자 표면으로 보내라 — 로그에는 영어 원본을 넘긴다:\n{}",
        violations.join("\n")
    );
}

/// `hook_failure::record(...)` 의 인자에 번역 호출이 직접 들어가면 실패.
///
/// 타입이 이미 막지만(`&DiagnosticEnglish`), 규약을 **읽을 수 있는 형태로** 못 박아 둔다 —
/// `record` 의 시그니처를 `&str` 로 되돌리는 변경이 이 테스트에 먼저 걸린다.
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
        "`hook_failure::record` 호출을 하나도 못 찾았다 — 모듈이 옮겨졌으면 이 가드의 \
         스캔 대상도 함께 옮겨라(조용한 통과 방지)"
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
    // 식별자 중간·끝의 `t` 는 오탐이 아니다.
    assert!(!contains_translation_call("format!(\"{port}\")"));
    assert!(!contains_translation_call("e.to_string()"));
    assert!(!contains_translation_call("self.source.to_string()"));
}
