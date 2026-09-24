//! 단축키 번들 타입에 TOML로 표현하기 어려운 Option 구조가 추가되는지 확인한다(ADR-0019).
//! TOML에는 null이 없어 시퀀스 원소의 None과 중첩 Option의 Some(None)이 문제가 된다.
//! 일반 Option 필드는 생략할 수 있고, 맵 값의 None도 키가 생략되지만 해당 엔트리를 그대로 복원할 수는 없다.
//! 기존 왕복 테스트에 None 값이 없으면 새 타입 문제가 드러나지 않아 선언 형태를 별도로 검사한다.
//!
//! CARRIED는 번들이 저장하는 타입을 사람이 따라가며 만든 목록이다.
//! 설정 소스의 선언 타입은 모두 명부와 대조하지만 다른 파일의 새 타입은 수동 등록이 필요하다.
//! 문자열 형태로 찾으므로 타입 별칭·완전수식 Option 경로·매크로 생성 필드를 놓칠 수 있다.
//! 튜플의 Option은 안전한 맵 값과 문자열로 구별하지 못해 검사하지 않는다.
//! 주석·일반 문자열은 제거하지만 raw string은 정확히 판독하지 못한다.

use std::path::PathBuf;

/// 이 설정 파일의 선언 타입은 모두 번들 검사 명부에 있어야 한다.
const KEYBINDINGS_SRC: &str = "crates/tasty-settings/src/keybindings.rs";

const BUNDLE_SRC: &str = "crates/tasty-host-plugin/src/keybinding_bundle.rs";

/// 다른 타입도 있는 파일이므로 등록한 override 타입만 검사한다.
const OVERRIDE_SRC: &str = "crates/tasty-host-plugin/src/registry_state.rs";

/// KeybindingBundle의 필드와 별칭을 따라 수동으로 등록한 직렬화 대상 타입.
const CARRIED: &[(&str, TypeKind, &str)] = &[
    (BUNDLE_SRC, TypeKind::Struct, "KeybindingBundle"),
    (KEYBINDINGS_SRC, TypeKind::Struct, "KeybindingSettings"),
    (KEYBINDINGS_SRC, TypeKind::Struct, "ScriptBinding"),
    (OVERRIDE_SRC, TypeKind::Enum, "ShortcutOverride"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TypeKind {
    Struct,
    Enum,
    Union,
}

impl TypeKind {
    const ALL: &'static [Self] = &[Self::Struct, Self::Enum, Self::Union];

    fn keyword(self) -> &'static str {
        match self {
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::Union => "union",
        }
    }
}

/// 공백을 제거한 타입 본문에서 찾을 금지 형태. 앞의 다섯은 시퀀스, 마지막은 중첩 Option이다.
const FORBIDDEN: &[&str] = &[
    "Vec<Option<",
    "[Option<",
    "VecDeque<Option<",
    "BTreeSet<Option<",
    "HashSet<Option<",
    "Option<Option<",
];

fn read(rel: &str) -> String {
    let p: PathBuf = tasty_doc_guards::repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// 본문 범위를 자르기 전에 주석·일반 문자열을 지워 예시나 중괄호가 판정에 섞이지 않게 한다.
/// raw string을 따로 판독하지 않아 내부 따옴표 뒤가 코드로 남거나 마지막 백슬래시 뒤의 코드를 가릴 수 있다.
fn code_only(s: &str) -> String {
    let c: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    let mut block = 0usize;
    while i < c.len() {
        if block > 0 {
            if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                block += 1;
                i += 2;
            } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                block -= 1;
                i += 2;
            } else {
                if c[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            continue;
        }
        if c[i] == '/' && c.get(i + 1) == Some(&'*') {
            block = 1;
            i += 2;
            continue;
        }
        if c[i] == '/' && c.get(i + 1) == Some(&'/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c[i] == '"' {
            out.push('"');
            i += 1;
            while i < c.len() && c[i] != '"' {
                if c[i] == '\\' {
                    i += 1;
                }
                if i < c.len() && c[i] == '\n' {
                    out.push('\n');
                }
                i += 1;
            }
            if i < c.len() {
                out.push('"');
                i += 1;
            }
            continue;
        }
        out.push(c[i]);
        i += 1;
    }
    out
}

fn type_body(src: &str, rel: &str, kind: TypeKind, name: &str) -> String {
    let header = format!("{} {name} {{", kind.keyword());
    let start = src.find(&header).unwrap_or_else(|| {
        panic!(
            "{rel} 에서 `{header}` 를 못 찾았다 — 타입이 옮겨졌으면 이 가드의 \
             명부(`CARRIED`)도 함께 옮겨야 한다 (ADR-0019)."
        )
    });
    let after = &src[start..];
    let open = after.find('{').expect("type has no opening brace");
    let mut depth = 0i32;
    for (i, ch) in after[open..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return after[..open + i + 1].to_string();
                }
            }
            _ => {}
        }
    }
    panic!("{rel} 의 `{header}` 본문의 닫는 중괄호를 못 찾았다");
}

/// 마스킹한 소스에서 키워드 경계와 식별자 형태로 선언된 타입 이름을 읽는다.
fn declared_types(src: &str) -> Vec<(TypeKind, String)> {
    let mut out = Vec::new();
    for kind in TypeKind::ALL {
        let kw = kind.keyword();
        for (at, _) in src.match_indices(kw) {
            let before_ok = at == 0
                || src[..at]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_whitespace());
            if !before_ok {
                continue;
            }
            let rest = &src[at + kw.len()..];
            let mut chars = rest.chars();
            if chars.next() != Some(' ') {
                continue;
            }
            let ident: String = chars
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if ident.is_empty() || !ident.starts_with(char::is_uppercase) {
                continue;
            }
            out.push((*kind, ident));
        }
    }
    out.sort();
    out.dedup();
    out
}

#[test]
fn keybinding_types_have_no_option_inside_a_sequence() {
    for (rel, kind, name) in CARRIED {
        let src = code_only(&read(rel));
        let body = type_body(&src, rel, *kind, name);
        let squeezed: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        for shape in FORBIDDEN {
            assert!(
                !squeezed.contains(shape),
                "{name}({rel})에 TOML에서 None을 표현할 수 없는 타입 형태 {shape}가 있다. 필드 표현이나 번들 포맷을 ADR-0019에 따라 재검토한다. 일반 Option 필드는 생략할 수 있지만 맵 값의 None은 엔트리 자체가 사라진다는 차이가 있다.\n{body}"
            );
        }
    }
}

/// 설정 파일의 새 선언을 놓치지 않도록 전체 타입 이름을 명부와 대조한다.
#[test]
fn roster_covers_every_type_declared_in_the_settings_source() {
    let src = code_only(&read(KEYBINDINGS_SRC));
    for (kind, name) in declared_types(&src) {
        assert!(
            CARRIED
                .iter()
                .any(|(rel, k, n)| *rel == KEYBINDINGS_SRC && *k == kind && *n == name),
            "{KEYBINDINGS_SRC}의 {} {name}이 CARRIED에 없다. 번들에 저장할 타입인지 확인하고 명부를 갱신한다(ADR-0019).",
            kind.keyword()
        );
    }
}
