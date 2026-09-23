//! Markdown 링크의 문서 내·문서 간 앵커를 검사한다.
//!
//! 제목에서 만든 슬러그와 본문의 명시 HTML 앵커를 대상 집합으로 사용한다.
//! 코드 블록과 인라인 코드에 든 앵커 예시는 대상이 아니다.
//! `site/content/`를 가리키는 링크는 사이트의 실제 HTML 링크 검사에 맡긴다.
//! 그 검사가 CI에 연결되어 있는지도 별도로 확인한다.
//!
//! 외부 URL, 참조식 링크, Markdown이 아닌 대상은 여기서 검사하지 않는다.
//! 없는 파일은 `cited_coordinates_exist`가 검사한다.
//! 경로는 슬래시로 정규화하며 앵커 규칙은 `docs/documentation-model.md`를 따른다.

// 이유: 이 파일은 합성 트리를 만들어 순회를 재는 양성 대조를 갖는다. 그 정리
// 코드(`let _ = remove_dir_all`)는 실패해도 할 일이 없다 — 이전 실행 잔여물이 없으면
// `NotFound` 가 정상 경로다. 그리고 전수 가드
// (`crates/tasty-doc-guards/tests/let_underscore_documented.rs`)는 이 lint 의 출력을
// 프로덕션 명부로 쓰므로 테스트 자리가 거기 섞이면 새 프로덕션 자리가 묻힌다 —
// `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

/// 저장소 Markdown 순회가 빈 목록이나 크게 누락된 목록으로 통과하지 않게 한다.
/// 점 디렉터리, 빌드 캐시, 의존성 디렉터리와 심볼릭 링크는 순회에서 제외한다.
const MD_FLOOR: Floor = Floor {
    min: 215,
    measured: 267,
    measured_on: "2026-09-24",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::Tree("e87dcea71"),
    why_this_gap: "추적 Markdown은 267개다. 로컬 전용 문서를 포함한 실제 순회는 268개였다. \
                   하한은 로컬 파일에 의존하지 않으며 가장 큰 비-ADR 문서 분류인 \
                   docs/features의 52개만큼 여유를 둔다. 검사 범위가 바뀌면 다시 측정한다.",
};

/// 이 축에서 판정한 앵커 참조가 이보다 적으면 검출기가 죽은 것으로 본다.
///
/// 하한을 문서 수와 **따로** 두는 이유: 순회가 멀쩡해도 링크 스캐너가 죽으면 위반 0 이
/// 나온다. 두 하한이 서로 다른 사고를 막는다.
///
/// 실측 2026-09-08: 160 건(문서내 33 · 크로스파일 127). 두 축의 원시 계수와 어긋나지
/// 않는다 — 레포 `.md` 전체의 원시 계수는 문서내 39 · 크로스파일 157 이고, 그 차 6 과
/// 30 이 정확히 `site/content/` **안을 가리키는** 링크다(렌더러 소관이라 판정에서 뺀다).
const REFS_FLOOR: usize = 120;

/// 사이트 판사가 앵커의 주인인 트리 — **가리켜지는 쪽**을 기준으로 판정에서 뺀다.
/// 그 판사가 실재하는지는 [`the_site_anchor_judge_is_still_wired`] 가 본다.
const RENDERER_OWNED_PREFIX: &str = "site/content/";

/// 헤딩 텍스트에서 GitHub 스타일 슬러그를 만든다.
///
/// 링크는 **표시 텍스트만** 남긴다(헤딩이 링크를 품는 자리가 실제로 있다). 강조·인라인
/// 코드 마커를 지우고, 단어 문자와 하이픈과 공백만 남긴 뒤 공백을 하이픈으로 바꾼다.
/// 한글은 단어 문자라 그대로 남는다.
fn slug(text: &str) -> String {
    let kept: String = link_text_only(text)
        .chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || *c == ' ')
        .collect();
    kept.trim().to_lowercase().replace(' ', "-")
}

/// 헤딩 텍스트에서 마크업을 걷어 낸다 — 두 슬러그 규칙이 **공유하는 전처리**다.
///
/// 여기까지는 같고, 갈리는 것은 그 다음 글자 규칙뿐이다. 전처리를 한 벌로 두는 이유가
/// 그것이다: 두 벌이면 갈림을 잴 때 전처리 차이가 규칙 차이로 섞여 들어온다.
fn link_text_only(text: &str) -> String {
    let mut s = String::with_capacity(text.len());
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    // `[표시](대상)` → `표시`
    while i < bytes.len() {
        if bytes[i] == '['
            && let Some(close) = (i + 1..bytes.len()).find(|&j| bytes[j] == ']')
            && bytes.get(close + 1) == Some(&'(')
            && let Some(end) = (close + 2..bytes.len()).find(|&j| bytes[j] == ')')
        {
            s.extend(&bytes[i + 1..close]);
            i = end + 1;
            continue;
        }
        s.push(bytes[i]);
        i += 1;
    }
    s.replace("**", "").replace(['`', '*'], "")
}

/// 인라인 코드 스팬을 지운다 — 백틱 안의 `](#x)` 는 링크가 아니라 예시다.
fn without_inline_code(mut line: &str) -> String {
    let mut out = String::new();
    while let Some(at) = line.find('`') {
        out.push_str(&line[..at]);
        let run = line[at..].bytes().take_while(|b| *b == b'`').count();
        let after = &line[at + run..];
        let mut scan = after;
        let mut closed = false;
        while let Some(next) = scan.find('`') {
            let closing = scan[next..].bytes().take_while(|b| *b == b'`').count();
            scan = &scan[next + closing..];
            if closing == run {
                line = scan;
                closed = true;
                break;
            }
        }
        if !closed {
            out.push_str(&line[at..at + run]);
            line = after;
        }
    }
    out.push_str(line);
    out
}

/// 코드펜스 밖의 줄만 (1-based 줄번호와 함께) 낸다.
fn prose_lines(contents: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut fence: Option<(u8, usize)> = None;
    for (i, line) in contents.lines().enumerate() {
        let text = line.trim_start();
        let marker = text.as_bytes().first().copied().unwrap_or_default();
        let count = text.bytes().take_while(|b| *b == marker).count();
        if let Some((opened, width)) = fence {
            if marker == opened && count >= width && text[count..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if matches!(marker, b'`' | b'~') && count >= 3 {
            fence = Some((marker, count));
        } else {
            out.push((i + 1, line));
        }
    }
    out
}

/// 그 문서가 **제공하는** 앵커 전부. 같은 텍스트가 반복되면 `-1`·`-2` 가 순서대로 붙는다.
fn anchors_of(contents: &str) -> HashSet<String> {
    anchors_with(contents, slug)
}

/// 주어진 슬러그 규칙으로 그 문서의 앵커 집합을 만든다. 중복 텍스트에는 `-1`·`-2` 가
/// **문서 순서로** 붙는다 — 두 규칙 모두 같은 방식이라 접미사는 갈림의 원인이 아니다.
fn anchors_with(contents: &str, rule: fn(&str) -> String) -> HashSet<String> {
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();
    let mut out = explicit_html_anchors(contents);
    for (_, line) in prose_lines(contents) {
        let Some(rest) = line.strip_prefix('#') else {
            continue;
        };
        let rest = rest.trim_start_matches('#');
        let Some(text) = rest.strip_prefix(' ') else {
            continue;
        };
        let base = rule(text);
        if base.is_empty() {
            continue;
        }
        let n = seen.entry(base.clone()).or_insert(0);
        let id = if *n == 0 {
            base.clone()
        } else {
            format!("{base}-{n}")
        };
        *n += 1;
        out.insert(id);
    }
    out
}

/// 코드 예시와 HTML 주석 밖의 시작 태그에서 id와 a의 name을 읽는다.
/// 저장소에서 쓰는 한 줄 태그를 지원한다. 여러 줄 태그와 HTML entity 해석은 하지 않는다.
fn explicit_html_anchors(contents: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut in_comment = false;
    for (_, line) in prose_lines(contents) {
        if line.starts_with("    ") || line.starts_with('\t') {
            continue;
        }
        let line = without_inline_code(line);
        let mut rest = line.as_str();
        while !rest.is_empty() {
            if in_comment {
                let Some((_, after)) = rest.split_once("-->") else {
                    break;
                };
                rest = after;
                in_comment = false;
                continue;
            }
            let Some((before, after)) = rest.split_once('<') else {
                break;
            };
            if before.bytes().rev().take_while(|b| *b == b'\\').count() % 2 == 1 {
                rest = after;
                continue;
            }
            if let Some(after) = after.strip_prefix("!--") {
                in_comment = true;
                rest = after;
                continue;
            }
            let Some(end) = html_tag_end(after) else {
                break;
            };
            let tag = &after[..end];
            rest = &after[end + 1..];
            let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
            let name = &tag[..name_end];
            if !name.chars().next().is_some_and(|c| c.is_ascii_alphabetic()) {
                continue;
            }
            for (key, value) in html_attributes(&tag[name_end..]) {
                if (key.eq_ignore_ascii_case("id")
                    || (name.eq_ignore_ascii_case("a") && key.eq_ignore_ascii_case("name")))
                    && !value.is_empty()
                {
                    out.insert(value.to_string());
                }
            }
        }
    }
    out
}

/// 따옴표로 감싼 속성값 안의 >는 태그를 닫지 않는다.
fn html_tag_end(text: &str) -> Option<usize> {
    let mut quote = None;
    for (at, c) in text.char_indices() {
        match (quote, c) {
            (Some(open), c) if open == c => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '>') => return Some(at),
            _ => {}
        }
    }
    None
}

/// 속성값 전체를 먼저 소비하므로 title 안에 든 id= 예시를 앵커로 오해하지 않는다.
fn html_attributes(mut rest: &str) -> Vec<(&str, &str)> {
    let mut out = Vec::new();
    loop {
        rest = rest.trim_start();
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '=')
            .unwrap_or(rest.len());
        let key = &rest[..end];
        if key.is_empty() {
            break;
        }
        rest = rest[end..].trim_start();
        let Some(after) = rest.strip_prefix('=') else {
            continue;
        };
        rest = after.trim_start();
        let Some(first) = rest.chars().next() else {
            break;
        };
        let value;
        if first == '\'' || first == '"' {
            let Some((quoted, after)) = rest[1..].split_once(first) else {
                break;
            };
            value = quoted;
            rest = after;
        } else {
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            value = &rest[..end];
            rest = &rest[end..];
        }
        out.push((key, value));
    }
    out
}

#[test]
fn explicit_anchors_resolve_but_examples_and_unrelated_attributes_do_not() {
    let body = r##"<a id="old-heading"></a>
<a name = 'legacy'></a><div id=section></div>
<a title="id='pretend'" data-id="wrong"></a>
<a title='><a id="inside-string">'></a>
\<a id="escaped"></a>
    <a id="indented-code"></a>
`<a id="inline"></a>`
``example ` <a id="long-inline"></a>``
~~~html
<a id="tilde-fenced"></a>
~~~
````html
```
<a id="long-fenced"></a>
````
<!-- <a id="comment"></a>
<a id="comment-next-line"></a> -->
```html
<a id="fenced"></a>
```
# Heading
[valid](#old-heading) [legacy](#legacy) [missing](#absent)
"##;
    assert_eq!(
        anchors_of(body),
        HashSet::from_iter(["old-heading", "legacy", "section", "heading"].map(str::to_string))
    );
    let corpus = Corpus::from_pairs(&[
        ("docs/a.md", body),
        ("docs/b.md", "[cross](a.md#old-heading)"),
    ]);
    let checked = audit(&corpus, RENDERER_OWNED_PREFIX);
    assert_eq!(checked.violations.len(), 1, "{:?}", checked.violations);
    assert!(checked.violations[0].contains("absent"));
}

/// 그 문서가 **인용하는** 링크 대상 전부 — `(줄번호, 대상)`.
fn links_of(contents: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (no, line) in prose_lines(contents) {
        let line = without_inline_code(line);
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        while i + 1 < chars.len() {
            if chars[i] == ']' && chars[i + 1] == '(' {
                let mut j = i + 2;
                while j < chars.len() && chars[j] != ')' && !chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && chars[j] == ')' {
                    out.push((no, chars[i + 2..j].iter().collect::<String>()));
                }
                i = j.max(i + 1);
                continue;
            }
            i += 1;
        }
    }
    out
}

/// 대상 문자열을 `(파일, 앵커)` 로 가른다. 앵커가 없거나 우리가 답할 물음이 아니면 `None`.
fn split_anchor(target: &str) -> Option<(Option<&str>, &str)> {
    if target.contains("://") || target.starts_with("mailto:") || target.starts_with('/') {
        return None;
    }
    let (head, anchor) = target.split_once('#')?;
    if anchor.is_empty() {
        return None;
    }
    if head.is_empty() {
        return Some((None, anchor));
    }
    if !head.ends_with(".md") {
        return None;
    }
    Some((Some(head), anchor))
}

/// 좌변 — 레포 전체 `.md`. 렌더러 소관 트리는 순회가 아니라 **대상 쪽**에서 뺀다.
///
/// 점 디렉토리는 내려가지 않는다. 커밋되지 않는 로컬 작업 폴더는 clone·CI 에 없지만
/// 개발자의 작업 트리에는 있고, 그러면 **같은 커밋이 기계마다 다른 좌변을 낸다** —
/// 실측 2026-09-08: worktree 433 · 원본 저장소 874. 뒤쪽에서만 그 폴더의 md 가
/// 세어졌고, 거기 있던 미해결 앵커 둘이 이 가드를 빨갛게 만들었다. 판정 능력은 안
/// 준다: 추적되는 `.md` 중 점 디렉토리 아래 있는 것이 0 개다(`git ls-files '*.md'`).
fn scanned_docs(root: &Path) -> Result<Vec<Walked>, String> {
    scanned_docs_under(root, root, &MD_FLOOR)
}

/// 순회 자체. 뿌리와 하한을 **인자로** 받는다 — 둘 다 모수의 성질이지 이 함수의 성질이
/// 아니고, 상수로 박아 두면 이 순회는 `.md` 433 개짜리 트리에서만 돌 수 있다. 그러면
/// 위 doc 이 말하는 두 가지(점 디렉토리를 안 내려간다 · `.md` 만 집는다)를 합성 트리에서
/// 한 번도 못 재고, 그 둘이 죽어도 실패는 순회가 아니라 앵커 쪽에서 나타난다.
fn scanned_docs_under(root: &Path, rel_base: &Path, floor: &Floor) -> Result<Vec<Walked>, String> {
    walk_with_floor(
        root,
        rel_base,
        floor,
        Descend::SkipBuildCachesAndDotDirs,
        &|found| found.rel.ends_with(".md"),
    )
}

/// 링크한 문서 자리에서 상대 경로를 푼다. `..` 를 문자로 접어 레포 밖으로 나가지 않게 한다.
fn resolve_rel(from_rel: &str, head: &str) -> Option<String> {
    let mut parts: Vec<&str> = from_rel.split('/').collect();
    parts.pop();
    for seg in head.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// 디스크에서 읽은 좌변. 경로 -> (앵커 집합, 원문).
///
/// 순회 결과와 **분리한다** — 판정은 이 자료구조 위에서만 돌고, 그래야 디스크 없이도
/// 같은 판정을 태울 수 있다.
struct Corpus {
    anchors_by_rel: BTreeMap<String, HashSet<String>>,
    contents_by_rel: BTreeMap<String, String>,
}

impl Corpus {
    /// 합성 좌변. `(경로, 원문)` 만 주면 앵커 집합은 [`anchors_of`] 가 만든다 — 앵커를
    /// 손으로 적어 넣으면 그 판독기가 픽스처에서 빠져 동어반복이 된다.
    fn from_pairs(pairs: &[(&str, &str)]) -> Self {
        let mut anchors_by_rel = BTreeMap::new();
        let mut contents_by_rel = BTreeMap::new();
        for (rel, text) in pairs {
            anchors_by_rel.insert((*rel).to_owned(), anchors_of(text));
            contents_by_rel.insert((*rel).to_owned(), (*text).to_owned());
        }
        Self {
            anchors_by_rel,
            contents_by_rel,
        }
    }
}

fn corpus_of(docs: &[Walked]) -> Corpus {
    let mut anchors_by_rel = BTreeMap::new();
    let mut contents_by_rel = BTreeMap::new();
    for found in docs {
        if let Ok(text) = std::fs::read_to_string(&found.path) {
            anchors_by_rel.insert(found.rel.clone(), anchors_of(&text));
            contents_by_rel.insert(found.rel.clone(), text);
        }
    }
    Corpus {
        anchors_by_rel,
        contents_by_rel,
    }
}

/// 판정 결과. 앞의 셋은 인구조사, 뒤의 둘은 위반이다.
struct Audit {
    intra: usize,
    cross: usize,
    violations: Vec<String>,
    violating_docs: std::collections::BTreeSet<String>,
}

/// 좌변을 네 갈래로 가른다 — 판정 밖(렌더러 소관·대상 없음) · 문서내 · 크로스파일, 그리고
/// 그중 안 풀리는 것.
///
/// 렌더러 소관 접두를 **인자로** 받는다. 상수에서 읽으면 "두 끝이 모두 content 안일 때만
/// 넘긴다" 는 그 조건을 합성 좌변에서 한 번도 못 태운다 — 오늘 생산 트리에서 위반이 0 이라
/// 이 함수의 **보고 갈래에는 입력이 아예 없다**.
fn audit(corpus: &Corpus, renderer_owned: &str) -> Audit {
    let mut out = Audit {
        intra: 0,
        cross: 0,
        violations: Vec::new(),
        violating_docs: std::collections::BTreeSet::new(),
    };
    for (rel, text) in &corpus.contents_by_rel {
        for (line_no, target) in links_of(text) {
            let Some((head, anchor)) = split_anchor(&target) else {
                continue;
            };
            let (target_rel, kind) = match head {
                None => (rel.clone(), "문서내"),
                Some(head) => {
                    let Some(resolved) = resolve_rel(rel, head) else {
                        continue;
                    };
                    (resolved, "크로스파일")
                }
            };
            // 사이트 판사에게 넘기는 것은 **두 끝이 모두** content 트리 안일 때뿐이다.
            // 발행된 사이트가 그 링크를 푸는 경우가 정확히 그 조합이고, 나머지 셋은
            // 어느 쪽이든 GitHub 이 푼다(`docs/` 는 발행되지 않는다).
            if rel.starts_with(renderer_owned) && target_rel.starts_with(renderer_owned) {
                continue;
            }
            let Some(anchors) = corpus.anchors_by_rel.get(&target_rel) else {
                // 대상 파일이 없거나 좌변 밖이다 — 경로 축(`cited_coordinates_exist`) 몫.
                continue;
            };
            if head.is_none() {
                out.intra += 1;
            } else {
                out.cross += 1;
            }
            if !anchors.contains(anchor) {
                out.violations
                    .push(format!("  {rel}:{line_no} — [{kind}] `{target}`"));
                out.violating_docs.insert(rel.clone());
            }
        }
    }
    out
}

#[test]
fn cited_anchors_resolve_to_a_heading() {
    let root = &tasty_doc_guards::repo_root();
    let docs = scanned_docs(root).unwrap_or_else(|why| panic!("{why}"));

    let corpus = corpus_of(&docs);
    let Audit {
        intra,
        cross,
        violations,
        violating_docs,
    } = audit(&corpus, RENDERER_OWNED_PREFIX);

    let judged = intra + cross;
    // 인구조사 — "없어서 0" 과 "못 봐서 0" 을 가르는 값은 실패문에만 두지 않는다.
    println!(
        "앵커 좌변: 문서 {} 개 · 판정 {judged} 건(문서내 {intra} · 크로스파일 {cross})",
        docs.len()
    );
    assert!(
        judged >= REFS_FLOOR,
        "판정한 앵커 참조가 {judged} 건뿐이다(문서내 {intra} · 크로스파일 {cross}, 하한 \
         {REFS_FLOOR}) — 링크 스캐너가 죽으면 위반 0 이 나오므로 모수를 함께 본다. \
         ★ 하한을 내려서 통과시키지 마라."
    );
    // 이 순회는 레포 루트에서 시작한다 — 작업 트리에 있는 `.md` 는 추적 여부와 무관하게
    // 좌변에 들어온다. 점 디렉토리와 `node_modules` 는 안 내려가지만, 점 없는 다른 이름의
    // 폴더는 그대로 들어온다(실측 2026-09-08: 그 형태로 이 가드가 빨개진다).
    // 그래서 실패할 때만 좌표의 출신을 물어 처방이 레포 밖에 붙는 것을 막는다 —
    // 좌변을 git 으로 바꾸지 않은 근거는 [`tasty_doc_guards::tracked_scope`] 에 있다.
    let outside = if violations.is_empty() {
        String::new()
    } else {
        let rels: Vec<String> = violating_docs.into_iter().collect();
        tasty_doc_guards::tracked_scope::outside_repo_note(root, &rels)
    };
    assert!(
        violations.is_empty(),
        "풀리지 않는 앵커 {} 건 (문서 {} 개 · 판정 {judged} 건 중):\n{}\n\
         ★ 헤딩을 grep 으로 찾아 확인하지 마라 — 강조 표시를 품은 헤딩을 0 건으로 낸다. \
         헤딩 텍스트에서 슬러그를 다시 만들어 대조해라.{}",
        violations.len(),
        docs.len(),
        violations.join("\n"),
        outside
    );
}

#[test]
fn slug_is_a_function_of_heading_text_only() {
    // 레벨도 위치도 슬러그에 안 들어간다 — 그래서 절을 옮겨도 앵커가 안 바뀐다.
    assert_eq!(
        slug("미측정 구간의 **길이** — 이 문서가 가진 적 없던 축"),
        "미측정-구간의-길이--이-문서가-가진-적-없던-축"
    );
    assert_eq!(
        slug("훅이 **어느 OS 에서** 도는가 — 위 표에 없는 축"),
        "훅이-어느-os-에서-도는가--위-표에-없는-축"
    );
    // 헤딩이 링크를 품으면 표시 텍스트만 남는다.
    // 구두점은 지워지되 공백은 남아 하이픈이 된다 — GitHub 렌더러와 같은 자리에서 갈린다.
    assert_eq!(slug("주체 (→ [actors.md](actors.md))"), "주체--actorsmd");
}

#[test]
fn duplicate_heading_text_gets_an_ordered_suffix() {
    let anchors = anchors_of("# 같은 것\n\n## 같은 것\n\n### 같은 것\n");
    assert!(anchors.contains("같은-것"));
    assert!(anchors.contains("같은-것-1"));
    assert!(anchors.contains("같은-것-2"));
}

#[test]
fn fenced_and_inline_code_are_not_links() {
    let src = "```\n[a](#없는앵커)\n```\n\n`[b](#또없는앵커)` 는 예시다.\n";
    assert!(links_of(src).is_empty(), "{:?}", links_of(src));
}

/// [`RENDERER_OWNED_PREFIX`] 의 넘김에 **판정 자리**를 준다 — 넘긴 쪽 판사가 사라지면
/// 여기서 죽는다.
///
/// ## 왜 규칙의 사본이 아니라 배선을 재는가
///
/// 앞선 판은 사이트 생성기의 `slugify` 사본을 이 파일에 두고 그 둘이 같은 답을 내는지
/// 봤다. 그 사본은 원문이 이 레포 안에 있을 때만 성립한다. 사이트가 Astro 로 옮겨 가면서
/// 렌더러는 **의존 라이브러리**가 됐고, 그 규칙은 이 레포의 커밋으로는 안 바뀐다 —
/// `npm ci` 가 다른 판을 받아 오는 것만으로 바뀐다. 사본을 두면 그날 이 시험은 초록인 채로
/// 낡는다.
///
/// 그래서 재는 것은 규칙이 아니라 **넘긴 쪽에 판사가 있는가**다. 축 둘이다.
///
/// - **판정기 축** — `site/scripts/check-links.mjs` 가 여전히 앵커를 본다. `id` 를 모으는
///   자리와, 같은 페이지·다른 페이지 두 갈래에서 그것을 대조하는 자리.
/// - **배선 축** — `pages.yml` 이 그 스크립트를 실제로 부른다.
///
/// 두 축을 함께 봐야 하는 이유는 이 자리가 실제로 한 번 끊긴 방식이다: 스크립트는 멀쩡히
/// 있었고 **부르는 곳만 없었다.** 판정기만 보면 그 상태가 초록이다.
#[test]
fn the_site_anchor_judge_is_still_wired() {
    let root = tasty_doc_guards::repo_root();
    let read = |rel: &str| {
        let path = root.join(rel);
        std::fs::read_to_string(&path).unwrap_or_else(|why| {
            panic!(
                "넘긴 쪽 판사를 못 읽었다 ({}): {why} — 못 읽은 채로 통과하면 이 시험은 \
                 \"판사가 있다\" 가 아니라 \"안 봤다\" 가 된다",
                path.display()
            )
        })
    };

    // 판정기 축 — 이 스크립트를 앵커 판사로 만드는 결정 셋.
    const JUDGE: &str = "site/scripts/check-links.mjs";
    let judge = read(JUDGE);
    let decisions: &[(&str, &str)] = &[
        ("산출 HTML 에서 `id` 를 모은다", r#"/\sid="([^"]+)"/g"#),
        (
            "같은 페이지 안의 `#조각`을 대조한다",
            "anchors.get(file).has(",
        ),
        (
            "다른 페이지의 `#조각`을 대조한다",
            "anchors.get(target)?.has(hash)",
        ),
    ];
    let missing: Vec<&str> = decisions
        .iter()
        .filter(|(_, needle)| !judge.contains(needle))
        .map(|(what, _)| *what)
        .collect();

    // 배선 축 — 그 스크립트를 부르는 자리.
    const WORKFLOW: &str = ".github/workflows/pages.yml";
    let wired = read(WORKFLOW).contains("npm run check-links");

    assert!(
        missing.is_empty() && wired,
        "`site/content/` 의 앵커를 볼 판사가 없어졌다.\n\
         판정기({JUDGE}) 에서 사라진 결정: {missing:?}\n\
         배선({WORKFLOW}) 이 `npm run check-links` 를 부르는가: {wired}\n\
         ★ 이것은 회귀가 아니라 **docs/documentation-model.md 의 재검토 조건이 발동한 것**이다 \
         (docs/documentation-model.md).\n\
         이 가드는 그 트리를 가리키는 앵커를 판정에서 **뺀다** — 뺀 근거가 \"그쪽에 더 \
         정확한 판사가 있다\" 이므로, 그 판사가 없으면 뺀 자리는 아무도 안 보는 구멍이다. \
         빨강이 뜻하는 것은 링크가 깨졌다가 아니라 **깨졌는지 아무도 안 본다**이다.\n\
         순서가 있다. (1) 판사를 되살린다(스크립트를 고쳤으면 위 결정을, 배선을 지웠으면 \
         스텝을). (2) 되살릴 수 없다면 ADR 을 다시 읽는다 — 넘길 곳이 없으면 넘김의 전제가 \
         사라진 것이고, 그때 할 일은 이 트리를 이 가드의 좌변에 들이는 것이다.\n\
         ☞ [`RENDERER_OWNED_PREFIX`] 를 지워서 통과시키지 마라 — 그러면 그 트리의 앵커를 \
         규칙의 **사본**이 판정하게 되고, 사본은 렌더러가 의존 라이브러리인 지금 따라갈 \
         대상이 없다."
    );
}

/// [`audit`] 의 **보고 갈래**와 **렌더러 소관 건너뛰기**에 입력을 넣는다. 둘 다 생산
/// 좌변으로는 못 태우는 자리다.
///
/// **왜 필요한지는 실측이다** (트리 d023893be · `--test cited_anchors_resolve`):
///  - 보고 갈래(`out.violations.push`)를 통째로 비우면 **rc=0 · 5 passed**.
///  - 건너뛰기 조건을 `&&` -> `||` 로 넓혀 한 끝만 content 여도 건너뛰게 하면 **rc=0**.
///  - 그 조건을 아예 꺼도(`false &&`) **rc=0**.
///
/// 생산 좌변에서 위반이 0 이고 판정 수가 하한(120)보다 훨씬 커서, 셋 다 조용하다.
/// (`resolve_rel` 의 `..` 접기만은 생산 트리가 이미 잡는다 — rc=1.)
///
/// ★ 합성 문서의 제목·경로·앵커는 전부 이 시험이 짓는다. 앵커 집합은 손으로 안 적고
/// [`anchors_of`] 에게 만들게 한다 — 적어 넣으면 그 판독기가 픽스처에서 빠진다.
#[test]
fn the_audit_reports_an_unresolved_anchor_and_skips_only_the_two_ended_site_links() {
    let corpus = Corpus::from_pairs(&[
        (
            "zone/guide.md",
            "# Alpha One\n\n[안으로](#alpha-one)\n[깨진 것](#no-such)\n\
             [옆으로](./other.md#beta-two)\n[없는 파일](./gone.md#whatever)\n",
        ),
        ("zone/other.md", "## Beta Two\n"),
        // 두 끝이 모두 렌더러 소관 — 사이트가 푼다. 판정에서 빠져야 한다.
        ("zone/rendered/a.md", "[사이트 안](./b.md#site-rule)\n"),
        ("zone/rendered/b.md", "## Site Rule\n"),
        // 한 끝만 렌더러 소관 — GitHub 이 푼다. 판정 대상이다.
        (
            "zone/deep/into_site.md",
            "[밖에서 안으로](../rendered/b.md#site-rule)\n",
        ),
        // 렌더러 소관 트리를 **가리키지만** 출발이 밖이라 판정 대상인 둘째 — 여기서는
        // 안 풀린다. 한 끝 조건이 `||` 로 넓어지면 이 위반이 사라진다.
        (
            "zone/deep/into_site2.md",
            "[밖에서 안으로, 없는 앵커](../rendered/c.md#no-such-heading)\n",
        ),
        ("zone/rendered/c.md", "## Under_Score\n"),
    ]);

    let a = audit(&corpus, "zone/rendered/");

    // ★ 오늘 생산 좌변이 못 태우는 갈래 — 안 풀리는 앵커가 좌표와 함께 나와야 한다.
    assert_eq!(
        a.violations.len(),
        2,
        "안 풀리는 앵커가 보고 갈래로 안 갔다: {:#?}",
        a.violations
    );
    assert!(
        a.violations.iter().any(|v| v.contains("zone/guide.md:4")
            && v.contains("문서내")
            && v.contains("#no-such")),
        "문서내 위반의 좌표·갈래·대상이 안 실렸다: {:#?}",
        a.violations
    );
    assert!(
        a.violations
            .iter()
            .any(|v| v.contains("zone/deep/into_site2.md") && v.contains("크로스파일")),
        "한 끝만 렌더러 소관인 링크가 판정에서 빠졌다: {:#?}",
        a.violations
    );
    assert_eq!(
        a.violating_docs.len(),
        2,
        "위반 문서 집합이 다르다: {:?}",
        a.violating_docs
    );

    // 인구조사 — 건너뛰기가 **두 끝 조건**이라는 것이 이 두 수에 걸린다. 조건을 끄면
    // `zone/rendered/a.md` 가 들어와 크로스파일이 하나 늘고, `||` 로 넓히면 한 끝만
    // content 인 둘이 빠져 둘이 준다. 대상 파일이 없는 링크는 어느 쪽으로도 안 센다.
    assert_eq!(
        (a.intra, a.cross),
        (2, 3),
        "판정 갈래의 크기가 다르다 — 건너뛰기 조건이나 경로 풀이가 움직였다"
    );
}

/// 순회가 **점 디렉토리를 안 내려가고 `.md` 만 집는가.** 합성 트리에서 잰다.
///
/// 이 성질은 [`scanned_docs`] 의 doc 이 실측과 함께 적어 둔 것인데(worktree 433 ·
/// 원본 저장소 874 — 뒤쪽에서만 점 디렉토리 아래 `.md` 가 세어져 이 가드가 빨개졌다),
/// 생산 트리에서는 그 갈래가 **안 걸리는 것으로만** 관측된다. 걸리는 쪽을 여기서 만든다.
#[test]
fn the_walk_skips_dot_directories_and_takes_only_markdown() {
    // 이유: 이 자리는 **프로세스당 한 번만** 불린다. cargo 시험 하네스는 `#[test]` 를
    //       한 프로세스에서 한 번 돌리고, 이 접두를 짓는 자리는 이 바이너리에 하나뿐이다
    //       (이 파일에서 임시 경로를 짓는 자리 1 · `#[test]` 7). 그래서 같은 프로세스의
    //       재호출이 없고, pid 가 지는 축(프로세스 간)이 이 자리에 필요한 축의 전부다.
    //       아래 `remove_dir_all` 이 지우는 것은 앞 호출의 트리가 아니라 **pid 가
    //       재사용된 옛 프로세스의 잔재**다 — 그것은 단조 카운터로도 안 없어진다.
    let root = std::env::temp_dir().join(format!(
        "tasty-cited-anchors-fixture-{}",
        std::process::id()
    ));
    // 이전 실행 잔여물 제거 — 없으면 `NotFound` 라 실패가 정상 경로다.
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("docs")).expect("합성 트리를 만들지 못했다");
    std::fs::create_dir_all(root.join(".hidden-work")).expect("합성 트리를 만들지 못했다");
    let write = |rel: &str, body: &str| {
        std::fs::write(root.join(rel), body).unwrap_or_else(|e| panic!("{rel}: {e}"));
    };
    write("top.md", "# Top\n");
    write("docs/one.md", "# One\n");
    write("docs/two.md", "# Two\n");
    write("docs/not-markdown.txt", "# Two\n");
    // 점 디렉토리 안 — 커밋되지 않는 로컬 작업 폴더의 형태다. 좌변에 들어오면 안 된다.
    write(".hidden-work/note.md", "[깨진 것](#없는-제목)\n");

    // 하한은 이 합성 트리의 성질이다. 실제 저장소의 `MD_FLOOR` 를 그대로 쓰면 걸린다.
    let floor = Floor {
        min: 2,
        measured: 3,
        measured_on: "2026-09-08",
        counted_on: tasty_doc_guards::floored_walk::CountedOn::SyntheticTree,
        why_this_gap: "이 합성 트리의 `.md` 수다. 갈래를 더 시험하려고 파일을 더하는 것은 \
                       정상 변경이라 실측에 붙이면 그때마다 빨개진다.",
    };
    let found = scanned_docs_under(&root, &root, &floor).expect("합성 트리 순회가 하한에 걸렸다");
    let mut rels: Vec<String> = found.iter().map(|f| f.rel.clone()).collect();
    rels.sort();
    assert_eq!(
        rels,
        vec![
            "docs/one.md".to_owned(),
            "docs/two.md".to_owned(),
            "top.md".to_owned()
        ],
        "순회가 집은 것이 다르다 — 점 디렉토리를 내려갔거나 `.md` 아닌 것을 집었다"
    );

    // 정리 — 다음 완주가 이전 잔여물을 읽지 않게 한다. 실패해도 판정과 무관하다.
    let _ = std::fs::remove_dir_all(&root);
}
