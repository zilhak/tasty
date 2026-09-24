//! PORT_MODES와 한국어·영어 가이드 표의 포트 모드 값을 비교한다.
//! 코드의 허용 목록에 맞춰 문서를 유지하되 실제 모드 동작이나 설명 열은 검사하지 않는다.
//! 모든 목록이 함께 잘못 바뀌는 경우도 이 비교로는 찾지 못한다.
//! 번역 대상인 괄호 설명은 제외하고 첫 열의 백틱 안 CLI 값만 읽는다.

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

fn const_values(src: &str, name: &str) -> BTreeSet<String> {
    let decl = format!("{name}: &[&str] = &[");
    let at = src
        .find(&decl)
        .unwrap_or_else(|| panic!("{CODE} 에서 {name} 선언을 못 찾았다 — 형태가 바뀌었다"));
    // 타입 &[&str]의 괄호가 아니라 초기화 배열을 읽도록 선언 뒤에서 시작한다.
    let open = at + decl.len() - 1;
    let close = src[open..].find(']').expect("닫는 괄호") + open;
    src[open..close]
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

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

/// 지정한 표 첫 열의 백틱 안 값만 읽어 번역된 괄호 설명과 구별한다.
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
        "{doc}에서 {head:?} 표를 찾지 못했다. 제목·형식·이동 여부를 확인한다."
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
            "{KO_DOC}의 포트 모드 표를 코드의 {CONST_NAME}에 맞춘다.\n  코드에만: {:?}\n  문서에만: {:?}",
            code.difference(&ko).collect::<Vec<_>>(),
            ko.difference(&code).collect::<Vec<_>>(),
        );
        assert_eq!(
            code,
            en,
            "{EN_DOC}의 포트 모드 표를 코드의 {CONST_NAME}에 맞춘다.\n  코드에만: {:?}\n  문서에만: {:?}",
            code.difference(&en).collect::<Vec<_>>(),
            en.difference(&code).collect::<Vec<_>>(),
        );
    }

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

    #[test]
    fn a_missing_table_is_a_measurement_failure_not_a_pass() {
        let r = std::panic::catch_unwind(|| {
            table_first_column("표가 없는 문서.\n", KO_HEAD, "fixture.md")
        });
        assert!(r.is_err(), "표가 없는데 빈 집합으로 통과했다");
    }
}
