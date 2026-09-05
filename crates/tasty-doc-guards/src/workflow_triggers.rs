//! 워크플로의 `on:` 트리거를 **구조로** 읽는다 — 그 파일이 매 push 도는가, 아니면
//! 경로 필터 뒤에 있는가.
//!
//! 이 판독이 따로 필요한 이유는 **주석과 트리거 키가 같은 글자를 쓰기 때문**이다.
//! `text.contains("paths-ignore:")` 로 세면 다른 워크플로의 필터를 *설명하는* 주석을
//! 가진 파일이 필터를 *가진* 파일로 읽힌다. 실측(2026-09-05): 레포의 워크플로 11 개 중
//! 정확히 하나가 그 형태였고, 하필 그것이 **필터가 없다는 것 자체가 존재 이유인**
//! `doc-guards.yml` 이다(ADR-0138). 그 오탐은 "필터 없는 채널이 하나도 없다" 쪽으로
//! 기울어 안전한 방향이지만, 안전한 방향의 오탐도 판정을 못 쓰게 만드는 것은 같다.
//!
//! **이 판독이 답하지 않는 것**: 그 필터가 실제로 어떤 push 를 걸렀는가. 어느 커밋들이
//! 한 push 였는지는 git 에 없다 — 그 수는 워크플로의 run 목록으로만 재고, 재는 법은
//! `docs/dev-guide/ci-gates.md` 에 있다. 여기서 보는 것은 **필터의 유무**뿐이다.

/// 한 워크플로의 `push:` 트리거 모양.
#[derive(Debug, PartialEq, Eq)]
pub struct PushTrigger {
    /// `on:` 블록에 `push:` 가 있는가.
    pub present: bool,
    /// 그 `push:` 아래에 `paths:` 나 `paths-ignore:` 가 붙었는가.
    pub path_filtered: bool,
    /// 그 `push:` 가 `tags:` 만 걸고 `branches:` 를 안 거는가.
    ///
    /// 필터가 없다는 것과 **보통 push 마다 돈다**는 것은 다르다. 태그 전용 트리거는
    /// 경로 필터가 하나도 없어도 일상 커밋에서는 안 뜬다 — 그것을 "필터 없는 채널" 로
    /// 세면 릴리스 워크플로가 매 push 채널인 척하게 된다(실측: `release.yml`).
    pub tags_only: bool,
}

/// YAML 주석을 지운다. `#` 은 **줄 첫 토큰이거나 공백 뒤에 올 때만** 주석이다 —
/// 그것이 YAML 의 규칙이고, 안 지키면 `'**/*.md#x'` 같은 값의 일부를 주석으로 잘라
/// 뒤따르는 키를 통째로 잃는다(있는 필터를 없다고 읽는 쪽이라 더 나쁘다).
fn strip_yaml_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let bytes = line.as_bytes();
        let mut cut = line.len();
        for (i, b) in bytes.iter().enumerate() {
            if *b == b'#' && (i == 0 || bytes[i - 1].is_ascii_whitespace()) {
                cut = i;
                break;
            }
        }
        out.push_str(&line[..cut]);
        out.push('\n');
    }
    out
}

/// `on:` 블록의 줄들. 최상위 `on:`(들여쓰기 0)부터 다음 최상위 키 직전까지.
///
/// `on: [push]` 같은 **인라인 흐름 표기**는 이 판독이 안 다룬다. 그 형태를 만나면
/// [`push_trigger`] 가 `None` 을 내고, 부르는 쪽은 그것을 통과가 아니라 **판정 불가**로
/// 다뤄야 한다 — 모르는 것을 초록으로 바꾸면 그 순간부터 아무것도 안 보게 된다.
fn on_block(text: &str) -> Option<Vec<&str>> {
    let mut lines = text.lines();
    // 찾은 줄 자체는 안 쓴다 — 이터레이터를 그 줄 **다음**으로 진행시키는 것이 목적이고,
    // 없으면 `?` 로 판정 불가를 낸다.
    lines.by_ref().find(|l| {
        let t = l.trim_end();
        t == "on:" || t == "\"on\":" || t == "'on':"
    })?;
    let mut block = Vec::new();
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        // 다음 최상위 키에서 끝난다.
        if !line.starts_with(' ') && !line.starts_with('\t') {
            break;
        }
        block.push(line);
    }
    Some(block)
}

/// 한 줄의 들여쓰기 폭(공백 수).
fn indent(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// 워크플로 본문에서 `push:` 트리거의 모양을 읽는다.
///
/// `None` 은 "필터가 없다" 가 아니라 **"이 판독기가 못 읽는 모양이다"** 다.
/// 최상위 `on:` 이 블록 표기가 아닐 때 그렇게 된다.
pub fn push_trigger(text: &str) -> Option<PushTrigger> {
    let stripped = strip_yaml_comments(text);
    let block = on_block(&stripped)?;
    let Some(at) = block
        .iter()
        .position(|l| indent(l) == 2 && l.trim_end().trim_start() == "push:")
    else {
        return Some(PushTrigger {
            present: false,
            path_filtered: false,
            tags_only: false,
        });
    };
    let mut path_filtered = false;
    let mut has_tags = false;
    let mut has_branches = false;
    for line in &block[at + 1..] {
        // 다음 트리거(같은 깊이)에서 끝난다.
        if indent(line) <= 2 {
            break;
        }
        let key = line.trim_start();
        if key.starts_with("paths:") || key.starts_with("paths-ignore:") {
            path_filtered = true;
        }
        if key.starts_with("tags:") || key.starts_with("tags-ignore:") {
            has_tags = true;
        }
        if key.starts_with("branches:") || key.starts_with("branches-ignore:") {
            has_branches = true;
        }
    }
    Some(PushTrigger {
        present: true,
        path_filtered,
        tags_only: has_tags && !has_branches,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 이 판독기가 존재하는 이유 그 자체. 주석이 다른 워크플로의 필터를 설명해도
    /// 이 파일에는 필터가 없다.
    #[test]
    fn a_comment_mentioning_a_filter_is_not_a_filter() {
        let yaml = "\
# crossplatform-check.yml 은 paths-ignore: docs/** 뒤에 있다.
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]

jobs:
  x:
";
        assert_eq!(
            push_trigger(yaml),
            Some(PushTrigger {
                present: true,
                path_filtered: false,
                tags_only: false
            })
        );
    }

    #[test]
    fn a_real_filter_under_push_is_seen() {
        let yaml = "\
on:
  push:
    branches: [main]
    paths-ignore:
      - 'docs/**'
  pull_request:
    branches: [main]
";
        assert!(push_trigger(yaml).unwrap().path_filtered);
    }

    /// 다른 트리거의 필터는 `push:` 의 필터가 아니다. 깊이로 끊지 않으면 여기서 샌다.
    #[test]
    fn a_filter_on_another_trigger_does_not_leak_into_push() {
        let yaml = "\
on:
  push:
    branches: [main]
  pull_request:
    branches: [main]
    paths:
      - 'src/**'
";
        assert_eq!(
            push_trigger(yaml),
            Some(PushTrigger {
                present: true,
                path_filtered: false,
                tags_only: false
            })
        );
    }

    #[test]
    fn a_workflow_without_push_says_so() {
        let yaml = "on:\n  workflow_dispatch:\n";
        assert_eq!(
            push_trigger(yaml),
            Some(PushTrigger {
                present: false,
                path_filtered: false,
                tags_only: false
            })
        );
    }

    /// 태그 전용 트리거는 필터가 없어도 매 push 채널이 아니다.
    #[test]
    fn a_tags_only_push_is_not_an_every_push_channel() {
        let yaml = "on:\n  push:\n    tags: ['v*']\n  workflow_dispatch:\n";
        let t = push_trigger(yaml).unwrap();
        assert!(t.present && !t.path_filtered && t.tags_only);
    }

    /// 브랜치를 함께 걸면 태그 전용이 아니다.
    #[test]
    fn a_push_naming_branches_is_not_tags_only() {
        let yaml = "on:\n  push:\n    branches: [main]\n    tags: ['v*']\n";
        assert!(!push_trigger(yaml).unwrap().tags_only);
    }

    /// 못 읽는 모양은 통과가 아니라 판정 불가다.
    #[test]
    fn an_inline_trigger_is_unreadable_rather_than_unfiltered() {
        assert_eq!(push_trigger("on: [push]\njobs:\n"), None);
    }

    /// 값 안의 `#` 은 주석이 아니다. 잘라 내면 뒤따르는 키를 잃는다.
    #[test]
    fn a_hash_inside_a_value_is_not_a_comment() {
        let yaml = "\
on:
  push:
    branches: [main]
    paths:
      - 'a#b/**'
";
        assert!(push_trigger(yaml).unwrap().path_filtered);
    }
}
