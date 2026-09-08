//! 마크다운이 인용한 **앵커가 실제로 풀리는가** 를 본다. 축 둘이다.
//!
//! ㄱ **문서내 축** — `](#슬러그)` 는 그 문서 자신의 헤딩에서 나온 슬러그여야 한다.
//! ㄴ **크로스파일 축** — `](다른.md#슬러그)` 는 그 파일의 헤딩에서 나온 슬러그여야 한다.
//!
//! ## 왜 필요한가 — 이 축에는 채널이 없었다
//!
//! 죽은 앵커는 죽은 경로보다 조용하다. 경로가 틀리면 링크가 404 로 눈에 띄지만, 앵커가
//! 틀린 링크는 **문서를 열어 주고 엉뚱한 자리(대개 맨 위)에 세운다** — 읽는 사람은
//! 자기가 스크롤을 놓친 줄 안다. 그리고 이 저장소는 절을 옮기고 헤딩을 다듬는 변경이
//! 잦아 앵커가 깨지는 경로가 상시로 열려 있다.
//!
//! 이 크레이트에 이미 사는 [`cited_coordinates_exist`](cited_coordinates_exist.rs) 의
//! 링크 축은 이 물음에 **의도적으로 답하지 않는다** — 그 `scan_links` 는 대상에서
//! `#` 뒤를 잘라 내고 순수 앵커(`#` 로 시작하는 대상)는 external 로 건너뛴다. 그쪽은
//! "파일이 실재하는가" 를 묻고 여기는 "그 파일 안에 그 자리가 있는가" 를 묻는다.
//!
//! ## 좌변 — 판정 대상은 **소스가 아니라 대상 트리**로 정한다
//!
//! 훑는 것은 레포 전체 `.md` 다. 판정에서 빼는 것은 **`site/content/` 를 가리키는 앵커**
//! 이고, 그 링크가 어디에서 출발했는지는 안 본다. 이유는 소비자다: 그 트리의 슬러그는
//! 발행된 사이트가 소비하고, `site/scripts/check-links.mjs` 가 **산출된 HTML 에서 실제로
//! 나온 `id="…"`** 로 판정한다. 같은 자리를 두 판사가 보면 답이 둘이 될 수 있고, 그중
//! 하나(이 파일)는 규칙의 **사본**이라 언제나 더 나쁜 판사다.
//!
//! **한때 이 절은 소스 기준(`site/content/` 발 링크를 통째로 제외)이었다. 그것이 구멍을
//! 하나 남겼다** — 그 트리에서 출발해 트리 **밖**(`docs/` 등)을 가리키는 앵커다. 그
//! 링크는 사이트 렌더러가 `Fragment` 로 기록하지 않고(`rewrite_link` 는 content 트리
//! 안의 `.md` 일 때만 기록한다) GitHub blob URL 로 바꿔 내보낸다 — 즉 **그 앵커를 푸는
//! 것은 GitHub 이고, 그러면 이쪽 규칙이 옳은 판사다.** 대상 기준으로 바꾸면 그 갈래가
//! 저절로 이쪽에 들어오고 겹침은 여전히 0 이다. 실측(2026-09-08) 그런 링크는 0 건이라
//! 지금 잡히는 것은 없다 — 비어 있는 갈래를 여는 것이 이 규칙의 값이다.
//!
//! ## 규칙은 하나다 — 그런데도 왜 넘기나
//!
//! 한때 규칙이 둘이었다. 사이트를 손으로 쓴 러스트 생성기가 렌더했고 그 `slugify` 는
//! `_` 를 `-` 로 접고 양끝 `-` 를 뗐다 — GitHub 은 둘 다 안 한다. 사이트가 Astro 로
//! 옮겨 가면서 그 생성기가 사라졌고, 지금 렌더러는 GitHub 과 같은 답을 낸다(실측은 ADR
//! 에 있다). **그래서 이 파일에는 사이트 규칙의 사본이 없다.**
//!
//! 그래도 `site/content/` 를 넘기는 것은 규칙이 갈려서가 아니라 **판사의 질** 때문이다.
//! 이 파일의 [`slug`] 는 GitHub 규칙을 옮겨 적은 사본이고, 사이트 판사는 렌더된 HTML 의
//! `id` 를 그대로 읽는다. 사본은 원본이 바뀌면 조용히 낡지만 실측은 안 낡는다. 그리고
//! 지금 렌더러는 이 레포가 아니라 의존 라이브러리(Astro 의 마크다운 처리기)라 그 변경이
//! 이 레포의 커밋으로는 안 보인다 — 사본으로 따라갈 수 있는 대상이 아니다.
//!
//! 결정과 실측값은 `docs/adr/0247-site-anchors-are-judged-by-the-artifact-not-a-copy-of-the-rule.md`
//! 한 곳에 있다. **여기 옮겨 적지 마라** — 같은 수를 두 곳에 적으면 한쪽만 갱신되는 날이
//! 오고, 그날 어느 쪽이 맞는지 아무도 모른다. 지금 값이 궁금하면 이 시험이 찍는 인구조사
//! 줄을 봐라(`앵커 좌변: 문서 N 개 · 판정 M 건`).
//!
//! 이 넘김의 수명은 [`the_site_anchor_judge_is_still_wired`] 가 지킨다 — 넘긴 쪽 판사가
//! 사라지면 그 시험이 ADR 을 가리키며 죽는다.
//!
//! ## 넘긴 쪽 판사는 실재하나
//!
//! 배선만 보고 넘기지 않는다. 판사는 `site/scripts/check-links.mjs` 이고 `pages.yml` 의
//! `npm run check-links` 스텝이 부른다 — 산출된 모든 HTML 에서 `id="…"` 를 모은 뒤 내부
//! 링크의 `#조각`을 그 집합과 맞춘다. 축도 둘 다 본다: 같은 페이지 안의 `#x` 와 다른
//! 페이지를 가리키는 `그.html#x` 가 각각 별개 갈래다.
//!
//! **이 배선은 한 번 끊긴 적이 있다.** 사이트가 Astro 로 옮겨 가면서 옛 생성기의
//! `--strict` 가 사라졌고, `check-links` 는 스크립트로만 남아 아무도 안 불렀다. 그동안
//! `site/content/` 의 앵커에는 판사가 **하나도** 없었다 — 이 가드는 넘겼고 넘긴 쪽은
//! 비어 있었다. [`the_site_anchor_judge_is_still_wired`] 가 그 상태를 빨강으로 만든다.
//!
//! 그 잡은 경로 필터 뒤에 있어 main push 마다 돌지는 않는다. **그래도 이 축에는 사각이
//! 안 생긴다**: 그 트리의 앵커가 깨지는 길은 링크를 고치는 것과 헤딩을 고치는 것 둘뿐이고,
//! 둘 다 `site/**` 변경이라 정확히 그때 잡이 돈다.
//!
//! ## 이 가드가 **안 보는** 앵커 (실측 2026-09-08, 레포 `.md` 전체)
//!
//! - HTML 앵커(`<div id="...">` · `<a name=...>`) — 3 건. 전부 plugin 문서가 산출 HTML 을
//!   **설명하는** 산문이고 마크다운 링크의 대상이 아니다. 마크다운 헤딩만 슬러그의
//!   출처로 본다.
//! - 외부 URL 프래그먼트(`](https://...#x)`) — 0 건. 우리 레포가 답할 수 있는 물음이
//!   아니다.
//! - 참조식 링크(`[a]: b.md#x`) — 0 건. 인라인 형태만 훑는다.
//! - `.md` 아닌 대상의 프래그먼트 — 0 건.
//! - 대상 파일이 아예 없는 경우 — [`cited_coordinates_exist`] 의 경로 축 몫이라 여기서는
//!   건너뛴다. 한 결함에 빨강을 둘 내지 않는다.
//!
//! ## Windows 에서 같은 답이 나오나 — 경로를 문자열로 다루는 자리 전수 (2026-09-08)
//!
//! 이 가드는 경로를 많이 만지므로 컴파일 통과와 판정 일치를 따로 본다. 문자열로 경로를
//! 다루는 자리는 **일곱**이고, **OS 경로와 링크 경로를 섞는 자리는 0** 이다.
//!
//! | 자리 | 어느 공간인가 | Windows 에서 |
//! |---|---|---|
//! | `found.rel.ends_with(".md")` | `floored_walk` 가 정규화한 rel — 구분자가 언제나 `/` | 같다 |
//! | `anchors_by_rel` · `contents_by_rel` 의 키 (`found.rel`) | 같은 rel 공간 | 같다 |
//! | `read_to_string(&found.path)` | OS `Path` — **열기만 하고 쪼개지 않는다** | 같다 |
//! | `target.starts_with('/')` · `head.ends_with(".md")` | 링크 문자열 — 마크다운에서 `/` 는 언제나 `/` | 같다 |
//! | `resolve_rel` 의 `from_rel.split('/')` | rel 공간 | 같다 |
//! | `resolve_rel` 의 `head.split('/')` · `parts.join("/")` | 링크 공간 → rel 공간 | 같다 |
//! | `target_rel.starts_with("site/content/")` | rel 공간 | 같다 |
//!
//! 핵심은 **크로스파일 링크를 파일 시스템 경로로 바꾸지 않는다**는 것이다.
//! `../../docs/dev-guide/build.md#x` 는 `/` 공간에서 접혀 `/` 로 이어 붙인 rel 이 되고,
//! 그 rel 로 rel-키 맵을 조회한다 — `Path::join` 도 `strip_prefix` 도 안 거친다. 그래서
//! 백슬래시가 끼어들 자리가 없다. 섞였다면 조용히 어긋났을 것이다(예외가 아니라 조회
//! 전멸 → "위반 0"). 유일한 OS 경로인 `found.path` 는 여는 데만 쓴다.
//!
//! 확인은 `cargo clippy -p tasty-doc-guards --all-targets --locked --target
//! x86_64-pc-windows-gnu` 로 한다 — **msvc 타깃은 쓰지 마라.** `mlua-sys`·`libsqlite3-sys`
//! 가 Windows C 툴체인을 요구해 중간에 죽고, 그때 계수는 0 이 나온다. 그 0 은 "위반이
//! 없다" 가 아니라 **"못 봤다"** 다.
//!
//! ## 판정기의 성질 — grep 으로 헤딩을 찾지 마라
//!
//! 슬러그는 헤딩 **텍스트**만의 함수다. 레벨(`###` 인지 `####` 인지)도 문서 안 위치도
//! 안 들어간다. 그래서 **절을 통째로 옮겨도 앵커는 안 바뀐다** — 위치가 끼어드는 것은
//! 같은 텍스트의 헤딩이 둘 이상일 때 `-1`·`-2` 접미사가 문서 순서로 붙는 경우뿐이다.
//!
//! ★ 그리고 이 판정을 `grep '^#.*텍스트'` 로 대신하려 하지 마라. 헤딩은 강조 표시를
//! 품는다(`### 훅이 **어느 OS 에서** 도는가`) — 그 패턴은 그런 헤딩을 **0 건**으로 내고,
//! 0 건은 "그 헤딩이 없다" 와 모양이 같아서 멀쩡한 앵커가 깨진 것으로 보고된다. 실제로
//! 이 가드를 짓기 직전에 그 오판이 한 번 났다. 슬러그는 세지 말고 **재생성해서** 맞춘다.

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

/// 순회가 실제로 레포의 `.md` 를 봤음을 보장하는 하한.
///
/// 모수는 **레포 전체 `.md`** 다 — 점으로 시작하는 디렉토리 아래만 뺀다. 한때 이 주석이
/// "`site/content/` 를 뺀 수" 라고 적고 바로 아래 `why_this_gap` 은 "레포 전체" 라고
/// 적었다. 같은 선언 안에서 두 문장이 서로 다른 술어를 말했고, 순회의 필터에는 그 뺄셈이
/// 없었다 — 수를 지키는 것은 수가 아니라 그 수를 낳는 정의다.
const MD_FLOOR: Floor = Floor {
    min: 384,
    measured: 448,
    measured_on: "2026-09-08",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::LaneTip("8bdbf1bdb"),
    why_this_gap: "실측(`8bdbf1bdb` 직전 1215 커밋): 이 모수는 400..448 로 \
                   움직였고 **감소가 한 번도 없었다** — 47 개 커밋에서 다 늘기만 했고 최대 \
                   증가가 2 다. 감소 진폭이 관측되지 않았으므로 '진폭 × 몇 배' 를 쓸 근거가 \
                   없고, 대신 아직 안 일어난 사건의 크기에 건다: 문서 카테고리 하나가 접히면 그 \
                   아래 `.md` 가 통째로 빠지고, 지금 트리에서 그 크기의 최대는 `docs/features` \
                   의 64 다. 여유 64 = 사건 하나. `docs/adr` 211 은 곱수에 안 넣는다 — 그것이 \
                   접히는 것은 카테고리 하나가 접히는 사건이 아니라 결정 기록 방식이 바뀌는 \
                   일이고, 그때 할 일은 하한을 견디는 것이 아니라 이 수를 다시 재는 것이다. 앞선 \
                   판은 이 자리에 계보(레포 전체를 세고 `site/content/` 를 안 뺀다)만 적고 폭은 \
                   '넓게·좁게' 로만 말했다 — 그 말은 어떤 실측으로도 거짓이 되지 않아 118 이라는 \
                   폭을 아무것도 안 정했다. 좌변은 레포 전체의 `.md` 이고 그중 402 가 `docs/` \
                   아래다",
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
fn without_inline_code(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut in_code = false;
    for c in line.chars() {
        if c == '`' {
            in_code = !in_code;
            continue;
        }
        if !in_code {
            out.push(c);
        }
    }
    out
}

/// 코드펜스 밖의 줄만 (1-based 줄번호와 함께) 낸다.
fn prose_lines(contents: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut in_fence = false;
    for (i, line) in contents.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
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
    let mut out = HashSet::new();
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
    /// 손으로 적어 넣으면 그 판독기가 픽스처에서 빠져 동어반복이 된다(R1078).
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
         ★ 이것은 회귀가 아니라 **ADR-0247 의 재검토 조건이 발동한 것**이다 \
         (docs/adr/0247-site-anchors-are-judged-by-the-artifact-not-a-copy-of-the-rule.md).\n\
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
/// 생산 좌변에서 위반이 0 이고 판정 수가 하한(120)보다 훨씬 커서, 셋 다 조용하다.
/// (`resolve_rel` 의 `..` 접기만은 생산 트리가 이미 잡는다 — rc=1.)
///
/// ★ 합성 문서의 제목·경로·앵커는 전부 이 시험이 짓는다. 앵커 집합은 손으로 안 적고
/// [`anchors_of`] 에게 만들게 한다 — 적어 넣으면 그 판독기가 픽스처에서 빠진다(R1078).
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

    // 하한은 이 합성 트리의 성질이다. `MD_FLOOR`(min 330) 를 그대로 쓰면 걸린다.
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
