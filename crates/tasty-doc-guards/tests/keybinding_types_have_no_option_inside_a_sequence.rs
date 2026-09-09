//! 단축키 이식 번들이 TOML 로 나갈 수 있는 모양인지 지키는 가드 — ADR-0257 의
//! 재검토 조건 "`Option<T>` 를 원소로 갖는 시퀀스 필드가 생긴다" 의 채널. 같은 뿌리로
//! 깨지는 `Option<Option<T>>` 도 함께 막는다(아래 참조).
//!
//! ## 무엇이 실제로 깨지는가 (실측 2026-09-09, `toml` 0.8 / `toml_edit` 0.20)
//!
//! TOML 에는 null 리터럴이 없다. 그래서 `None` 은 **생략할 키가 있는 자리에서만**
//! 통과한다. 측정한 모양은 다섯이다.
//!
//! - **필드 자리의 평범한 `Option<T>` 는 통과한다** — 직렬화기가 그 키를 통째로 생략하고
//!   (`toml_edit` 의 `ser::map` 이 `UnsupportedNone` 을 그 자리에서만 삼킨다),
//!   `#[serde(default)]` 가 붙은 타입은 역직렬화도 그대로 `None` 으로 돌아온다.
//!   중첩 구조체 안이든 `[[array of tables]]` 안이든 같다.
//! - **맵 값 자리의 `Option<T>` 도 직렬화는 통과한다 — 다만 그 키가 사라진다.**
//!   `BTreeMap<String, Option<String>>` 에 `("k", None)` 을 넣으면 결과 테이블에 `k` 가
//!   아예 안 실리고, 역직렬화하면 그 엔트리가 **없는 맵**이 돌아온다(`None` 을 값으로 가진
//!   엔트리가 아니다). 필드 자리와 달리 `#[serde(default)]` 가 되살릴 대상 자체가 없다.
//! - **시퀀스 원소의 `None` 은 깨진다** — `Vec<Option<T>>` · `[Option<T>; N]` 는
//!   `unsupported None value`(`toml_edit::ser::Error::UnsupportedNone`) 로 실패한다.
//!   배열 원소에는 생략할 키가 없기 때문이다.
//! - **튜플의 `None` 도 깨진다** — 튜플이 배열로 나가므로 `(String, Option<String>)` 는
//!   필드 자리에 있어도, `Vec<(String, Option<String>)>` 안에 있어도 같은 에러다.
//! - **`Option<Option<T>>` 의 `Some(None)` 도 깨진다 — 필드 자리인데 깨진다.**
//!   바깥 `Option` 이 키 생략을 이미 써 버려 안쪽 `None` 에는 생략할 키가 남지 않는다
//!   (`None` 과 `Some(Some(x))` 는 통과한다).
//!
//! 그래서 이 가드가 보는 것은 "`Option` 이 있는가" 가 아니라 **"`None` 이 생략할 키를
//! 못 갖는 자리인가"** 다. 앞엣것으로 세면 안전한 필드까지 위반으로 잡아, 실재하지 않는
//! 위반에 대한 처방을 내보내게 된다.
//!
//! ## 왜 시험이 아니라 소스 형태로 재는가
//!
//! 깨짐은 **값**에 `None` 이 실제로 들어야 발화한다. `Vec<Option<T>>` 필드가 생겨도
//! 컴파일은 통과하고, 기존 round-trip 시험(`round_trips_the_whole_host_section`)도
//! 그 필드에 `None` 을 넣기 전까지는 초록이다. 그래서 판정의 주어는 값이 아니라
//! **선언의 모양**이고, 그것은 판정 시점에 레포가 읽을 수 있다.
//!
//! ## 무엇을 좌변으로 삼는가
//!
//! 번들이 싣는 타입 [`CARRIED`] 전량이다 — `encode` 가 직렬화하는 주어
//! `KeybindingBundle` 에서 출발해 필드 타입을 펼치며(타입 별칭 포함) std 가 아닌 이름
//! 있는 타입마다 재귀해 얻은 폐포이고, 세 파일에 흩어져 있다. 그 명부가 [`KEYBINDINGS_SRC`]
//! 에 대해서는 **닫혀 있는지도 대조한다** — 그 파일은 번들이 싣는 설정 타입만을 위해
//! 존재하므로, 거기 선언된 타입 전량이 명부에 있어야 한다
//! ([`roster_covers_every_type_declared_in_the_settings_source`]).
//!
//! ## 이 가드가 닿지 않는 곳
//!
//! 선언 텍스트를 읽으므로 [`FORBIDDEN`] 의 문자열과 글자가 안 맞으면 못 본다. 측정으로
//! 확인한 구멍은 아래와 같다 — **닫혀 있지 않다.**
//!
//! - **타입 별칭 뒤에 숨은 것** (`type Slots = Vec<Option<String>>;` 을 필드 타입으로 쓰면
//!   본문에는 `Slots` 만 남는다). **지금 하나 있다** — `KeybindingBundle` 의
//!   `plugin_keybindings` 는 별칭 `PluginShortcutOverrides` 로 적혀 있어 그 뒤의 모양이
//!   이 가드에 안 보인다. 그 별칭이 펼쳐지는 `ShortcutOverride` 는 명부에 따로 실어 본다.
//! - **명부에 없는 파일의 타입** — 번들이 새 타입을 들이면 [`CARRIED`] 를 손으로 넓혀야
//!   한다. [`KEYBINDINGS_SRC`] 한 파일만 완전성 대조가 붙어 있고, 나머지 두 파일에는
//!   그런 채널이 없다.
//! - **매크로가 만들어 내는 필드** — 선언 텍스트에 타입이 안 나타나면 못 본다. 지금
//!   명부의 네 타입은 모두 필드를 손으로 적고 있어 해당 사례가 없다.
//! - **튜플 안의 `Option`** — 위에서 측정했듯 실제로 깨지는데 안 잡는다. 일부러 안
//!   막았다: 공백을 지운 뒤 `,Option<` 로 세면 **안전한** 맵 값
//!   (`HashMap<String,Option<String>>`)까지 위반으로 잡혀, 실재하지 않는 위반에 대한
//!   처방("표현을 다시 정해라")을 내보내게 된다. 튜플 필드가 실제로 생기면 그때
//!   튜플만 골라내는 판정을 짓는다.
//! - **완전수식 경로** (`Vec<std::option::Option<T>>` · `Vec<core::option::Option<T>>`) —
//!   글자가 안 맞아 안 잡힌다.
//!
//! 주석과 문자열 리터럴 안의 텍스트는 [`code_only`] 가 지우므로 위반으로 안 센다
//! (raw string 은 갈래가 셋 — [`code_only`] 참조).

use std::path::PathBuf;

/// 번들이 싣는 설정 타입이 사는 파일. 이 파일은 그 타입들만을 위해 존재하므로
/// [`roster_covers_every_type_declared_in_the_settings_source`] 가 완전성을 대조한다.
const KEYBINDINGS_SRC: &str = "crates/tasty-settings/src/keybindings.rs";

/// 번들 본체 타입이 사는 파일.
const BUNDLE_SRC: &str = "crates/tasty-host-plugin/src/keybinding_bundle.rs";

/// plugin override 타입이 사는 파일. 번들과 무관한 타입도 여럿 사는 파일이라
/// 완전성 대조는 안 붙인다 — 명부가 이름으로 하나만 고른다.
const OVERRIDE_SRC: &str = "crates/tasty-host-plugin/src/registry_state.rs";

/// 번들이 싣는 타입 전량 — (파일, 종류, 이름).
///
/// 도출: `keybinding_bundle::encode` 가 직렬화하는 주어 `KeybindingBundle` 에서 출발해
/// 필드 타입을 하나씩 펼치고(타입 별칭 `PluginShortcutOverrides` 포함), std 가 아닌
/// 이름 있는 타입마다 재귀했다. 나머지 필드 타입은 전부 `String` · `u32` · `Vec<_>` ·
/// `BTreeMap<_, _>` · `[String; N]` 라 재귀가 거기서 끝난다. 같은 파일의 다른 타입
/// (`DecodeEnv` · `DecodedBundle` · `BundleWarning` · `SchemaFound` · `BundleError`,
/// `PluginsConfig` · `PluginsDisabled` · `PluginGrants`)은 번들이 직렬화하지 않는다.
const CARRIED: &[(&str, TypeKind, &str)] = &[
    (BUNDLE_SRC, TypeKind::Struct, "KeybindingBundle"),
    (KEYBINDINGS_SRC, TypeKind::Struct, "KeybindingSettings"),
    (KEYBINDINGS_SRC, TypeKind::Struct, "ScriptBinding"),
    (OVERRIDE_SRC, TypeKind::Enum, "ShortcutOverride"),
];

/// 선언 키워드 — 본문을 잘라낼 때와 완전성을 대조할 때 같은 집합을 쓴다.
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

/// `None` 이 생략할 키를 못 갖는 자리 — 공백을 지운 타입 문자열에서 찾는 형태.
///
/// 앞 다섯은 시퀀스 원소, 마지막 하나는 바깥 `Option` 이 키 생략을 소진한 필드 자리다.
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

/// 주석과 문자열 리터럴의 **내용**을 지운다.
///
/// 두 가지를 막는다 — 설명 문구 안의 `Vec<Option<…>>` 이 위반으로 세지는 것(거짓 양성),
/// 그리고 그 안의 중괄호가 [`type_body`] 의 깊이 계수를 흔드는 것. 그래서 본문을
/// 잘라내기 **전에** 원본 전체에 한 번 적용한다.
///
/// 줄 주석(`//`·`///`)·블록 주석(`/* */`, 중첩 포함)·큰따옴표 문자열을 모두 본다.
/// 줄바꿈은 남겨 실패문의 본문이 원본과 같은 줄 나눔으로 읽히게 한다.
///
/// raw string(`r#"…"#`)을 따로 알아보지는 않는다. 실측(2026-09-09)으로 갈래가 셋이다.
///
/// - 안에 `"` 가 없는 raw string 은 **평범한 문자열과 똑같이 내용이 지워진다**
///   (`r#"Vec<Option<u8>>"#` → `r#""#`). 위반으로 안 센다.
/// - 안에 `"` 가 든 raw string 은 그 따옴표가 문자열을 조기에 닫아 **그 뒤 텍스트가
///   코드로 남는다** — 거짓 양성이 될 수 있는 유일한 갈래다. 명부의 세 파일 중
///   [`OVERRIDE_SRC`] 의 `#[cfg(test)]` 모듈에 그런 raw string 이 있지만, 그 자리는
///   [`type_body`] 가 잘라내는 `ShortcutOverride` 본문보다 뒤라 판정에 안 닿는다.
/// - `\` 로 끝나는 raw string(`r"C:\path\"`)은 escape 건너뛰기가 닫는 따옴표를 삼켜
///   **파일 끝까지 먹는다.** 그러면 중괄호 짝이 깨져 [`type_body`] 가 패닉으로 죽는다 —
///   조용한 통과가 아니라 fail-loud 다.
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

/// `<kind> <name> {` 부터 중괄호 깊이가 0 으로 돌아오는 지점까지.
fn type_body(src: &str, rel: &str, kind: TypeKind, name: &str) -> String {
    let header = format!("{} {name} {{", kind.keyword());
    let start = src.find(&header).unwrap_or_else(|| {
        panic!(
            "{rel} 에서 `{header}` 를 못 찾았다 — 타입이 옮겨졌으면 이 가드의 \
             명부(`CARRIED`)도 함께 옮겨야 한다 (ADR-0257)."
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

/// 주석·문자열을 지운 본문에서 선언된 타입 이름 전량을 뽑는다.
///
/// `<kind> <ident>` 형태만 본다 — 키워드 앞이 공백/줄머리여야 하고(`enum` 이 다른
/// 식별자의 꼬리인 경우를 배제), 뒤에는 식별자가 와야 한다.
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
                "`{name}`({rel}) 에 `None` 이 생략할 키를 못 갖는 자리가 생겼다\
                 (`{shape}…`). TOML 에는 null 리터럴이 없어 배열 원소에는 생략할 키가 \
                 없고, `Option<Option<T>>` 의 안쪽 `None`(`Some(None)`)도 바깥 `Option` 이 \
                 키 생략을 이미 써 버려 같은 처지다 — `keybinding_bundle::encode` 가 \
                 그 필드만이 아니라 **번들 전체**를 `unsupported None value` 로 \
                 실패시킨다. 필드 자리의 평범한 `Option<T>` 와 맵 값 자리의 `Option<T>` 는 \
                 안전하니 이 가드를 그리로 넓히지 말고, 그 필드의 표현이나 번들 포맷을 \
                 다시 정해라 \
                 (docs/adr/0257-the-keybinding-bundle-is-a-toml-file-with-a-schema-tag.md \
                 재검토 조건).\n{body}"
            );
        }
    }
}

/// [`KEYBINDINGS_SRC`] 는 번들이 싣는 설정 타입만을 위해 존재하는 파일이라, 거기 선언된
/// 타입은 전부 TOML 로 오간다. 그래서 명부가 그 파일에 대해 닫혀 있는지 대조한다 —
/// 위 가드가 "같은 파일의 다른 타입" 을 놓치던 자리다.
#[test]
fn roster_covers_every_type_declared_in_the_settings_source() {
    let src = code_only(&read(KEYBINDINGS_SRC));
    for (kind, name) in declared_types(&src) {
        assert!(
            CARRIED
                .iter()
                .any(|(rel, k, n)| *rel == KEYBINDINGS_SRC && *k == kind && *n == name),
            "{KEYBINDINGS_SRC} 에 `{} {name}` 이 선언됐는데 `CARRIED` 명부에 없다. \
             이 파일의 타입은 전부 `[keybindings]` 로 TOML 에 실리므로 \
             `keybinding_types_have_no_option_inside_a_sequence` 가 함께 봐야 한다 \
             — 명부에 넣어라 (ADR-0257).",
            kind.keyword()
        );
    }
}
