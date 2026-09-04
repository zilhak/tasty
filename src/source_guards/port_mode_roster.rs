//! 포트 발견 모드 명부가 적힌 세 자리가 같은 값을 열거하는지 못 박는다.
//!
//! 자리 셋이고, 셋 다 "`--port-mode` 에 넣을 수 있는 값이 무엇인가" 라는 **한
//! 질문**에 답한다.
//!
//! 1. `crates/tasty-remote-profiles/src/profile.rs` 의 `PORT_MODES` — 코드
//! 2. `site/content/remote/attach.md` 의 포트 모드 표 — 한국어 가이드
//! 3. `site/content/en/remote/attach.md` 의 같은 표 — 영어 가이드
//!
//! # 초록이 뜻하는 것
//!
//! **세 자리가 같은 값을 열거한다.** 그것뿐이다.
//!
//! 초록이 뜻하지 **않는** 것 셋을 먼저 적는다.
//!
//! - **그 값들이 실제로 받아들여진다는 것이 아니다.** 그것은
//!   `is_valid_port_mode` 의 명제고 이 판정기는 그 함수를 부르지 않는다.
//! - **목록이 완전하다는 것이 아니다.** 셋이 함께 빠뜨리면 초록이다. 이
//!   판정기는 자리 사이의 **어긋남**을 잡지 명부의 완전성을 잡지 않는다.
//! - **문서의 설명이 옳다는 것이 아니다.** 표의 둘째 열은 안 본다.
//!
//! # 어긋나면 어느 쪽이 틀렸나
//!
//! 상수 쪽이 옳다. `is_valid_port_mode` 가 그 상수를 그대로 쓰므로 코드의
//! 동작이 거기서 나온다 — 문서는 복제고 상수는 오라클이다. 그래서 실패
//! 메시지는 문서를 고치라고 말한다.
//!
//! # 왜 이 자리가 판정 가능한가
//!
//! 한국어와 영어 가이드가 짝을 이루는 자리는 대개 집합 동등을 걸 수 없다 —
//! 같은 열에 식별자와 번역 대상 자리표시자가 섞여 있어서, 동등을 요구하면
//! 번역을 금지하게 된다. 이 자리는 다르다. 첫 열의 백틱 토큰이 전부 CLI 가
//! 문자 그대로 받는 리터럴이고, 번역되는 부분(`(기본)` / `(default)`)은 백틱
//! **밖**에 있다. 자리가 균질해서 판정이 정의된다.

use std::collections::BTreeSet;

use super::repo_root;

const CODE: &str = "crates/tasty-remote-profiles/src/profile.rs";
const CONST_NAME: &str = "PORT_MODES";
const KO_DOC: &str = "site/content/remote/attach.md";
const KO_HEAD: &str = "| 값 | 동작 |";
const EN_DOC: &str = "site/content/en/remote/attach.md";
const EN_HEAD: &str = "| Value | Behavior |";

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
}

/// 상수의 문자열 리터럴. 배열 경계는 `=` 뒤의 `[` 부터 짝이 맞는 `]` 까지다.
fn const_values(src: &str, name: &str) -> BTreeSet<String> {
    let decl = format!("{name}: &[&str] = &[");
    let at = src
        .find(&decl)
        .unwrap_or_else(|| panic!("{CODE} 에서 {name} 선언을 못 찾았다 — 형태가 바뀌었다"));
    // 선언 전체를 건너뛴 뒤에 여는 괄호를 잡는다. `find('[')` 를 바로 쓰면
    // 배열이 아니라 타입 `&[&str]` 의 괄호를 집어 값 0 개를 돌려준다.
    let open = at + decl.len() - 1;
    let close = src[open..].find(']').expect("닫는 괄호") + open;
    src[open..close]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// 코드펜스를 지운다 — 펜스 안의 예시 명령이 표나 백틱으로 읽히지 않게.
fn strip_fences(text: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            inside = !inside;
            continue;
        }
        if !inside {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// 지목한 헤더로 시작하는 표의 첫 열에서 백틱 토큰을 모은다.
///
/// 첫 열만 보고 백틱 안만 취하는 것이 균질성을 지키는 방법이다 — 같은 칸의
/// 괄호 주석(`(기본)` / `(default)`)은 백틱 밖이라 안 들어온다.
fn table_first_column(text: &str, head: &str, doc: &str) -> BTreeSet<String> {
    let src = strip_fences(text);
    let mut out = BTreeSet::new();
    let mut inside = false;
    let mut rows = 0usize;
    for line in src.lines() {
        let t = line.trim();
        if !inside {
            if t == head {
                inside = true;
            }
            continue;
        }
        if !t.starts_with('|') {
            break;
        }
        if t.chars().all(|c| "|-: ".contains(c)) {
            continue;
        }
        rows += 1;
        let first = t.trim_matches('|').split('|').next().unwrap_or("");
        let mut rest = first;
        while let Some(i) = rest.find('`') {
            let after = &rest[i + 1..];
            match after.find('`') {
                Some(j) => {
                    out.insert(after[..j].to_string());
                    rest = &after[j + 1..];
                }
                None => break,
            }
        }
    }
    assert!(
        rows > 0,
        "{doc} 에서 헤더 {head:?} 로 시작하는 표를 못 찾았다 — 표가 옮겨졌거나 \
         헤더가 바뀌었다. 이건 통과가 아니라 측정 실패다"
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_modes_are_enumerated_the_same_in_code_and_in_both_guides() {
        let code = const_values(&read(CODE), CONST_NAME);
        assert!(
            code.len() >= 3,
            "{CONST_NAME} 에서 값을 {} 개밖에 못 읽었다 — 측정 실패다",
            code.len()
        );
        let ko = table_first_column(&read(KO_DOC), KO_HEAD, KO_DOC);
        let en = table_first_column(&read(EN_DOC), EN_HEAD, EN_DOC);
        assert_eq!(
            code,
            ko,
            "{KO_DOC} 의 포트 모드 표가 {CONST_NAME} 과 어긋난다. 상수가 오라클이니 \
             문서를 고쳐라.\n  코드에만: {:?}\n  문서에만: {:?}",
            code.difference(&ko).collect::<Vec<_>>(),
            ko.difference(&code).collect::<Vec<_>>(),
        );
        assert_eq!(
            code,
            en,
            "{EN_DOC} 의 포트 모드 표가 {CONST_NAME} 과 어긋난다. 상수가 오라클이니 \
             문서를 고쳐라.\n  코드에만: {:?}\n  문서에만: {:?}",
            code.difference(&en).collect::<Vec<_>>(),
            en.difference(&code).collect::<Vec<_>>(),
        );
    }

    /// 이 자리가 균질하다는 것 — 번역되는 부분이 판정에 안 들어온다.
    ///
    /// 두 가이드의 같은 칸에는 번역되는 괄호 주석이 붙어 있다. 그것이 값으로
    /// 새면 이 판정기는 번역을 금지하는 가드가 된다.
    #[test]
    fn the_translated_parenthetical_does_not_leak_into_the_values() {
        let ko_raw = read(KO_DOC);
        let en_raw = read(EN_DOC);
        assert!(
            ko_raw.contains("`auto` (기본)") && en_raw.contains("`auto` (default)"),
            "두 가이드의 괄호 주석이 사라졌다 — 이 대조가 무의미해졌다"
        );
        let ko = table_first_column(&ko_raw, KO_HEAD, KO_DOC);
        let en = table_first_column(&en_raw, EN_HEAD, EN_DOC);
        assert_eq!(ko, en, "번역되는 부분이 값으로 샜다");
        assert!(
            !ko.iter()
                .any(|v| v.contains("기본") || v.contains("default")),
            "괄호 주석이 값에 섞였다: {ko:?}"
        );
    }

    /// 파서 회귀 — 실제 자리를 고쳐서 통과시키지 않는다(합성 픽스처).
    #[test]
    fn the_parser_reads_only_the_named_table_and_ignores_code_fences() {
        const FIXTURE: &str = "\
설명 문단.

```sh
tasty tool remote-profile edit --port-mode file-unix
| `펜스안` | 표처럼 생겼지만 표가 아니다 |
```

| 값 | 동작 |
|---|---|
| `alpha` (기본) | 설명 |
| `beta` | 설명 |
| `gamma-delta` | 설명 |

다른 표:

| 값 | 동작 |
|---|---|
| `남의표` | 설명 |
";
        let got = table_first_column(FIXTURE, "| 값 | 동작 |", "fixture.md");
        assert_eq!(
            got,
            ["alpha", "beta", "gamma-delta"]
                .iter()
                .map(|s| s.to_string())
                .collect::<BTreeSet<_>>(),
            "펜스 안이나 뒤따르는 남의 표를 읽었다"
        );
    }

    /// 변이 대조 — 개수를 보존하는 치환은 집합 동등만 잡는다.
    #[test]
    fn swapping_one_value_is_caught_although_the_count_is_unchanged() {
        let code = const_values(&read(CODE), CONST_NAME);
        let ko = table_first_column(&read(KO_DOC), KO_HEAD, KO_DOC);
        let victim = code.iter().next().expect("빈 집합").clone();
        let mut mutated = code.clone();
        mutated.remove(&victim);
        mutated.insert(format!("{victim}-not-a-real-mode"));
        assert_eq!(mutated.len(), code.len(), "변이가 개수를 바꿨다");
        assert_ne!(
            mutated, ko,
            "치환 변이가 판정을 통과했다 — 이 가드는 어긋남을 못 잡는다"
        );
    }

    /// 표를 못 찾았을 때 조용히 빈 집합으로 통과하지 않는다.
    #[test]
    fn a_missing_table_is_a_measurement_failure_not_a_pass() {
        let r = std::panic::catch_unwind(|| {
            table_first_column("표가 없는 문서.\n", KO_HEAD, "fixture.md")
        });
        assert!(r.is_err(), "표가 없는데 통과했다 — 0 을 초록으로 읽는다");
    }
}
