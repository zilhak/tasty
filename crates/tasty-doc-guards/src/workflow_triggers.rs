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

/// **답하는 물음**: 이 워크플로의 `push:` 가 매 push 도는가 — 경로 필터가 붙었는가,
/// 태그 전용인가.
///
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

/// 경로 필터 없는 채널이 덮는 테스트 타깃의 집합.
///
/// **왜 한 벌인가**: 이 물음("필터 없는 채널이 이 타깃을 덮는가")에 답하는 것이 한때
/// 둘이었고, 둘이 **같은 항목에 다른 답**을 냈다. 실측(2026-09-05) — `doc-guards.yml` 의
/// 호출을 `--test` 하나로 좁히는 변이에서 `filter_free_channel_still_exists` 는 발화했고
/// `filtered_guards_are_not_totally_blind` 는 침묵했다. 뒤쪽이 `--test <이름>` 만 읽고
/// `-p <패키지>` 를 안 읽었기 때문이고, 그 구멍을 **디렉토리 이름 상수로 건너뛰기**로
/// 메우고 있었다. 이름으로 면제하면 성질이 같고 이름이 다른 것이 통째로 빠진다(ADR-0175).
///
/// 반대로 **합치지 않은 것도 있다.** `ci_channel_claims_match_workflows` 는 "자동 채널이
/// 있는가(언젠가라도)" 를 묻고 여기서는 "매 push 도는가" 를 묻는다. `test.yml` 에 필터를
/// 다는 변이에서 앞은 침묵하고 뒤는 발화하는데 **둘 다 자기 물음에는 맞다** — 그러면
/// 물음이 둘이라 사본도 둘이다.
#[derive(Debug, Default)]
pub struct FilterFreeCoverage {
    /// `--test <이름>` 으로 지목된 타깃 이름들.
    pub named: std::collections::BTreeSet<String>,
    /// `-p <패키지>` 로 **좁혀지지 않고** 불린 패키지들.
    pub packages: std::collections::BTreeSet<String>,
    /// `--workspace` 를 좁힘 없이 부르는 잡이 있는가 — 있으면 전부 덮인다.
    pub whole_workspace: bool,
}

/// 호출을 특정 타깃/종류로 좁히는 플래그.
const NARROWING: &[&str] = &["--test", "--lib", "--bins", "--bin", "--doc", "--example"];

/// 한 `cargo test` 호출이 타깃/종류로 좁혀졌는가.
///
/// 판정을 함수로 꺼낸 이유는 **부를 수 있게** 하기 위해서다. 목록이 순회 안에 인라인으로
/// 박혀 있으면 그 목록을 검사하려면 워크플로 디렉토리를 통째로 픽스처로 지어야 하고,
/// 그러면 아무도 안 짓는다 — 실측(2026-09-06): 이 목록에서 `--test` 를 지워도 이 크레이트
/// 전체가 초록이었는데, 그 플래그가 **유일한 좁힘 근거**인 자동 잡이 그때 둘 있었다.
fn narrows(inv: &str) -> bool {
    inv.split_whitespace().any(|w| NARROWING.contains(&w))
}

/// 주석·스텝 이름을 지우고 한 줄로 편다.
///
/// 스텝 이름을 지우는 이유는 이 레포의 스텝 이름이 명령을 그대로 쓰기 때문이다
/// (`- name: cargo test -p tasty-doc-guards`). 안 지우면 **이름이 채널로 읽혀**, 실행
/// 스텝이 좁혀졌는데도 안 좁혀진 옛 이름을 들고 있으면 통과한다.
///
/// 한 줄로 펴는 이유는 `run: |` 블록과 `run: >` 접힌 스칼라, 줄 끝 `\` 이음이 전부 한
/// 명령을 여러 줄에 나누기 때문이다. 줄 단위로 보면 `cargo test ... \` 에서 끊겨 뒤에
/// 오는 플래그를 놓친다 — **있는 좁힘을 없다고 판정하는 쪽이라 더 나쁘다.** 실측으로
/// 이 함정에 걸릴 뻔했다: `test.yml` 의 semver-guards 는 접힌 스칼라로 `--test` 셋을
/// 거는데, 줄 단위로 보면 좁혀지지 않은 전체 호출로 보인다.
fn flatten(yaml: &str) -> String {
    yaml.replace("\r\n", "\n")
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && !t.starts_with("- name:") && !t.starts_with("name:")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 잡 하나 — 헤더에 적힌 이름과, 그 헤더부터 다음 헤더 직전까지의 본문.
#[derive(Debug, Clone)]
pub struct JobSpan {
    /// `jobs:` 아래 2 칸 헤더에 적힌 이름(뒤의 `:` 는 뗐다).
    pub name: String,
    /// 헤더 줄을 **포함한** 본문. 주석이 그대로 남아 있다 — 아래 술어 설명 참조.
    pub body: String,
}

/// `jobs:` 아래의 잡을 이름과 본문으로 자른다 — **자동/수동을 안 가른다.**
///
/// 이 함수가 헤더 규칙의 **유일한 자리**다. 아래 넷([`job_headers`] ·
/// [`job_header_count`] · [`automatic_job_names`] · [`automatic_job_bodies`])이 전부
/// 여기서 나온다. 규칙을 두 자리에 쓰면 두 수의 차가 뜻을 잃는데 **갈렸는지를 보는 것이
/// 없다** — 한때 `workflow-channels` 가 자기 사본을 갖고 있었고 노출 판정기 대조는
/// `manual` 열이 정수인지만 봤다.
///
/// ★ **헤더는 주석을 뗀 사본에서 고르고, 본문은 원문을 담는다.** 두 사본을 쓰는 이유가
/// 각각 있다: 원문에서 헤더를 고르면 2 칸 들여쓰기에 `:` 로 끝나는 **주석 줄**이 잡으로
/// 세어진다(실측 2026-09-08: `crossplatform-check.yml` 의 `check-headless` 와
/// `check-release` 사이 주석 블록이 그 형태라 자동 잡이 4 가 아니라 5 로 나왔다 — 오차의
/// 방향이 언제나 **더 초록**이라 하한은 그것을 못 잡는다). 거꾸로 본문에서까지 주석을
/// 떼면 소비자가 "주석 안의 낱말을 명령으로 읽는가" 를 더는 못 묻는다 —
/// `guard_test_channels_stay_split` 의 `commands_only` 가 정확히 그것을 묻고, 그 물음은
/// 주석이 본문에 남아 있어야 성립한다. [`strip_yaml_comments`] 는 줄 수를 보존하므로 두
/// 사본의 줄이 1:1 로 맞는다.
///
/// **잡 헤더 판정은 2 칸 들여쓰기 관례에 매달려 있다.** 관례를 깨는 워크플로가 오면
/// 잡 헤더가 하나도 안 잡혀 파일 전체가 한 덩어리가 되고, 그러면 그 안의 수동 전용
/// 조건 하나가 **파일 전체의 호출을 통째로** 지운다 — 또 줄이는 방향이다.
/// 실측(2026-09-05): 레포의 워크플로 11 개가 전부 2 칸이라 지금은 안 걸린다.
///
/// `jobs:` 와 **첫 헤더 사이**의 줄은 어느 잡에도 안 넣는다. 이름 없는 덩어리를 잡으로
/// 세면 그 수가 늘고, 늘어나는 오차는 하한이 원리적으로 못 잡는다.
pub fn job_spans(yaml: &str) -> Vec<JobSpan> {
    let normalized = yaml.replace("\r\n", "\n");
    let stripped = strip_yaml_comments(&normalized);
    let mut out: Vec<JobSpan> = Vec::new();
    let mut in_jobs = false;
    for (line, bare) in normalized.lines().zip(stripped.lines()) {
        if bare.starts_with("jobs:") {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        let is_job_head =
            bare.starts_with("  ") && !bare.starts_with("   ") && bare.trim_end().ends_with(':');
        if is_job_head {
            out.push(JobSpan {
                name: bare.trim().trim_end_matches(':').to_string(),
                body: String::new(),
            });
        }
        if let Some(cur) = out.last_mut() {
            cur.body.push_str(line);
            cur.body.push('\n');
        }
    }
    out
}

/// 잡 이름 전부 — **자동/수동을 안 가린다.**
///
/// 파일 하나가 답을 낸다. 디렉토리 전체를 훑는 쪽이 이 값을 **파일마다** 받아야 부분
/// 실명(파일 셋만 안 읽혀도 합계는 하한 위에 남는 것)을 갈 수 있다.
pub fn job_headers(yaml: &str) -> Vec<String> {
    job_spans(yaml).into_iter().map(|j| j.name).collect()
}

/// [`job_headers`] 의 수. 이름이 필요 없는 자리에서 쓴다.
pub fn job_header_count(yaml: &str) -> usize {
    job_spans(yaml).len()
}

/// 수동 전용 잡인가 — 자동 채널에서 빼는 술어.
///
/// ★ **이 술어는 줄이는 방향으로 틀린다** — 그 방향의 오차는 언제나 더 초록이라
/// 안 보인다. 문자열이 들어 있기만 하면 버리므로, `if:` 가 **분리(∨)** 인 잡도
/// 수동 전용으로 센다. 실측(2026-09-05): `release.yml` 의 빌드 잡 4 개가 그 형태다
/// (`always() && (needs... == 'success' || event_name == 'workflow_dispatch')`) —
/// 태그 push 에서도 도는데 여기서 버려진다.
///
/// **오늘 그 누락의 효과는 0 이고, 그 0 을 쟀다**: `release.yml` 에는 `cargo test`
/// 호출이 하나도 없어서 커버리지에 기여할 것이 애초에 없고, 그 파일은 태그 전용이라
/// [`push_trigger`] 단계에서 이미 빠진다. 그래서 지금 고치지 않는다 — 다만 그 파일이
/// 언젠가 `cargo test` 를 들이면 **조용히** 안 보이게 된다. 그때는 술어를 "순수 조건
/// (`if: github.event_name == 'workflow_dispatch'`)일 때만 버린다" 로 좁혀라.
///
/// 같은 술어를 `ci_channel_claims_match_workflows` 도 쓴다. 답을 둘로 만들지 않으려고
/// 형태를 맞춰 둔 것이고, 위 한계도 그대로 공유한다.
fn is_manual_only(body: &str) -> bool {
    body.contains("github.event_name == 'workflow_dispatch'")
}

/// 자동 회차에 도는 잡의 **이름**.
///
/// [`job_headers`] 와의 차가 "수동 전용이라 빠진 잡" 이다. 두 값이 같은 [`job_spans`]
/// 에서 나오므로 그 차는 언제나 뜻을 갖는다.
pub fn automatic_job_names(yaml: &str) -> Vec<String> {
    job_spans(yaml)
        .into_iter()
        .filter(|j| !is_manual_only(&j.body))
        .map(|j| j.name)
        .collect()
}

/// `jobs:` 아래의 잡 본문 중 **자동 회차에 도는 것**.
///
/// 잡 단위로 갈라야 하는 이유: 워크플로에 필터가 없어도 그 안의 잡이 이벤트 조건으로
/// 수동 전용일 수 있다. 파일 단위로 보면 그 잡의 명령이 자동 채널로 읽힌다 — 실측으로
/// 이 함정에 걸렸다(2026-09-05): `test.yml` 은 필터가 없고 `cargo test --workspace` 를
/// 들고 있지만 그 잡은 수동 전용이라, 잡을 안 가른 첫 판에서 판정이 **잘못된 이유로**
/// 초록이었다.
/// **답하는 물음**: 이 워크플로의 잡 중 **자동 회차에 도는 것**은 어느 것인가.
/// (수동 전용 조건이 붙은 잡을 뺀 나머지. 트리거·경로필터는 [`push_trigger`] 가 본다.)
///
/// **밖에서 부를 수 있게 `pub` 이다.** 이 판정을 셸이나 일회용 스크립트로 흉내 내면
/// 갈린다 — 실측(2026-09-05): 이 레포에서 하루에 세 레인이 각자 미러를 만들었고 셋 다
/// 원본과 다른 답을 냈다. 갈리는 방향은 대체로 **덜 잡는 쪽**이라 조용하다.
/// 부르는 길은 `workflow-channels` 판정기 바이너리다.
pub fn automatic_job_bodies(yaml: &str) -> Vec<String> {
    job_spans(yaml)
        .into_iter()
        .filter(|j| !is_manual_only(&j.body))
        .map(|j| j.body)
        .collect()
}

/// 평탄화된 본문에서 `cargo test` 호출을 하나씩 잘라낸다. 각 조각은 다음 `cargo ` 직전
/// 까지라, 한 스텝에 명령이 여럿이어도 플래그가 섞이지 않는다.
fn cargo_test_invocations(flat: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = flat[from..].find("cargo test") {
        let start = from + rel;
        let rest = &flat[start + "cargo test".len()..];
        let end = rest
            .find("cargo ")
            .map_or(flat.len(), |n| start + "cargo test".len() + n);
        out.push(&flat[start..end]);
        from = start + "cargo test".len();
    }
    out
}

/// **답하는 물음**: 경로 필터 없이 매 push 도는 잡이 어느 테스트 타깃을 덮는가.
///
/// 워크플로 디렉토리 전체에서 [`FilterFreeCoverage`] 를 읽는다.
///
/// `on:` 을 못 읽는 워크플로가 있으면 `Err` 로 그 이름들을 낸다 — **판정 불가는 통과가
/// 아니다.** 모르는 것을 빈 집합으로 바꾸면 덮인 타깃을 안 덮인 것으로 세고, 그 방향의
/// 오답은 "이미 덮인 가드를 옮겨라" 라는 거짓 요구가 된다.
pub fn filter_free_coverage(
    workflows: &std::path::Path,
) -> Result<FilterFreeCoverage, Vec<String>> {
    let mut out = FilterFreeCoverage::default();
    let mut unreadable = Vec::new();
    let entries = match std::fs::read_dir(workflows) {
        Ok(e) => e,
        Err(e) => return Err(vec![format!("{}: {e}", workflows.display())]),
    };
    let mut paths: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "yml" || x == "yaml"))
        .collect();
    paths.sort();
    for path in paths {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let Ok(text) = std::fs::read_to_string(&path) else {
            unreadable.push(name);
            continue;
        };
        let Some(trigger) = push_trigger(&text) else {
            unreadable.push(name);
            continue;
        };
        if !trigger.present || trigger.path_filtered || trigger.tags_only {
            continue;
        }
        for body in automatic_job_bodies(&text) {
            let flat = flatten(&body);
            for inv in cargo_test_invocations(&flat) {
                let words: Vec<&str> = inv.split_whitespace().collect();
                for w in words.windows(2) {
                    if w[0] == "--test" {
                        out.named.insert(w[1].to_string());
                    }
                }
                if narrows(inv) {
                    continue;
                }
                for w in words.windows(2) {
                    if w[0] == "-p" || w[0] == "--package" {
                        out.packages.insert(w[1].to_string());
                    }
                }
                if words.contains(&"--workspace") {
                    out.whole_workspace = true;
                }
            }
        }
    }
    if unreadable.is_empty() {
        Ok(out)
    } else {
        Err(unreadable)
    }
}

impl FilterFreeCoverage {
    /// 이 타깃이 필터 없는 채널에 덮이는가. `stem` 은 `--test` 에 쓰는 타깃 이름,
    /// `package` 는 그 타깃이 속한 패키지 이름이다.
    pub fn covers(&self, stem: &str, package: &str) -> bool {
        self.whole_workspace || self.named.contains(stem) || self.packages.contains(package)
    }
}

#[cfg(test)]
mod coverage_tests {
    use super::*;

    fn coverage() -> FilterFreeCoverage {
        filter_free_coverage(&crate::repo_root().join(".github/workflows"))
            .unwrap_or_else(|bad| panic!("`on:` 을 못 읽은 워크플로: {bad:?}"))
    }

    /// 침묵 탐지. 판독이 통째로 비면 모든 타깃이 "안 덮임" 이 되고, 그 방향의 오답은
    /// 이미 덮인 가드에 "옮겨라" 를 요구한다.
    ///
    /// **한쪽만 비는 것은 고장이 아니다** — 워크플로가 `--test` 를 안 쓰거나 `-p` 를 안
    /// 쓰는 것은 정상 설정이다. 그 자리를 "수집이 깨졌다" 로 말하면 설정 변화에 틀린
    /// 진단이 붙는다(실측으로 그 오진을 냈다: `test.yml` 에 필터를 다는 변이에서
    /// `named` 가 비었고, 그건 판독이 아니라 채널이 사라진 것이었다). 내용은 아래 두
    /// 테스트가 각각 짚는다.
    #[test]
    fn the_reader_is_not_silently_empty() {
        let c = coverage();
        assert!(
            !c.named.is_empty() || !c.packages.is_empty() || c.whole_workspace,
            "필터 없는 채널을 하나도 못 읽었다 — 판독이 깨졌거나 채널이 전부 사라졌다"
        );
    }

    /// 접힌 스칼라(`>`)로 여러 줄에 걸친 `--test` 를 읽는가. 줄 단위로 보면 이 호출은
    /// 좁혀지지 않은 것으로 보이고, 그러면 워크스페이스 전체가 덮인 것으로 센다.
    #[test]
    fn a_folded_scalar_narrowing_is_seen() {
        let c = coverage();
        assert!(
            c.named.contains("changelog_unreleased"),
            "접힌 스칼라 안의 `--test changelog_unreleased` 를 못 읽었다: {:?}",
            c.named
        );
        assert!(
            !c.packages.contains("tasty"),
            "좁혀진 호출을 패키지 전체 채널로 셌다: {:?}",
            c.packages
        );
    }

    /// ★ **분할 규칙 자체를 시험한다 — 잡이 둘일 때만 시험된다.**
    ///
    /// 이 단정이 없던 동안 잡 헤더 규칙(2 칸 들여쓰기)을 3 칸으로 바꾸는 변이가
    /// **아무 테스트도 못 죽였다**(실측 2026-09-05). 이유는 단순하다 — 그때 있던
    /// 픽스처가 전부 **잡 하나짜리**였다. 잡이 하나면 헤더를 못 찾아 파일 전체가 한
    /// 덩어리가 돼도 개수가 1 로 같다. 규칙이 시험되려면 잡이 둘이어야 한다.
    ///
    /// 그리고 둘일 때 진짜 위험이 드러난다: 헤더를 못 찾으면 **수동 전용 잡 하나가
    /// 같은 파일의 자동 잡을 함께 지운다.** 아래가 그 형태다.
    #[test]
    fn two_jobs_split_and_a_manual_one_does_not_silence_the_automatic_one() {
        let yaml = "on:\n  push:\n    branches: [main]\njobs:\n  auto:\n    steps:\n      \
                    - run: cargo test -p alpha\n  manual:\n    if: github.event_name == \
                    'workflow_dispatch'\n    steps:\n      - run: cargo test -p beta\n";
        let bodies = automatic_job_bodies(yaml);
        assert_eq!(
            bodies.len(),
            1,
            "잡 둘 중 자동인 하나만 남아야 한다 — 0 이면 헤더를 못 찾아 파일이 한 덩어리가 \
             되고 수동 전용 조건이 자동 잡까지 지운 것이다: {bodies:?}"
        );
        assert!(
            bodies[0].contains("cargo test -p alpha"),
            "자동 잡의 명령이 사라졌다: {bodies:?}"
        );
        assert!(
            !bodies.iter().any(|b| b.contains("cargo test -p beta")),
            "수동 전용 잡의 명령이 자동 채널로 새어 들어왔다: {bodies:?}"
        );
    }

    /// ★ **2 칸 들여쓰기에 `:` 로 끝나는 주석 줄은 잡이 아니다.**
    ///
    /// 헤더를 원문에서 고르면 그런 주석이 잡으로 세어지고, 그 뒤 줄들이 **가짜 잡의
    /// 본문**이 된다. 방향이 나쁘다 — 잡 수가 **늘어나므로** 하한(`JOB_FLOOR` 류)은
    /// 언제나 통과하고, 그 초록은 "판독이 산다" 가 아니라 "판독이 헛것을 하나 더 셌다"
    /// 다. 실측 2026-09-08 에 레포에 그 형태가 실재했다(`crossplatform-check.yml` 의
    /// `check-headless` 와 `check-release` 사이 주석 블록) — 자동 잡이 4 가 아니라 5 로
    /// 나왔고, 그 값이 `JOB_FLOOR.measured` 에 20 으로 적혀 있었다(참값 19).
    ///
    /// 본문 쪽은 반대 방향으로 지킨다: 반환하는 본문에는 주석이 **남아 있어야** 한다.
    /// 아래 둘째 단정이 그것이다 — 소비자(`commands_only` 류)가 "주석 안의 낱말을
    /// 명령으로 읽는가" 를 물을 수 있는 것은 그 덕분이다.
    #[test]
    fn a_comment_shaped_like_a_job_header_is_not_a_job() {
        let yaml = "on:\n  push:\n    branches: [main]\njobs:\n  alpha:\n    steps:\n      \
                    - run: cargo test -p alpha\n\n  # 아래 잡이 무엇을 닫는지 적는다:\n  \
                    #  - 하나\n  beta:\n    steps:\n      - run: cargo test -p beta\n";
        let bodies = automatic_job_bodies(yaml);
        assert_eq!(
            bodies.len(),
            2,
            "잡은 둘인데 {}개로 셌다 — 주석 줄을 헤더로 읽으면 늘고, 그 오차는 하한을 \
             통과하는 방향이라 안 보인다: {bodies:?}",
            bodies.len()
        );
        assert!(
            bodies.iter().any(|b| b.contains("아래 잡이 무엇을 닫는지")),
            "본문에서 주석이 사라졌다 — 소비자가 '주석 속 낱말을 명령으로 읽는가' 를 더는 \
             못 묻는다: {bodies:?}"
        );
        assert_eq!(
            job_header_count(yaml),
            2,
            "잡 헤더 수도 같은 규칙이어야 한다 — 갈리면 `전체 − 자동` 이 뜻을 잃는다"
        );
    }

    /// 레포에서도 같은 것을 묻는다 — 픽스처만으로는 관례가 실제로 그러한지 모른다.
    ///
    /// 잡이 여럿인 워크플로가 실재하므로 **잡 본문 총수 > 워크플로 파일 수** 여야 한다.
    /// 헤더 규칙이 깨지면 파일마다 최대 하나가 되어 이 부등식이 무너진다.
    #[test]
    fn the_repo_has_more_automatic_job_bodies_than_workflow_files() {
        let dir = crate::repo_root().join(".github/workflows");
        let mut files = 0usize;
        let mut bodies = 0usize;
        for e in std::fs::read_dir(&dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .flatten()
        {
            let p = e.path();
            if !p.extension().is_some_and(|x| x == "yml" || x == "yaml") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            files += 1;
            bodies += automatic_job_bodies(&text).len();
        }
        assert!(
            files >= 8,
            "워크플로를 {files} 개밖에 못 읽었다 — 모수가 깨졌다"
        );
        assert!(
            bodies > files,
            "잡 본문 {bodies} 개 ≤ 워크플로 {files} 개 — 잡이 여럿인 워크플로가 실재하는데 \
             파일마다 하나 이하로 나왔다. 헤더 규칙이 깨져 파일이 한 덩어리가 된 것이다"
        );
    }

    /// 좁힘 없이 패키지를 부르는 잡이 있다 — 그것이 문서 가드들의 채널이다.
    ///
    /// ★ **양쪽을 함께 묻는다.** "덮인다" 만 재면 [`FilterFreeCoverage::covers`] 가 항상
    /// 참을 내도 통과한다 — 그리고 그 고장은 이 판독을 쓰는 모든 가드를 한꺼번에
    /// 무력화한다(덮였다고 답하면 사각이 0 이 되어 전부 초록이다). 한 방향만 재면
    /// 무정보다.
    #[test]
    fn the_coverage_answers_both_yes_and_no() {
        let c = coverage();
        assert!(
            c.covers("no_checkbox_in_docs", "tasty-doc-guards"),
            "`tasty-doc-guards` 가 필터 없는 채널에 안 덮인다: {c:?}"
        );
        assert!(
            !c.covers("a_target_no_workflow_names", "tasty-plugin-markdown"),
            "아무 워크플로도 이름으로 부르지 않고 그 패키지를 좁힘 없이 돌리지도 않는데 \
             덮였다고 답했다. 판독이 한쪽으로만 답하는 상태이거나, 필터 없는 잡이 새로 \
             `--workspace` 를 돌기 시작한 것이다(그러면 이 판독을 쓰는 사각 탐지가 전부 \
             공허해지므로 그 자리에서 다시 판단해야 한다): {c:?}"
        );
    }

    /// [`NARROWING`] 의 성분마다, 그 성분 **하나만** 든 호출이 좁힘으로 읽히는가.
    ///
    /// 조각을 상수에서 만들지 않고 **손으로 적는다.** 목록을 순회해 조각을 지으면 오타 난
    /// 항목(`--tesst`)도 자기 자신과는 맞아 통과한다 — 그러면 이 테스트가 목록의 사본이 될
    /// 뿐 목록을 검사하지 않는다.
    ///
    /// 조각마다 성분을 **하나만** 담는다. 둘을 담으면 하나를 지워도 다른 하나가 받쳐 주어
    /// 그 지움이 조용해진다.
    ///
    /// 실측 2026-09-06: 이 테스트가 없을 때 `NARROWING` 에서 `--test` 를 지워도
    /// `cargo test -p tasty-doc-guards` 는 초록이었다(rc=0). 레포 판정이 안 죽는 이유는
    /// 좁힘 판정이 **느슨해지는 방향**이라 위반 목록이 비기 때문이다 — 하한은 순회가
    /// 죽는 방향만 본다. 그때 `--test` 가 유일한 좁힘 근거인 자동 잡은 둘이었다.
    #[test]
    fn every_narrowing_flag_is_actually_read_as_narrowing() {
        let cases: [(&str, &str); 6] = [
            ("--test", "cargo test --workspace --locked --test e2e_tests"),
            ("--lib", "cargo test --workspace --locked --lib"),
            ("--bins", "cargo test --workspace --locked --bins"),
            ("--bin", "cargo test --workspace --locked --bin tasty"),
            ("--doc", "cargo test --workspace --locked --doc"),
            (
                "--example",
                "cargo test --workspace --locked --example demo",
            ),
        ];
        for (flag, inv) in cases {
            assert!(
                narrows(inv),
                "`{flag}` 하나로 좁힌 호출을 좁힘으로 안 읽는다. 그 성분이 목록에서 \
                 빠졌거나 철자가 틀렸다 — 그러면 그 플래그로만 좁힌 자동 잡이 **전체 \
                 스위트**로 읽히고, 이 판독을 쓰는 사각 탐지가 그 잡을 통째로 잘못 센다"
            );
        }
    }

    /// 음성 대조 — 좁힘이 없는 호출을 좁힘으로 읽으면 위 판정이 공허해진다.
    #[test]
    fn an_unnarrowed_invocation_is_not_read_as_narrowed() {
        assert!(
            !narrows("cargo test --workspace --locked --no-fail-fast"),
            "좁힘 플래그가 없는데 좁혀졌다고 읽었다 — 이 술어가 늘 참을 내면 필터 없는 \
             채널이 하나도 안 세어지고, 모든 타깃이 '안 덮였다' 로 뒤집힌다"
        );
        assert!(
            !narrows("cargo test --workspace --locked -p tasty-doc-guards"),
            "`-p` 는 패키지 선택이지 타깃 좁힘이 아니다 — 좁힘으로 읽으면 그 패키지를 \
             통째로 도는 잡이 커버리지에서 빠진다"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  스텝 단위 판독 — 잡 결론이 안 말하는 것
// ─────────────────────────────────────────────────────────────────────────────

/// 워크플로 한 잡 안의 스텝 하나.
///
/// **왜 잡이 아니라 스텝인가.** GitHub Actions 의 스텝 기본 조건은 `success()` 라,
/// 앞 스텝이 죽으면 뒤 스텝은 **안 돌고 `skipped` 로 끝난다.** 그때 뒤 스텝이 배선한
/// 조합은 실패가 아니라 **미측정**인데, 잡 결론은 그 구분을 안 말한다 — 잡은 그냥
/// 빨갛고, 그 안에서 무엇이 돌았고 무엇이 사라졌는지는 스텝 줄에만 있다.
///
/// 실측(2026-09-07 기준 최근 30 실행): `crossplatform-check` 의 `check-windows` 에서
/// 세 회차(`34008068587` · `34111807266` · `34135280936`)에 앞 스텝이 죽어 그 뒤의
/// `cargo test (unit)` · `cargo test (windows integration — shm · doc-guards)` 가
/// 통째로 `skipped` 였다. 같은 형태가 `check-headless` 에도 있었고 그쪽만 처방이
/// 들어갔다(`if: ${{ !cancelled() }}`).
#[derive(Debug, Clone)]
pub struct Step {
    /// 이 스텝이 속한 잡 이름.
    pub job: String,
    /// `- name:` 이나 `- uses:` 에 적힌 글자.
    pub name: String,
    /// 파일 기준 줄 번호(1-based). 주석을 지운 사본에서도 줄이 보존되므로 원문과 같다.
    pub line: usize,
    /// 잡 안 순번(1-based).
    pub ordinal: usize,
    /// `if:` 가 붙었는가. 붙으면 기본 `success()` 조건이 **대체**되어 앞이 죽어도 돈다.
    pub has_if: bool,
    /// `continue-on-error: true` 인가. 붙으면 이 스텝의 실패가 잡 결론에 안 들어간다.
    pub continue_on_error: bool,
    /// 스텝 헤더 다음부터 다음 스텝 직전까지의 줄들.
    pub body: String,
}

/// 잡별로 스텝을 갈라 낸다.
///
/// **`automatic_job_bodies` 와 같은 관례에 매달린다** — 잡 헤더는 2 칸, 스텝 헤더는
/// 6 칸(`      - name:`)이다. 관례를 깨는 워크플로가 오면 스텝이 하나도 안 잡히고,
/// 그 방향의 오답은 **덜 잡는 쪽**이라 조용하다. 그래서 이 판독을 쓰는 쪽은 좌변이
/// 0 이 되는 것을 통과가 아니라 판정 불가로 다뤄야 한다.
///
/// 주석은 [`strip_yaml_comments`] 로 지우고 센다 — 그 함수가 줄을 보존하므로 줄
/// 번호는 원문과 같다. 주석 안의 `cargo test` 나 `if:` 를 세면 다른 잡의 설정을
/// *설명하는* 주석이 그 잡의 설정으로 읽힌다.
pub fn job_steps(yaml: &str) -> Vec<Step> {
    let stripped = strip_yaml_comments(&yaml.replace("\r\n", "\n"));
    let mut out: Vec<Step> = Vec::new();
    let mut in_jobs = false;
    let mut job = String::new();
    let mut ordinal = 0usize;
    for (i, line) in stripped.lines().enumerate() {
        let no = i + 1;
        if line.starts_with("jobs:") {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        let is_job_head =
            line.starts_with("  ") && !line.starts_with("   ") && line.trim_end().ends_with(':');
        if is_job_head {
            job = line.trim().trim_end_matches(':').to_string();
            ordinal = 0;
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("      - name:")
            .or_else(|| line.strip_prefix("      - uses:"))
        {
            ordinal += 1;
            out.push(Step {
                job: job.clone(),
                name: rest.trim().to_string(),
                line: no,
                ordinal,
                has_if: false,
                continue_on_error: false,
                body: String::new(),
            });
            continue;
        }
        if let Some(step) = out.last_mut() {
            if line.starts_with("        if:") {
                step.has_if = true;
            }
            if line.trim() == "continue-on-error: true" {
                step.continue_on_error = true;
            }
            step.body.push_str(line);
            step.body.push('\n');
        }
    }
    out
}

/// 이 스텝이 검증을 돌리는가 — `cargo test` 또는 `cargo clippy`.
fn runs_verification(step: &Step) -> bool {
    let flat = step.body.replace('\n', " ");
    let mut words = flat.split_whitespace().peekable();
    while let Some(w) = words.next() {
        if w == "cargo" && matches!(words.peek(), Some(&"test") | Some(&"clippy")) {
            return true;
        }
    }
    false
}

/// **앞선 cargo 스텝이 죽으면 조용히 사라지는 검증 스텝.**
///
/// 술어는 셋을 모두 만족하는 것이다:
/// 1. 자기가 `cargo test`/`cargo clippy` 를 돌린다,
/// 2. 같은 잡에 자기보다 **앞선 스텝이 하나라도** 있다,
/// 3. 자기에게 `if:` 도 `continue-on-error:` 도 없다.
///
/// ★ **2 번의 모수가 2026-09-08 에 넓어졌다.** 처음에는 "앞선 **cargo** 스텝" 으로
/// 좁혀 놓고, 앞에 cargo 가 없는 자리는 "setup 이 죽는 것이고 그때는 잡 결론이 빨개져
/// 잡 단위 판독으로 보인다" 고 적었다. **그 논거가 틀렸다** — 잡 결론이 빨개지는 것과
/// 그 잡 안에서 무엇이 사라졌는지는 다르고, 후자가 안 보인다는 것이 이 층의 정의
/// 자체다(ADR-0230 ㄴ 층). 좁은 술어는 자기가 세운 층의 정의와 모순됐다.
///
/// 실측이 반증했다: run `34111807266` 에서 `check-windows` 의
/// `normalize the working tree to committed line endings`(git 명령, cargo 아님)가 죽자
/// `cargo clippy` 가 통째로 `skipped` 였다. 좁은 술어는 그 자리를 **안 센다** —
/// `cargo clippy` 앞에 cargo 스텝이 하나도 없기 때문이다.
///
/// 같은 트리에서 두 술어의 값(2026-09-08): 좁은 것 **3** · 넓은 것 **7**. 차이 넷은
/// `check-windows` 의 `cargo clippy` · `doc-guards` 의 `cargo test -p tasty-doc-guards` ·
/// `test.yml` 의 `cargo test (semver guards)` 와 `Build tests` 다.
///
/// `continue-on-error` 를 제외하는 이유는 그 스텝이 안전해서가 아니라 **다른 병**이기
/// 때문이다 — 그쪽은 자기 실패가 잡 결론에 안 들어가는 것이고, 실측 실패는 0 건이다
/// (최근 30 실행: success 26 · failure 0 · skipped 4). 두 축을 한 수로 합치면 어느
/// 쪽이 움직였는지 못 읽는다.
pub fn swallowable_verification_steps(yaml: &str) -> Vec<Step> {
    let steps = job_steps(yaml);
    let mut out = Vec::new();
    for (k, s) in steps.iter().enumerate() {
        if !runs_verification(s) || s.has_if || s.continue_on_error {
            continue;
        }
        if steps[..k].iter().any(|p| p.job == s.job) {
            out.push(s.clone());
        }
    }
    out
}

/// [`swallowable_verification_steps`] 의 **짝** — `if:` 로 앞 스텝의 죽음에서 떼어 둔
/// 검증 스텝.
///
/// 둘을 함께 세야 값의 움직임이 읽힌다: 처방이 들어가면 저쪽이 줄고 이쪽이 늘며,
/// 스텝이 통째로 사라지면 저쪽만 준다. 한 수만 못박으면 그 둘이 같은 모양으로 보인다.
///
/// **정리·배포 스텝은 안 센다.** `if: always()` 가 붙은 `Clean up dist` 나
/// `if: github.event_name == 'push'` 인 업로드도 앞의 죽음과 무관하게 돌지만, 그것들은
/// 검증을 배선하지 않아 사라져도 미측정을 만들지 않는다. 실측(2026-09-08): `if:` 를
/// 가진 스텝은 레포 전체에 18 이고 그중 검증은 **2** 다 — 넓은 술어로 못박으면 릴리스
/// 워크플로의 정리 스텝을 고칠 때마다 이 판정이 이유 없이 죽는다.
pub fn protected_verification_steps(yaml: &str) -> Vec<Step> {
    job_steps(yaml)
        .into_iter()
        .filter(|s| s.has_if && runs_verification(s))
        .collect()
}

#[cfg(test)]
mod step_tests {
    use super::*;

    /// 술어의 성분 하나하나를 손으로 쓴 픽스처로 고정한다.
    ///
    /// 픽스처를 **손으로 쓰는 이유**: 이 판독의 상수나 레포의 워크플로에서 뽑아 만들면
    /// 그 자리에 대한 항진명제가 된다(R1078). 아래 YAML 은 실제 워크플로의 *형태*를
    /// 본떴을 뿐 그 파일에서 읽어 오지 않는다.
    const FIXTURE: &str = r#"
name: fixture
on:
  push:
jobs:
  first:
    steps:
      - uses: actions/checkout@v4
      - name: cargo check
        run: cargo check --workspace --locked
      - name: swallowable
        run: cargo test --workspace --locked
      - name: protected
        if: ${{ !cancelled() }}
        run: cargo test --workspace --lib
      - name: swallowed failure is invisible
        continue-on-error: true
        run: cargo test --workspace --doc
  second:
    steps:
      - name: the first step of its job
        run: cargo test -p something
"#;

    /// 침묵 탐지가 먼저다 — 스텝을 하나도 못 뽑으면 아래 판정이 전부 공허하게 초록이 된다.
    #[test]
    fn the_step_reader_finds_steps_at_all() {
        let steps = job_steps(FIXTURE);
        assert_eq!(
            steps.len(),
            6,
            "픽스처에서 스텝을 {}개 뽑았다(기대 6) — 스텝 헤더 판정이 깨졌다. \
             이 판독은 6 칸 들여쓰기 관례에 매달리고, 깨지면 **덜 잡는 쪽**으로 \
             틀려서 조용하다",
            steps.len()
        );
        let jobs: Vec<&str> = steps.iter().map(|s| s.job.as_str()).collect();
        assert!(
            jobs.contains(&"first") && jobs.contains(&"second"),
            "잡 귀속이 깨졌다: {jobs:?} — 잡을 못 가르면 다른 잡의 앞 스텝이 \
             이 잡의 앞 스텝으로 읽혀 좌변이 부푼다"
        );
    }

    /// 술어의 네 성분을 한 번에 가른다. 넷 중 **하나만** 걸려야 한다.
    #[test]
    fn only_a_step_a_dead_neighbour_can_swallow_is_counted() {
        let got: Vec<String> = swallowable_verification_steps(FIXTURE)
            .iter()
            .map(|s| s.name.trim().to_string())
            .collect();
        assert_eq!(
            got,
            vec!["swallowable".to_string()],
            "술어가 고르는 것이 달라졌다: {got:?}\n  \
             기대는 `swallowable` 하나다 — 나머지 셋이 빠지는 이유가 각각 다르다:\n  \
             · `protected` 는 `if:` 가 기본 `success()` 를 대체해 앞이 죽어도 돈다\n  \
             · `continue-on-error` 짜리는 **다른 병**이다(자기 실패가 잡 결론에 안 \
             들어간다). 한 수로 합치면 어느 축이 움직였는지 못 읽는다\n  \
             · `second` 잡의 것은 **잡의 첫 스텝**이라 삼킬 앞이 아예 없다"
        );
    }

    /// 짝 술어 — 같은 픽스처에서 보호된 검증 스텝은 `protected` 하나다.
    ///
    /// 두 술어가 같은 스텝을 동시에 고르면 두 수의 합이 검증 스텝 수를 넘어 값의
    /// 움직임을 못 읽는다. 여기서 그 배타를 고정한다.
    #[test]
    fn the_protected_and_the_swallowable_never_hold_the_same_step() {
        let prot: Vec<String> = protected_verification_steps(FIXTURE)
            .iter()
            .map(|s| s.name.trim().to_string())
            .collect();
        assert_eq!(
            prot,
            vec!["protected".to_string()],
            "짝 술어가 고르는 것이 달라졌다: {prot:?} — 기대는 `protected` 하나다"
        );
        let swal: Vec<String> = swallowable_verification_steps(FIXTURE)
            .iter()
            .map(|s| s.name.trim().to_string())
            .collect();
        for p in &prot {
            assert!(
                !swal.contains(p),
                "`{p}` 가 두 술어에 다 들어간다 — 두 수를 함께 읽는 전제가 깨진다"
            );
        }
    }

    /// ★ 이 판독은 **주석을 안 센다** — 그리고 그 차이가 실제로 값을 바꿨다.
    ///
    /// 실측(2026-09-08): 같은 물음을 주석을 안 지운 일회용 스크립트로 세니 **7**,
    /// 이 판독으로 세니 **6** 이었다. 차이는 `crossplatform-check.yml` 의 `fd budget`
    /// 스텝 하나였고, 그 스텝 본문에는 `cargo test` 가 없다 — 뒤따르는 스텝을
    /// *설명하는* 주석에 그 글자가 있었을 뿐이다. 세는 사본이 다르면 같은 이름의
    /// 술어가 다른 수를 낸다.
    #[test]
    fn a_cargo_invocation_that_lives_only_in_a_comment_is_not_a_step_that_runs_it() {
        const COMMENTED: &str = r#"
jobs:
  only:
    steps:
      - uses: actions/checkout@v4
      - name: cargo check
        run: cargo check --locked
      - name: not a verification step
        run: echo hello
      # 아래 스텝은 cargo test --workspace 를 돌린다 — 이 줄은 주석이다
      - name: the real one
        run: cargo test --workspace
"#;
        let picked: Vec<String> = swallowable_verification_steps(COMMENTED)
            .iter()
            .map(|s| s.name.trim().to_string())
            .collect();
        assert_eq!(
            picked,
            vec!["the real one".to_string()],
            "주석 안의 `cargo test` 를 스텝이 그것을 돌리는 것으로 읽었다: {picked:?} — \
             그러면 좌변이 부풀고, 부푼 자리에 `if:` 를 달라는 처방이 붙는다. \
             그 처방은 실재하지 않는 위반에 대한 것이다"
        );
    }
}
