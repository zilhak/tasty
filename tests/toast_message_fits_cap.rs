//! 등록한 토스트 문구가 모든 언어에서 글자 수 상한에 들어가는지 확인한다.
//! {}가 있으면 고정 문구에 MIN_FRAGMENT_CHARS를 더해 축약할 경로의 최소 공간을 남기는지 본다.
//! 실제 경로를 넣는 호출부는 t_fmt_fit를 사용해야 한다.

use std::collections::BTreeMap;
use std::path::Path;

const LANGS: &[&str] = &tasty_i18n::BUILTIN_CODES;

/// 토스트로 쓰는 키 접두사만 등록한다. 모달처럼 다른 상한을 쓰는 문구는 포함하지 않는다.
const TOAST_KEY_PREFIXES: &[(&str, &str)] = &[(
    "persistence.warn.",
    "부팅 시 설정·레이아웃을 읽지 못한 사실을 알리는 Warning 토스트 \
     (`src/app/boot_machine.rs::report_persistence_incidents`)",
)];

#[test]
fn toast_strings_fit_the_toast_cap_in_every_locale() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("lang");
    let cap = tasty_i18n::TOAST_MAX_CHARS;
    let mut problems = Vec::new();
    let mut checked = 0usize;

    for lang in LANGS {
        for (key, value) in load(&root, lang) {
            if !TOAST_KEY_PREFIXES.iter().any(|(p, _)| key.starts_with(p)) {
                continue;
            }
            checked += 1;
            let skeleton = value.replace("{}", "").chars().count();
            let needed = skeleton
                + if value.contains("{}") {
                    tasty_i18n::MIN_FRAGMENT_CHARS
                } else {
                    0
                };
            if needed > cap {
                problems.push(format!(
                    "  lang/{lang}.toml `{key}`: 필요한 최소 공간 {needed}자(고정 문구 {skeleton} + 경로 하한)가 상한 {cap}을 넘는다\n      {value:?}"
                ));
            }
        }
    }

    assert!(
        checked > 0,
        "토스트 키를 하나도 찾지 못했다 — TOAST_KEY_PREFIXES 가 실제 키와 어긋났다"
    );
    assert!(
        problems.is_empty(),
        "토스트 문구에 경로를 축약해 넣을 공간이 부족하다. 필요한 조치 안내를 남기고 문구를 줄인다:\n{}",
        problems.join("\n")
    );
}

#[test]
fn every_declared_toast_prefix_matches_at_least_one_key() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("lang");
    let en = load(&root, "en");
    for (prefix, reason) in TOAST_KEY_PREFIXES {
        assert!(
            en.keys().any(|k| k.starts_with(prefix)),
            "lang/en.toml 에 `{prefix}` 로 시작하는 키가 없다 ({reason}) — \
             키를 옮겼으면 TOAST_KEY_PREFIXES 도 함께 고쳐라"
        );
    }
}

/// 상한 안내는 숫자를 번역문에 고정하지 않고 placeholder로 받는다.
#[test]
fn the_char_limit_notice_takes_the_cap_as_an_argument_in_every_locale() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("lang");
    for lang in LANGS {
        let notice = load(&root, lang)
            .remove("toast.char_limit_notice")
            .unwrap_or_else(|| panic!("lang/{lang}.toml 에 toast.char_limit_notice 가 없다"));
        assert_eq!(
            notice.matches("{}").count(),
            1,
            "lang/{lang}.toml `toast.char_limit_notice` 는 캡 값을 받을 `{{}}` 를 정확히 \
             하나 가져야 한다 (현재: {notice:?})"
        );
        assert!(
            !notice.chars().any(|c| c.is_ascii_digit()),
            "lang/{lang}.toml의 toast.char_limit_notice에 숫자가 고정돼 있다. 상한을 {{}} 인자로 받도록 한다: {notice:?}"
        );
    }
}

fn load(dir: &Path, lang: &str) -> BTreeMap<String, String> {
    let path = dir.join(format!("{lang}.toml"));
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let value: toml::Value = text
        .parse()
        .unwrap_or_else(|e| panic!("parse {}: {e}", path.display()));
    let mut out = BTreeMap::new();
    flatten("", &value, &mut out);
    out
}

fn flatten(prefix: &str, value: &toml::Value, out: &mut BTreeMap<String, String>) {
    match value {
        toml::Value::Table(table) => {
            for (k, v) in table {
                let key = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                flatten(&key, v, out);
            }
        }
        toml::Value::String(s) => {
            out.insert(prefix.to_string(), s.clone());
        }
        _ => {}
    }
}

// 경로를 넣는 호출도 길이를 제한하도록 해당 함수의 t_fmt 사용을 검사한다.

const PATH_WARNING_FN: &str = "report_persistence_incidents";

#[test]
fn the_path_bearing_boot_warning_renders_through_the_eliding_helper() {
    let src_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/app/boot_machine.rs");
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", src_path.display()));
    let lines: Vec<&str> = src.lines().collect();
    let (start, end) = fn_span(&lines, PATH_WARNING_FN).unwrap_or_else(|| {
        panic!(
            "{}::{PATH_WARNING_FN} 를 못 찾았다 — 이름이 바뀌었으면 이 가드도 함께 옮겨라",
            src_path.display()
        )
    });

    let body = &lines[start..=end];
    let offenders: Vec<String> = body
        .iter()
        .enumerate()
        .filter(|(_, l)| !is_comment_line(l))
        .filter(|(_, l)| strip_line_comment(l).contains("t_fmt("))
        .map(|(i, l)| format!("  {}:{}: {}", src_path.display(), start + i + 1, l.trim()))
        .collect();

    assert!(
        offenders.is_empty(),
        "{PATH_WARNING_FN}에서 t_fmt 호출을 찾았다. 긴 경로가 조치 안내를 잘라내지 않도록 삽입 값을 줄이는 t_fmt_fit를 사용한다:\n{}",
        offenders.join("\n")
    );

    assert!(
        body.iter().any(|l| l.contains("t_fmt_fit(")),
        "{PATH_WARNING_FN}에서 t_fmt_fit 호출을 찾지 못했다. 함수 범위와 호출 형식을 확인한다."
    );
}

/// 단순 중괄호 깊이로 함수 범위를 찾는다. 문자열 안 중괄호까지 구별하는 파서는 아니다.
fn fn_span(lines: &[&str], name: &str) -> Option<(usize, usize)> {
    let header = format!("fn {name}(");
    let start = lines
        .iter()
        .position(|l| !is_comment_line(l) && l.contains(&header))?;
    let (mut depth, mut opened) = (0i32, false);
    for (i, line) in lines.iter().enumerate().skip(start) {
        for ch in strip_line_comment(line).chars() {
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            return Some((start, i));
        }
    }
    None
}

fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with("/*") || t.starts_with('*')
}

/// 첫 // 뒤를 제거하며 문자열 내부의 //는 구별하지 못한다.
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}
