//! 단축키 이식 번들이 TOML 로 나갈 수 있는 모양인지 지키는 가드 — ADR-0255 의
//! 재검토 조건 "`Option<T>` 를 원소로 갖는 시퀀스 필드가 생긴다" 의 채널.
//!
//! ## 무엇이 실제로 깨지는가 (실측 2026-09-09, `toml` 0.8 / `toml_edit` 0.20)
//!
//! TOML 에는 null 리터럴이 없다. 그런데 `None` 이 깨지는 자리는 **한 층뿐이다**.
//!
//! - **필드 자리·맵 값 자리의 `None` 은 통과한다** — 직렬화기가 그 키를 통째로 생략하고
//!   (`toml_edit` 의 `ser::map` 이 `UnsupportedNone` 을 그 자리에서만 삼킨다),
//!   `#[serde(default)]` 가 붙은 타입은 역직렬화도 그대로 `None` 으로 돌아온다.
//!   중첩 깊이는 상관없다 — 중첩 구조체 안이든 `[[array of tables]]` 안이든 같다.
//! - **시퀀스 원소의 `None` 은 깨진다** — `Vec<Option<T>>` · `[Option<T>; N]` 는
//!   `unsupported None value`(`toml_edit::ser::Error::UnsupportedNone`) 로 실패한다.
//!   배열 원소에는 생략할 키가 없기 때문이다.
//!
//! 그래서 이 가드가 보는 것은 "`Option` 이 있는가" 가 아니라 **"`Option` 이 시퀀스
//! 원소인가"** 다. 앞엣것으로 세면 안전한 필드까지 위반으로 잡아, 실재하지 않는 위반에
//! 대한 처방을 내보내게 된다.
//!
//! ## 왜 시험이 아니라 소스 형태로 재는가
//!
//! 깨짐은 **값**에 `None` 이 실제로 들어야 발화한다. `Vec<Option<T>>` 필드가 생겨도
//! 컴파일은 통과하고, 기존 round-trip 시험(`round_trips_the_whole_host_section`)도
//! 그 필드에 `None` 을 넣기 전까지는 초록이다. 그래서 판정의 주어는 값이 아니라
//! **선언의 모양**이고, 그것은 판정 시점에 레포가 읽을 수 있다.
//!
//! ## 이 가드가 닿지 않는 곳
//!
//! 선언 텍스트를 읽으므로, 시퀀스 원소의 `Option` 이 **타입 별칭 뒤에 숨거나**
//! 이 파일 밖에 정의된 타입 안에 있으면 못 본다. 지금은 번들이 싣는 타입이 둘 다
//! 이 파일에 있어 그 구멍이 닫혀 있다 — 다른 파일의 타입을 필드로 들이면 이 가드도
//! 함께 넓혀야 한다.

use std::path::PathBuf;

/// 번들이 싣는 호스트 측 타입이 모두 사는 파일.
const KEYBINDINGS_SRC: &str = "crates/tasty-settings/src/keybindings.rs";

/// 시퀀스 원소 자리의 `Option` — 공백을 지운 타입 문자열에서 찾는 형태.
const FORBIDDEN: &[&str] = &[
    "Vec<Option<",
    "[Option<",
    "VecDeque<Option<",
    "BTreeSet<Option<",
    "HashSet<Option<",
];

fn read(rel: &str) -> String {
    let p: PathBuf = tasty_doc_guards::repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// 줄 주석을 지운다 — 설명 문구 안의 `Vec<Option<...>>` 이 위반으로 세지지 않게.
fn code_only(s: &str) -> String {
    s.lines()
        .map(|l| match l.find("//") {
            Some(i) => &l[..i],
            None => l,
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `struct <name> {` 부터 중괄호 깊이가 0 으로 돌아오는 지점까지.
fn struct_body(src: &str, name: &str) -> String {
    let header = format!("struct {name} {{");
    let start = src.find(&header).unwrap_or_else(|| {
        panic!(
            "{KEYBINDINGS_SRC} 에서 `{header}` 를 못 찾았다 — 타입이 \
                                  옮겨졌으면 이 가드의 대상도 함께 옮겨야 한다 (ADR-0255)."
        )
    });
    let after = &src[start..];
    let open = after.find('{').expect("struct has no opening brace");
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
    panic!("`{header}` 본문의 닫는 중괄호를 못 찾았다");
}

#[test]
fn keybinding_types_have_no_option_inside_a_sequence() {
    let src = read(KEYBINDINGS_SRC);
    for name in ["KeybindingSettings", "ScriptBinding"] {
        let body = code_only(&struct_body(&src, name));
        let squeezed: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        for shape in FORBIDDEN {
            assert!(
                !squeezed.contains(shape),
                "`{name}` 에 시퀀스 원소가 `Option` 인 필드가 생겼다(`{shape}…`). \
                 TOML 에는 null 리터럴이 없어 배열 원소의 `None` 은 생략할 키가 없다 — \
                 `keybinding_bundle::encode` 가 그 필드만이 아니라 **번들 전체**를 \
                 `unsupported None value` 로 실패시킨다. 필드 자리의 `Option` 은 안전하니 \
                 이 가드를 넓히지 말고, 그 필드의 표현이나 번들 포맷을 다시 정해라 \
                 (docs/adr/0255-the-keybinding-bundle-is-a-toml-file-with-a-schema-tag.md \
                 재검토 조건).\n{body}"
            );
        }
    }
}
