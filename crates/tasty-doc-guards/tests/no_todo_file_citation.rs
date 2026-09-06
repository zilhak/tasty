//! 휘발성 로컬 문서 인용 재유입 가드 — 커밋되는 파일이 git 에 올라가지 않는
//! 로컬 작업 문서를 인용하면 fail 한다.
//!
//! 배경: `CLAUDE.md` "소스 주석의 TODO 파일 및 디자인 changelog 인용 금지" 와
//! `docs/adr/template.md` "비-git 경로 참조 금지" 가 이미 규정한 것을 실제로
//! 강제한다. 로컬 작업 폴더는 `.gitignore` 대상이라 커밋되지 않고 완료된 항목은
//! 관례상 파일 자체가 삭제되므로, 그 번호·경로는 **로컬 세션에서만 유효한
//! 휘발성 식별자**다. 저장소를 새로 clone 한 사람에게 그 좌표는 존재한 적이 없다.
//!
//! 실제로 번호 재사용이 일어나 인용이 *무관한 문서* 로 해석되는 사례까지 나왔다 —
//! 죽은 참조를 넘어 오도하는 참조가 된다. 같은 문제를 진단한 선례는
//! [ADR-0027](../docs/adr/0027-figma-planning-sot-naming-derived-index.md) 의
//! "세션/트랙 식별자 누수" 항목이다.
//!
//! **대체 수단(위반 시 이 중 하나를 쓴다)**:
//! 1. 이유가 자명하면 — 번호/경로 대신 이유를 주석에 직접 서술
//! 2. 설계 결정이 크면 — `docs/adr/` 에 ADR 을 쓰고 그 경로를 인용
//! 3. 기능 동작 설명이면 — `docs/`(dev-guide / features / plugins) 문서를 참조
//!
//! **탐지 패턴 6 종** (하나만 잡는 정규식으로는 절반도 못 거른다):
//! - P1 번호 인용 — 대문자 `TODO` + 공백 런(0 개 이상) + 선택적 하이픈 + 숫자.
//!   **어순 양방향**: 한국어 문장에서는 번호가 앞에 온다(`<숫자>번 TODO`). 뒤 어순만
//!   보던 시절 그 형태가 소스에 두 건 살아 있었다 — 같은 죽은 좌표인데 어순 하나로
//!   가드를 통과했다.
//! - P2 conductor 번호 인용 — `todo-conductor`(대소문자 무시) + 구분자 런 + 숫자
//! - P3 경로 인용 — 로컬 작업 폴더 + `todo` / `todo-conductor` / `plans` / `conductor`
//! - P4 디자인 changelog slug — `YYYY-MM-DD-<slug>`. 원격 Claude Design 프로젝트
//!   내부에만 존재해 로컬 파일시스템에 흔적조차 없으므로 더 휘발적이다.
//! - P5 앵커 슬러그 변형 — 마크다운 앵커(`#...`) 안에 굳은 번호(`-todo-<숫자>`).
//!   제목에 번호를 달면 그 번호가 앵커의 일부가 되어 링크·주석으로 퍼지고, 나중에
//!   제목에서 번호를 떼는 순간 그 참조가 **전부 깨진다** (실제로 한 번 발생했다 —
//!   제목 하나를 고치자 링크 1곳과 주석 15줄이 죽은 좌표가 됐다). 대문자 `TODO`
//!   가 아니라 P1 이 못 잡는 별도 형태다.
//! - P6 로컬 폴더 언급 — 하위 경로가 무엇이든, 아예 없든 잡는다. 폴더 이름 단독
//!   언급도 금지 대상이라는
//!   [ADR-0105](../docs/adr/0105-no-nongit-path-refs-in-tracked-sources.md) 의
//!   결정을 강제한다. P3 가 네 개 하위 디렉토리로 좁혀 놓았던 것을 넓힌 형태다.
//!
//! **매칭은 구분자 개수·대소문자로 회피되지 않아야 한다.** 구분자를 한 개만 소비하는
//! 매처는 공백 두 개만 넣어도 통과하고, 원문 그대로 비교하는 매처는 대문자 표기로
//! 통과한다. 그래서 P2~P6 은 구분자를 런으로 소비하고 소문자로 비교한다. P1 만
//! 예외적으로 공백 런까지만 넓힌다 — 임의 문장부호(`:` · `#` · `.`)를 구분자로
//! 허용하면 "TODO. 40" 같은 평범한 문장이 걸린다. 티켓 인용은 공백이나 하이픈으로
//! 쓰이지 문장부호로 쓰이지 않는다.
//!
//! **스캔 대상 정의 — denylist 전수 순회.** ADR-0105 의 규칙 범위가 "git 이 추적하는
//! 모든 파일" 이므로, 확장자·디렉토리 화이트리스트로 "볼 파일" 을 열거하지 않는다.
//! 순회가 닿는 모든 파일을 대상으로 삼고 바이너리 확장자만 뺀다. 화이트리스트 방식은
//! 스크립트·CI 설정·루트 문서·`site/` 를 통째로 놓치는 사각지대를 만들었고, 항목을
//! 추가해도 다음 사각지대가 또 생긴다. `git ls-files` 로 추적 집합을 직접 묻는 방법도
//! 있으나, 테스트가 git 바이너리와 저장소 메타데이터의 존재에 의존하게 되어 tarball
//! 빌드에서 깨지고
//! [ADR-0096](../docs/adr/0096-unit-tests-isolated-from-user-environment.md)
//! 의 "테스트는 환경을 읽지 않는다" 와도 어긋난다. gitignored 산출물은 `PRUNE_DIRS`
//! 가지치기로 덮이고, 남는 것(서명 파일 등)은 애초에 인용을 담지 않는다.
//!
//! **오탐 회피 — 홈 경로는 그 자리 직전 문맥으로 가른다.** 로컬 지침 폴더는 사용자
//! 홈에도 같은 이름이 있고, 홈 쪽은 ADR-0105 가 범위 밖으로 확정한 항목이다. 판정을
//! *줄 전체* 에서 홈 표기를 찾는 식으로 하면, 정당한 홈 경로가 한 번 나오는 줄에
//! 섞인 진짜 레포 로컬 참조까지 통과한다 — 그래서 **occurrence 직전** 만 본다
//! ([`home_context_before`]). 로컬 작업 폴더 쪽은 홈에 존재할 수 없어 예외가 없다.
//! reverse-DNS plugin id 는 이름 앞 글자가 식별자 문자라 경로 시작이 아니므로 애초에
//! 걸리지 않는다.
//!
//! 이 판정은 **휴리스틱**이다 — 직전 창에서 홈 표기를 낱말로 찾는 방식이라,
//! `let home = ...; home.join(".claude")` 처럼 변수 이름이 `home` 인 관용구에 기대고
//! 있다. 같은 뜻의 다른 이름(`user_root` · `profile_dir` 등)으로 쓰면 정당한 홈 경로가
//! 위반으로 잡힌다. 그때는 [`HOME_NEARBY`] 에 그 이름을 추가하거나, 소스를 `~` 표기로
//! 바꿔 접두 판정(①)에 걸리게 한다. 창을 넓히거나 부분문자열 매칭으로 되돌리는 것은
//! 답이 아니다 — 그러면 이 절이 막으려는 "줄 전체 면제" 로 되돌아간다.
//!
//! 선례: `tests/no_emoji_in_source.rs`(구조 템플릿) · `tests/design_token_adherence.rs`.

// 이유: 이 타깃은 전부 테스트다. 테스트의 `let _ =` 는 정책이 사유를 요구하지
// 않으므로 `clippy::let_underscore_must_use` 명부(프로덕션 전용)에 섞이면 안 된다
// — docs/dev-guide/error-handling.md.
#![allow(clippy::let_underscore_must_use)]
use std::path::{Path, PathBuf};

/// 예외 목록 — (repo-relative 경로, 그 파일에서만 허용하는 패턴 id).
///
/// **파일 통째가 아니라 패턴 단위**로 면제한다. 파일 전체를 빼면 그 파일이 *다른*
/// 형태의 위반을 새로 들여도 영영 잡히지 않는다 — 규칙 본문을 담은 파일일수록
/// 그렇게 되기 쉽다. 등록 기준은
/// [ADR-0105](../docs/adr/0105-no-nongit-path-refs-in-tracked-sources.md) 가 정한
/// 그대로다: *그 파일의 본질이 그 형태를 담는 것인가*. "고치기 번거롭다" 는 사유가
/// 아니다.
/// - `CLAUDE.md`: 규칙 본문이 번호 인용(P1)과 changelog slug(P4)를 **예시로** 든다.
///   예시를 지우면 규칙이 무엇을 금지하는지 알 수 없게 된다. 경로 인용(P3/P6)은
///   면제하지 않는다 — 규칙을 설명하는 데 실제 경로가 필요하지 않다.
/// - `.gitignore`: 제외 항목을 적는 것이 그 파일의 정의다(ADR-0105 범위 밖 4항).
/// - `docs/adr/0027-...`: 휘발 경로 누수를 *문제로 서술* 하는 예시(참조가 아니다).
///   게다가 Accepted ADR 의 Context 본문이라 template 규칙상 수정 대상도 아니다.
///
/// **이 파일 자신은 면제가 없다.** 순회 입력으로 폴더 이름이 필요한 곳은 조각으로
/// 조립하고([`ws_dir`]), 패턴 픽스처는 `fx!` 로 판정 지점을 끊어 쓴다. 가드가 자기
/// 자신에게만 통째 면제를 두면, 위에 적은 "파일 통째 면제 금지" 원칙의 유일한 예외가
/// 가드 본인이 되어 앞뒤가 맞지 않는다.
const ALLOWLIST: &[(&str, &[&str])] = &[
    ("CLAUDE.md", &["P1", "P4"]),
    (".gitignore", &["P6"]),
    (
        "docs/adr/0027-figma-planning-sot-naming-derived-index.md",
        &["P3", "P6"],
    ),
];

/// 탐지 패턴 표 — (id, 설명, 판정 함수). 한 줄에 대해 **전부** 돌린다.
type Finder = fn(&str) -> Option<String>;
const PATTERNS: &[(&str, &str, Finder)] = &[
    ("P1", "번호 인용", find_p1),
    ("P2", "conductor 번호 인용", find_p2),
    ("P3", "경로 인용", find_p3),
    ("P4", "디자인 changelog slug", find_p4),
    ("P5", "앵커 슬러그 번호", find_p5),
    ("P6", "로컬 폴더 언급", find_p6),
];

/// 순회에서 통째로 가지치기할 디렉토리명. 빌드 산출물·워크트리·VCS·의존성 +
/// vendored 서드파티 번들.
///
const PRUNE_DIRS: &[&str] = &[
    "target",
    "dist",
    "_site",
    ".worktree",
    ".git",
    ".idea",
    "node_modules",
];

/// vendored 서드파티 번들 — **파일 단위로 열거한다.**
///
/// 이유는 비용과 오탐이다. 수 MB 짜리 minified 번들을 산문 패턴으로 훑는 것이 순수
/// 비용이고, 라이브러리를 갱신했을 때 그 안의 문자열이 우연히 P4 형태를 띠면
/// **무관한 이유로 CI 가 빨개진다.**
///
/// **`assets` 라는 디렉토리 이름으로 면제하던 것을 파일 열거로 바꿨다.** 이름은 덮는
/// 범위가 열려 있어서, 그 디렉토리에 우리 콘텐츠가 들어오면 조용히 안 봐진다. 그것이
/// 가정이 아니라 이미 벌어진 상태였다 — 실측(2026-09-05) 이름 면제가 덮고 있던 것에
/// 우리가 쓴 `assets/linux/tasty.desktop` · `assets/icons/*.svg` ·
/// `crates/tasty-plugin-markdown/assets/NOTICE.md` 가 포함돼 있었다. 폰트·이미지는
/// 이름이 아니라 정본 [`tasty_doc_guards::is_binary_artifact_ext`] 가 이미 덮고 있어서,
/// 이름 면제가 실제로 더 덮던 것은
/// **우리 파일들뿐이었다.**
///
/// 열거는 새로 들어온 것을 안 덮는다는 점에서 이름과 강도가 다르다. 목록이 실재와
/// 어긋나면 [`the_vendored_list_matches_what_is_there`] 가 빨개진다.
const VENDORED_FILES: &[&str] = &[
    "crates/tasty-plugin-markdown/assets/highlight.min.js",
    "crates/tasty-plugin-markdown/assets/katex.min.css",
    "crates/tasty-plugin-markdown/assets/katex.min.js",
    "crates/tasty-plugin-markdown/assets/mermaid.min.js",
];

/// gitignored 로컬 폴더 이름의 조각. 이 파일 자신이 P6 에 걸리지 않도록 나눠 둔다.
const LOCAL_HEAD: &str = "claude";
const LOCAL_TAIL: &str = "-workspace";

/// 로컬 작업 폴더 이름(선행 `.` 없음).
fn ws_dir() -> String {
    format!("{LOCAL_HEAD}{LOCAL_TAIL}")
}

/// P6 가 잡는 로컬 폴더 — (이름, 선행 `.` 이 필요한가 = 홈에도 같은 이름이 있는가).
///
/// **긴 이름을 먼저 본다** — 짧은 쪽이 접두라, 순서를 바꾸면 로컬 작업 폴더를 보고도
/// 지침 폴더 이름으로 보고한다.
///
/// 로컬 작업 폴더는 홈에 존재할 수 없으므로 선행 `.` 유무와 무관하게 잡는다 — 점을
/// 뺀 표기(`<폴더>/temp`)로 쓰는 것이 가장 흔한 회피 형태다. 지침 폴더는 사용자 홈에도
/// 같은 이름이 있어 선행 `.` 과 직전 문맥으로 가른다.
fn local_dirs() -> Vec<(String, bool)> {
    vec![(ws_dir(), false), (LOCAL_HEAD.to_string(), true)]
}

/// 순회 가지치기 대상인지 — `PRUNE_DIRS` + gitignored 로컬 폴더(선행 `.`).
///
/// 로컬 폴더는 worktree 에 **심볼릭 링크**로 걸려 있을 수 있다. `is_dir()` 은 링크를
/// 따라가므로, 가지치기하지 않으면 순회가 레포 밖 실제 경로까지 새어나간다.
fn is_pruned(name: &str) -> bool {
    PRUNE_DIRS.contains(&name)
        || name
            .strip_prefix('.')
            .is_some_and(|rest| rest == LOCAL_HEAD || rest == ws_dir())
}

/// 이름으로 걸리거나, **디렉토리 자신이 빌드 캐시라고 밝히거나**.
///
/// 이름만 볼 때는 `CARGO_TARGET_DIR` 로 만든 다른 이름의 빌드 디렉토리가 통째로
/// 모수에 들어왔다 — 실측(2026-09-05) 이 가드가 1.30s → 86.30s 가 됐다.
/// 판정 근거와 후보 비교는 [`tasty_doc_guards::is_build_cache_dir`].
fn is_pruned_dir(path: &Path, name: &str) -> bool {
    is_pruned(name) || tasty_doc_guards::is_build_cache_dir(path)
}

/// 금지되는 하위 디렉토리 — 이 넷 뒤에 오는 경로만 P3 가 잡는다.
const FORBIDDEN_SUBDIRS: &[&str] = &["todo-conductor", "todo", "plans", "conductor"];

/// 이름 **바로 앞** 에 붙는 홈 경로 접두. 경로 구분자 한 겹은 벗기고 본다.
const HOME_PREFIXES: &[&str] = &["~", "$home", "%userprofile%"];

/// 이름 직전 짧은 창 안에 **단어로** 있으면 홈 문맥으로 보는 표기(코드/산문).
///
/// 단어 경계를 요구하는 이유: 부분문자열로 보면 `Homebrew` 나 `renderHome()` 같은
/// 무관한 낱말이 그 줄의 진짜 위반을 면제시킨다. 영숫자가 앞뒤에 붙으면 다른 낱말로
/// 본다(`home_dir` 는 `_` 가 경계라 `home` 으로 잡힌다).
const HOME_NEARBY: &[&str] = &["home", "claude_config_dir", "홈의"];

/// 직전 문맥을 보는 창 크기(문자 수).
const HOME_WINDOW: usize = 32;

/// 구분자 런을 소비한다 — 개수를 제한하면 공백 하나만 더 넣어도 회피된다.
fn skip_run(bytes: &[u8], mut i: usize, allowed: &[u8]) -> usize {
    while i < bytes.len() && allowed.contains(&bytes[i]) {
        i += 1;
    }
    i
}

/// `bytes[i..]` 가 숫자로 시작하면 그 숫자열의 끝 인덱스.
fn digits_end(bytes: &[u8], i: usize) -> Option<usize> {
    if i >= bytes.len() || !bytes[i].is_ascii_digit() {
        return None;
    }
    let mut end = i;
    while end < bytes.len() && bytes[end].is_ascii_digit() {
        end += 1;
    }
    Some(end)
}

/// P1 — 번호와 대문자 `TODO` 가 붙어 있는 형태를 **양쪽 어순 모두** 잡는다.
/// 뒤 어순은 `TODO` + 공백 런 + 선택적 하이픈 + 숫자, 앞 어순은 숫자 + `번` +
/// 공백 런 + `TODO`(한국어 문장의 자연스러운 순서). 번호 없는 평범한 `TODO:`
/// 주석은 대상이 아니다(금지 대상은 *파일 번호 인용* 이지 할 일 표시가 아니다).
fn find_p1(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let mut from = 0;
    while let Some(pos) = line[from..].find("TODO") {
        let start = from + pos;
        let mut i = skip_run(bytes, start + 4, b" \t");
        if i < bytes.len() && bytes[i] == b'-' {
            i += 1;
        }
        if let Some(end) = digits_end(bytes, i) {
            return Some(line[start..end].to_string());
        }
        if let Some(num_start) = korean_ordinal_start(line, start) {
            return Some(line[num_start..start + 4].to_string());
        }
        from = start + 4;
    }
    None
}

/// 앞 어순 판정 — `line[..todo_at]` 의 꼬리가 `숫자+번` + 공백 런인지 본다.
/// 맞으면 숫자열이 시작하는 바이트 인덱스.
fn korean_ordinal_start(line: &str, todo_at: usize) -> Option<usize> {
    const ORDINAL: &str = "번";
    let head = line[..todo_at].trim_end_matches([' ', '\t']);
    let digits = head.strip_suffix(ORDINAL)?;
    let num_start = digits.len()
        - digits
            .chars()
            .rev()
            .take_while(char::is_ascii_digit)
            .count();
    (num_start < digits.len()).then_some(num_start)
}

/// P2 — `todo-conductor`(대소문자 무시) + 구분자 런 + 숫자.
fn find_p2(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let needle = "todo-conductor";
    let mut from = 0;
    while let Some(pos) = lower[from..].find(needle) {
        let start = from + pos;
        let i = skip_run(bytes, start + needle.len(), b" \t/_-");
        if let Some(end) = digits_end(bytes, i) {
            return Some(line[start..end].to_string());
        }
        from = start + needle.len();
    }
    None
}

/// P3 — 로컬 작업 폴더 + 금지 하위 디렉토리. 대소문자·슬래시 개수로 회피되지 않는다.
fn find_p3(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let dir = ws_dir();
    let mut from = 0;
    while let Some(pos) = lower[from..].find(&dir) {
        let start = from + pos;
        from = start + dir.len();
        let after = skip_run(bytes, from, b"/\\");
        if after == from {
            continue; // 폴더 이름 뒤에 경로 구분자가 없다 — 하위 인용이 아니다.
        }
        if let Some(sub) = FORBIDDEN_SUBDIRS
            .iter()
            .find(|s| lower[after..].starts_with(**s))
        {
            return Some(format!("{dir}/{sub}"));
        }
    }
    None
}

/// P4 — 디자인 changelog 판정 slug(`YYYY-MM-DD-<slug>`). 대소문자를 가리지 않는다.
fn find_p4(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    let is_d = |i: usize| i < bytes.len() && bytes[i].is_ascii_digit();
    let is_dash = |i: usize| i < bytes.len() && bytes[i] == b'-';
    for start in 0..bytes.len() {
        // 앞 글자가 숫자면 연도 4 자리의 시작이 아니다(더 긴 숫자열의 중간).
        if start > 0 && bytes[start - 1].is_ascii_digit() {
            continue;
        }
        if !(is_d(start) && is_d(start + 1) && is_d(start + 2) && is_d(start + 3)) {
            continue;
        }
        if !(is_dash(start + 4) && is_d(start + 5) && is_d(start + 6)) {
            continue;
        }
        if !(is_dash(start + 7) && is_d(start + 8) && is_d(start + 9)) {
            continue;
        }
        if !is_dash(start + 10) {
            continue;
        }
        let mut end = start + 11;
        if !(end < bytes.len() && bytes[end].is_ascii_alphabetic()) {
            continue;
        }
        while end < bytes.len() && (bytes[end].is_ascii_alphabetic() || bytes[end] == b'-') {
            end += 1;
        }
        return Some(line[start..end].to_string());
    }
    None
}

/// 앵커 슬러그 안의 번호(`todo-<숫자>`)를 찾는다. 슬러그 시작이거나 `-` 뒤에 와야
/// 한다 — `todo-conductor` 는 뒤가 숫자가 아니라 걸리지 않는다. 호출부가 소문자로
/// 낮춘 슬러그를 넘긴다.
fn slug_todo_number(slug: &str) -> Option<String> {
    let bytes = slug.as_bytes();
    let needle = "todo-";
    let mut from = 0;
    while let Some(pos) = slug[from..].find(needle) {
        let start = from + pos;
        // `start - 1` 이 멀티바이트 연속 바이트여도 `-`(ASCII) 와는 절대 같지 않다.
        let at_boundary = start == 0 || bytes[start - 1] == b'-';
        let digits_start = skip_run(bytes, start + needle.len(), b"-");
        from = start + needle.len();
        if !at_boundary {
            continue;
        }
        if let Some(end) = digits_end(bytes, digits_start) {
            return Some(slug[start..end].to_string());
        }
    }
    None
}

/// P5 — 마크다운 앵커(`#<슬러그>`) 안에 굳은 번호.
fn find_p5(line: &str) -> Option<String> {
    let mut from = 0;
    while let Some(pos) = line[from..].find('#') {
        let start = from + pos;
        let rest = &line[start + 1..];
        let end = rest
            .find(|c: char| c.is_whitespace() || matches!(c, ')' | '`' | ',' | '"' | '\''))
            .map(|i| start + 1 + i)
            .unwrap_or(line.len());
        let slug = &line[start + 1..end];
        if let Some(hit) = slug_todo_number(&slug.to_ascii_lowercase()) {
            return Some(format!("#{slug} ({hit})"));
        }
        from = start + 1;
    }
    None
}

/// 이름 직전 문맥이 사용자 홈을 가리키는가.
///
/// **줄 전체가 아니라 그 자리 직전만** 본다. 줄 전체를 훑으면 정당한 홈 경로가 한 번
/// 나오는 줄에 섞인 진짜 레포 로컬 참조까지 통과한다. `at` 은 선행 `.` 의 인덱스다.
fn home_context_before(lower: &str, at: usize) -> bool {
    let head = &lower[..at];
    // ① 경로 접두가 바로 앞에 붙은 형태 — 구분자는 **런으로** 벗긴다(소스에서
    //    이스케이프된 `\\` 나 중복 `/` 로 회피되지 않게).
    let trimmed = head.trim_end_matches(['/', '\\']);
    if HOME_PREFIXES.iter().any(|p| trimmed.ends_with(p)) {
        return true;
    }
    // ② 코드/산문 문맥이 직전 짧은 창 안에 낱말로 있는 형태.
    let window = head
        .char_indices()
        .rev()
        .take(HOME_WINDOW)
        .last()
        .map_or(head, |(i, _)| &head[i..]);
    HOME_NEARBY.iter().any(|a| contains_word(window, a))
}

/// `hay` 안에 `word` 가 **낱말로** 있는가 — 앞뒤가 영숫자면 다른 낱말의 일부다.
fn contains_word(hay: &str, word: &str) -> bool {
    let bytes = hay.as_bytes();
    let mut from = 0;
    while let Some(pos) = hay[from..].find(word) {
        let start = from + pos;
        from = start + word.len();
        let before_ok = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let after_ok = bytes.get(from).is_none_or(|c| !c.is_ascii_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
    }
    false
}

/// P6 — 레포 로컬 폴더 언급. 하위 경로가 무엇이든, 아예 없든 잡는다.
///
/// P3 는 네 개 하위 디렉토리가 뒤따를 때만 잡았다. ADR-0105 가 폴더 이름 단독 언급
/// 까지 금지로 확정했으므로 그 범위를 여기서 강제한다.
fn find_p6(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for (name, needs_dot) in local_dirs() {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(&name) {
            let start = from + pos;
            from = start + name.len();
            let dotted = start > 0 && bytes[start - 1] == b'.';
            if needs_dot && !dotted {
                continue; // 홈에도 있는 이름은 경로 표기일 때만 대상이다.
            }
            // 이름(또는 선행 `.`) 앞이 식별자 문자면 더 긴 이름의 일부다
            // (reverse-DNS plugin id, `tasty-plugin-<이름>` 등). 경로 시작이 아니다.
            let prev = if dotted {
                start.checked_sub(2)
            } else {
                start.checked_sub(1)
            };
            if let Some(p) = prev {
                let c = bytes[p];
                if c.is_ascii_alphanumeric() || c == b'_' || c == b'-' {
                    continue;
                }
            }
            // 뒤가 식별자 문자면 다른 이름이다. 긴 이름을 먼저 보므로 로컬 작업
            // 폴더는 이 검사에 걸리기 전에 잡힌다.
            if let Some(&c) = bytes.get(from)
                && (c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
            {
                continue;
            }
            if needs_dot && home_context_before(&lower, start - 1) {
                continue;
            }
            return Some(if dotted {
                format!(".{name}")
            } else {
                name.clone()
            });
        }
    }
    None
}

/// 스캔에서 뺄 확장자 — 바이너리라 인용을 담을 수 없는 것. `read_to_string` 이
/// 비-UTF8 을 걸러 주지만, 여기서 먼저 쳐내 순회 비용을 줄인다.
/// 스캔 대상 파일인지 — repo-relative 경로 기준.
///
/// **denylist 전수 방식**: 순회가 닿은 파일은 기본적으로 전부 대상이고 바이너리
/// 확장자만 뺀다(정본 [`tasty_doc_guards::is_binary_artifact_ext`]). 확장자가 없는
/// 파일(`Justfile` · 훅 스크립트)도 대상이다. 근거는 모듈 주석 "스캔 대상 정의" 참조.
fn is_scan_target(rel: &str) -> bool {
    if VENDORED_FILES.contains(&rel) {
        return false;
    }
    let name = rel.rsplit('/').next().unwrap_or("");
    // 선행 `.` 은 확장자 구분자가 아니다 — dotfile 은 확장자 없음으로 본다.
    let ext = name
        .trim_start_matches('.')
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase());
    match ext {
        Some(e) => !tasty_doc_guards::is_binary_artifact_ext(&e),
        None => true,
    }
}

/// `path` 하위를 재귀 순회하며 스캔 대상 파일을 모은다. `is_pruned` 는 가지치기.
///
/// 디렉토리를 읽지 못하면 **panic 한다.** 조용히 건너뛰면 가드가 도는 줄 알면서
/// 실제로는 그 하위를 통째로 안 보는 상태가 되고, 그건 위양성보다 나쁘다.
fn gather(path: &Path, root: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        let rel = rel_of(path, root);
        if is_scan_target(&rel) {
            out.push(path.to_path_buf());
        }
        return;
    }
    let entries = std::fs::read_dir(path)
        .unwrap_or_else(|e| panic!("스캔 대상 디렉토리를 읽지 못했다: {} — {e}", path.display()));
    for entry in entries {
        let entry = entry
            .unwrap_or_else(|e| panic!("디렉토리 항목을 읽지 못했다: {} — {e}", path.display()));
        let p = entry.path();
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        // ★ **이름으로 하는 가지치기는 종류를 묻지 않는다.** worktree 에서 `.git` 은
        // 디렉토리가 아니라 `gitdir:` 한 줄이 든 **파일**이다 — 종류를 먼저 물으면
        // 그 파일이 가지치기를 빠져나가 모집단에 들고, 같은 커밋이 worktree 와 메인
        // 체크아웃에서 서로 다른 파일을 보게 된다. 모집단이 환경을 읽으면 답도
        // 언젠가 환경을 읽는다.
        if is_pruned(name) {
            continue;
        }
        // 캐시 표식 판정은 디렉토리 **안** 을 읽는다 — 파일에는 뜻이 없다.
        if p.is_dir() && is_pruned_dir(&p, name) {
            continue;
        }
        gather(&p, root, out);
    }
}

/// `rel` 이 면제받는 패턴 id 들. `ALLOWLIST` 조회를 순회에서 분리한 것이라
/// 합성 경로로 면제 창을 직접 찌를 수 있다.
///
/// 경로는 **정확 일치**다 — 접두/접미로 넓히면 `CLAUDE.md` 하나를 면제한 것이
/// `docs/CLAUDE.md` 나 `CLAUDE.md.bak` 까지 덮는다.
fn allowed_patterns(rel: &str) -> &'static [&'static str] {
    ALLOWLIST
        .iter()
        .find(|(f, _)| *f == rel)
        .map_or(&[], |(_, pats)| *pats)
}

/// 한 줄에 대한 판정 — 면제 적용까지 포함한다. 순회(파일 열기·경로 처리)와 갈라 둔 이유는
/// 면제를 겨냥한 변이를 **합성 문자열**로 찌르기 위해서다. 판정이 순회 안에 인라인으로
/// 있으면 면제 창을 시험하려면 레포에 진짜 위반을 심는 수밖에 없고, 그건 느린 데다
/// 되돌리다 사고가 난다.
fn violations_in_line(rel: &str, line: &str) -> Vec<String> {
    let allowed = allowed_patterns(rel);
    PATTERNS
        .iter()
        .filter(|(id, _, _)| !allowed.contains(id))
        .filter_map(|(id, kind, find)| find(line).map(|m| format!("{id} {kind}: `{m}`")))
        .collect()
}

fn rel_of(file: &Path, root: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn no_todo_file_citation() {
    let root = &tasty_doc_guards::repo_root();
    let mut files = Vec::new();
    gather(root, root, &mut files);
    files.sort();

    let mut violations = Vec::new();
    for file in files {
        let rel = rel_of(&file, root);
        let Ok(contents) = std::fs::read_to_string(&file) else {
            continue; // 비-UTF8 은 인용을 담을 수 없다.
        };
        for (i, line) in contents.lines().enumerate() {
            for found in violations_in_line(&rel, line) {
                violations.push(format!("  {}:{} — {found}", rel, i + 1));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "커밋되는 파일이 git 에 올라가지 않는 경로(로컬 작업 폴더 · 로컬 지침 폴더)나 \
         그 안의 문서·디자인 changelog slug 를 인용했다 — 그 좌표는 clone 한 사람에게 \
         존재한 적이 없고, 번호는 재사용되어 무관한 문서로 해석된다. 규칙 전문과 범위 밖 \
         4 종은 `docs/adr/0105-no-nongit-path-refs-in-tracked-sources.md`.\n\
         대체 수단 3 가지 중 하나를 쓸 것: (1) 이유가 자명하면 번호 대신 이유를 직접 서술 \
         (2) 설계 결정이 크면 `docs/adr/` 에 ADR 을 쓰고 그 경로를 인용 \
         (3) 기능 동작 설명이면 `docs/`(dev-guide / features / plugins) 문서를 참조.\n\
         앵커(P5)면 제목에서 번호를 떼고 그 제목을 가리키던 참조도 함께 고칠 것 — \
         제목의 번호는 앵커로 굳어 링크·주석으로 퍼진다.\n\
         P6 면 위치를 적지 말고 \"커밋되지 않는 로컬 전용 지침이 정한다\" 로 위임할 것.\n\
         그 형태를 담는 것이 본질인 파일이면 ALLOWLIST 에 (경로, 허용 패턴) 으로 추가:\n{}",
        violations.join("\n")
    );
}

// ── 패턴 함수 단위 테스트 ────────────────────────────────────────────────
//
// 메인 스캔은 "지금 레포가 깨끗한가" 만 말해 준다. 패턴이 *무엇을 잡고 무엇을
// 통과시키는지* 는 레포 상태와 무관하게 고정돼야 한다 — 특히 오탐 회피 쪽은
// 레포에 그 형태가 남아 있지 않으면 메인 스캔이 영영 검증하지 못한다.

/// 픽스처 조립 — 이 파일은 ALLOWLIST 면제가 없으므로, 금지 형태를 그대로 적으면
/// 자기 스캔에 걸린다. 조각을 끊는 지점은 각 패턴의 **판정 지점**이라(구분자 앞,
/// 폴더 이름 중간) 조립 결과는 리터럴과 같고 소스에는 그 형태가 남지 않는다.
/// `concat!` 이라 런타임 비용도 없다.
macro_rules! fx {
    ($($p:literal),+ $(,)?) => { concat!($($p),+) };
}

#[test]
fn p1_catches_numbered_todo_citation_only() {
    assert_eq!(
        find_p1(fx!("see TODO", " 40")),
        Some(fx!("TODO", " 40").into())
    );
    assert_eq!(find_p1(fx!("(TODO", "18)")), Some(fx!("TODO", "18").into()));
    assert_eq!(
        find_p1(fx!("TODO", "-7 은 이미 닫혔다")),
        Some(fx!("TODO", "-7").into())
    );
    // 공백 런 — 하나만 소비하면 공백 두 개로 회피된다.
    assert_eq!(
        find_p1(fx!("see TODO", "  40")),
        Some(fx!("TODO", "  40").into())
    );
    assert_eq!(
        find_p1(fx!("see TODO", " \t 40")),
        Some(fx!("TODO", " \t 40").into())
    );
    assert_eq!(
        find_p1(fx!("see TODO", " -40")),
        Some(fx!("TODO", " -40").into())
    );
    // 번호 없는 평범한 할 일 표시는 대상이 아니다.
    assert_eq!(find_p1("// TODO: refactor this later"), None);
    assert_eq!(find_p1("TODOS 는 소문자 아님"), None);
    // 임의 문장부호는 구분자로 보지 않는다 — 평범한 문장의 오탐을 막는다.
    // 앞 어순(한국어) — 구분자 개수와 무관하게 잡는다.
    assert_eq!(
        find_p1(fx!("17번 ", "TODO", " — mesh mirror")),
        Some(fx!("17번 ", "TODO").into())
    );
    assert_eq!(
        find_p1(fx!("(18번", "TODO", ")")),
        Some(fx!("18번", "TODO").into())
    );
    // `번` 앞에 숫자가 없으면 티켓 인용이 아니다.
    assert_eq!(find_p1(fx!("이번 ", "TODO", " 는 크다")), None);
    assert_eq!(find_p1(fx!("번 ", "TODO")), None);

    assert_eq!(find_p1("TODO: 40"), None);
    assert_eq!(find_p1("TODO. 40"), None);
    assert_eq!(find_p1("TODO #40"), None);
    assert_eq!(find_p1("TODO_40"), None);
    assert_eq!(find_p1("see todo 40"), None);
}

#[test]
fn p2_catches_conductor_ticket_numbers() {
    assert_eq!(
        find_p2(fx!("todo-conductor", "/12 참조")),
        Some(fx!("todo-conductor", "/12").into())
    );
    assert_eq!(
        find_p2(fx!("TODO-CONDUCTOR", " 3")),
        Some(fx!("TODO-CONDUCTOR", " 3").into())
    );
    // 구분자 런 — 개수로 회피되지 않는다.
    assert_eq!(
        find_p2(fx!("todo-conductor", "//12")),
        Some(fx!("todo-conductor", "//12").into())
    );
    assert_eq!(
        find_p2(fx!("todo-conductor", "  12")),
        Some(fx!("todo-conductor", "  12").into())
    );
    // 번호가 없으면 P2 는 잡지 않는다(폴더 이름 단독 언급은 P6 소관).
    assert_eq!(find_p2("todo-conductor 디렉토리"), None);
    assert_eq!(find_p2("todo-conductor#12"), None);
}

#[test]
fn p3_catches_workspace_subdir_paths() {
    assert_eq!(
        find_p3(fx!("claude", "-workspace/todo/3.md")),
        Some(fx!("claude", "-workspace/todo").into())
    );
    // 대소문자·슬래시 개수로 회피되지 않는다.
    assert_eq!(
        find_p3(fx!("claude", "-workspace/Todo/3.md")),
        Some(fx!("claude", "-workspace/todo").into())
    );
    assert_eq!(
        find_p3(fx!("claude", "-workspace//todo/3.md")),
        Some(fx!("claude", "-workspace/todo").into())
    );
    // 금지 하위가 아니면 P3 는 잡지 않는다(폴더 언급 자체는 P6 소관).
    assert_eq!(find_p3(fx!("claude", "-workspace/temp/x.png")), None);
}

#[test]
fn p4_catches_design_changelog_slug() {
    assert_eq!(
        find_p4(fx!("판정 slug 는 2026-07-03", "-spacing-offgrid 였다")),
        Some(fx!("2026-07-03", "-spacing-offgrid").into())
    );
    // 대문자 표기로 회피되지 않는다.
    assert_eq!(
        find_p4(fx!("2026-07-03", "-Spacing-Offgrid")),
        Some(fx!("2026-07-03", "-Spacing-Offgrid").into())
    );
    // 날짜만으로는 changelog slug 가 아니다.
    assert_eq!(find_p4("Date: 2026-09-04"), None);
    // 더 긴 숫자열의 중간은 연도가 아니다.
    assert_eq!(find_p4("id 120260-07-03-x"), None);
}

#[test]
fn p5_catches_anchor_slug_number() {
    assert!(find_p5(fx!("[링크](x.md#a-todo", "-12-b)")).is_some());
    // 대문자 앵커·하이픈 런으로 회피되지 않는다.
    assert!(find_p5(fx!("[링크](x.md#a-TODO", "-12-b)")).is_some());
    assert!(find_p5(fx!("[링크](x.md#a-todo", "--12)")).is_some());
    // `todo-conductor` 는 뒤가 숫자가 아니라 앵커 번호가 아니다.
    assert_eq!(find_p5("[링크](x.md#todo-conductor-notes)"), None);
    assert_eq!(find_p5("[링크](x.md#todo12)"), None);
    assert_eq!(find_p5("# 평범한 마크다운 제목"), None);
}

#[test]
fn p6_catches_local_workspace_mentions() {
    // 폴더 단독 언급 — P3 가 놓치던 형태.
    assert_eq!(
        find_p6(fx!("산출물은 .", "claude", "-workspace 아래")),
        Some(fx!(".", "claude", "-workspace").into())
    );
    assert_eq!(
        find_p6(fx!("스크린샷은 .", "claude", "-workspace/temp/ 에")),
        Some(fx!(".", "claude", "-workspace").into())
    );
    // 선행 `.` 을 뺀 표기 — 로컬 작업 폴더는 홈에 없으므로 그래도 위반이다.
    assert_eq!(
        find_p6(fx!("claude", "-workspace/temp 에 둔다")),
        Some(fx!("claude", "-workspace").into())
    );
    // 대문자 표기로 회피되지 않는다.
    assert_eq!(
        find_p6(fx!(".", "CLAUDE", "-WORKSPACE/temp")),
        Some(fx!(".", "claude", "-workspace").into())
    );
    // 레포 로컬 지침 폴더 — 상대 표기 변형 포함.
    assert_eq!(
        find_p6(fx!("설정은 .", "claude", "/CLAUDE.md 가 정한다")),
        Some(fx!(".", "claude").into())
    );
    assert_eq!(
        find_p6(fx!("./.", "claude", "/x 를 읽는다")),
        Some(fx!(".", "claude").into())
    );
    assert_eq!(
        find_p6(fx!(r".\.", "claude", r"\x 를 읽는다")),
        Some(fx!(".", "claude").into())
    );
    assert_eq!(
        find_p6(fx!("폴더는 .", "claude", " 하나뿐")),
        Some(fx!(".", "claude").into())
    );
}

#[test]
fn p6_home_exemption_is_adjacent_not_line_wide() {
    // 사용자 홈의 런타임 경로 — ADR-0105 가 범위 밖으로 확정한 항목.
    assert_eq!(find_p6("~/.claude/settings.json 을 머지한다"), None);
    assert_eq!(find_p6("$HOME/.claude/projects 아래를 훑는다"), None);
    assert_eq!(find_p6("%USERPROFILE%\\.claude\\settings.json"), None);
    assert_eq!(find_p6("Ok(base.home_dir().join(\".claude\"))"), None);
    assert_eq!(find_p6("아니면 홈의 `.claude/projects`."), None);
    assert_eq!(
        find_p6("$CLAUDE_CONFIG_DIR 미설정 시 .claude/projects"),
        None
    );
    // **같은 줄 어딘가의 홈 표기로는 면제되지 않는다.** 줄 전체를 보면 정당한 홈
    // 경로가 하나 있는 줄의 진짜 위반까지 통과한다.
    assert_eq!(
        find_p6(fx!("~/.tasty 와 .", "claude", "/CLAUDE.md 를 비교")),
        Some(fx!(".", "claude").into())
    );
    assert_eq!(
        find_p6(fx!("Homebrew 설치 후 .", "claude", "/CLAUDE.md 수정")),
        Some(fx!(".", "claude").into())
    );
    assert_eq!(
        find_p6(fx!(
            "renderHome() 은 .",
            "claude",
            "/settings.json 을 읽는다"
        )),
        Some(fx!(".", "claude").into())
    );
    assert_eq!(
        find_p6(fx!("홈 화면 설정은 .", "claude", "/CLAUDE.md 가 정한다")),
        Some(fx!(".", "claude").into())
    );
}

#[test]
fn p6_allows_identifiers_and_build_outputs() {
    // reverse-DNS plugin id — 경로가 아니다.
    assert_eq!(find_p6("id = \"com.tasty.claude\""), None);
    assert_eq!(find_p6("com.tasty.claude-design 은 제거됐다"), None);
    // 빌드 산출물·생성물 경로 — 범위 밖.
    assert_eq!(
        find_p6("crates/tasty-plugin-claude/tasty-plugin.toml.sig"),
        None
    );
    assert_eq!(find_p6("site/release.json 을 읽는다"), None);
    assert_eq!(find_p6("target/release/tasty-plugin-claude"), None);
}

#[test]
fn scan_target_covers_scripts_ci_and_root_docs() {
    // 예전 화이트리스트가 통째로 놓치던 사각지대.
    assert!(is_scan_target("scripts/bench/perf-10-surfaces.sh"));
    assert!(is_scan_target("CLAUDE.md"));
    assert!(is_scan_target("crates/tasty-design-tokens/README.md"));
    assert!(is_scan_target(".github/workflows/test.yml"));
    assert!(is_scan_target(".githooks/pre-commit"));
    assert!(is_scan_target("Justfile"));
    assert!(is_scan_target("site/content/help/troubleshooting.md"));
    // 바이너리는 제외.
    assert!(!is_scan_target("assets/icon.png"));
    assert!(!is_scan_target(
        "crates/tasty-plugin-claude/tasty-plugin.toml.sig"
    ));
}

/// **모집단이 환경을 읽으면 답도 환경을 읽는다.** worktree 에서 `.git` 은 파일이고
/// 메인 체크아웃에서는 디렉토리다 — 가지치기가 종류를 물으면 앞쪽에서만 그 파일이
/// 모집단에 들어 두 트리가 서로 다른 파일을 본다. 실재하는 레포를 상대로 시험하면
/// 이 회귀가 체크아웃 종류에 따라 조용히 사라지므로 임시 디렉토리로 형태를 짓는다.
#[test]
fn pruning_is_by_name_not_by_kind() {
    let dir = std::env::temp_dir().join(format!("tasty-prune-kind-{}", std::process::id()));
    // 앞선 실행의 잔여를 치운다 — 없는 것이 정상이라 실패가 정보가 아니다.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("임시 디렉토리");
    // worktree 의 형태 — 가지치기 이름을 가진 것이 디렉토리가 아니라 파일이다.
    std::fs::write(dir.join(".git"), "gitdir: elsewhere\n").expect("쓰기");
    std::fs::write(dir.join(".worktree"), "x\n").expect("쓰기");
    std::fs::write(dir.join("keep.md"), "x").expect("쓰기");

    let mut files = Vec::new();
    gather(&dir, &dir, &mut files);
    let mut seen: Vec<String> = files.iter().map(|f| rel_of(f, &dir)).collect();
    seen.sort();
    // 정리 실패는 무시한다 — 임시 디렉토리라 남아도 판정에 영향이 없다.
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(
        seen,
        vec!["keep.md".to_string()],
        "가지치기가 종류를 물었다 — 가지치기 이름을 가진 파일이 모집단에 들어왔다"
    );
}

/// **이름이 아닌 근거로도 가지치기된다.** 이 절이 없으면 `is_pruned_dir` 이 이름
/// 판정으로 퇴화해도 위 테스트가 전부 초록이라 — 다른 이름의 빌드 디렉토리가 모수에
/// 다시 들어온 것을 아무도 못 본다. 양극성으로 잡는다: 표식이 있으면 걸리고, 이름이
/// 같아 보여도 표식이 없으면 안 걸린다.
#[test]
fn a_build_dir_under_another_name_is_still_pruned() {
    let dir = std::env::temp_dir().join(format!("tasty-prune-{}", std::process::id()));
    // 정리 실패는 무시한다 — 임시 디렉토리라 남아도 판정에 영향이 없고, 여기서
    // 죽으면 진짜 실패가 정리 오류에 가린다.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("임시 디렉토리");

    assert!(
        !is_pruned_dir(&dir, "target-e2e-headless"),
        "표식이 없으면 이름이 빌드 디렉토리처럼 보여도 가지치기하지 않는다"
    );

    std::fs::write(
        dir.join("CACHEDIR.TAG"),
        "Signature: 8a477f597d28d172789f06886806bc55\n",
    )
    .expect("표식 쓰기");
    assert!(
        is_pruned_dir(&dir, "target-e2e-headless"),
        "표식이 있으면 이름과 무관하게 가지치기한다"
    );

    // 정리 실패는 무시한다 — 임시 디렉토리라 남아도 판정에 영향이 없고, 여기서
    // 죽으면 진짜 실패가 정리 오류에 가린다.
    let _ = std::fs::remove_dir_all(&dir);
}

/// vendored 목록이 실재와 맞는가. **열거의 값은 여기서 나온다** — 목록이 이름 면제와
/// 다른 것은 "새로 들어온 것을 안 덮는다" 는 점뿐이고, 그것은 목록이 낡지 않을 때만
/// 성립한다.
///
/// 양방향으로 본다. 목록에 있는데 없는 파일(번들이 옮겨졌다)과, 같은 디렉토리에
/// 목록에 없는 minified 번들이 새로 들어온 것.
#[test]
fn the_vendored_list_matches_what_is_there() {
    let root = &tasty_doc_guards::repo_root();
    for rel in VENDORED_FILES {
        assert!(
            root.join(rel).is_file(),
            "vendored 목록이 없는 파일을 가리킨다: {rel} — 번들이 옮겨졌으면 목록도 옮겨라"
        );
    }

    let dir = root.join("crates/tasty-plugin-markdown/assets");
    let mut unlisted = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("vendored 디렉토리를 읽지 못했다") {
        let path = entry.expect("디렉토리 항목").path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !(name.ends_with(".min.js") || name.ends_with(".min.css")) {
            continue;
        }
        let rel = format!("crates/tasty-plugin-markdown/assets/{name}");
        if !VENDORED_FILES.contains(&rel.as_str()) {
            unlisted.push(rel);
        }
    }
    assert!(
        unlisted.is_empty(),
        "목록에 없는 minified 번들이 있다 — 열거를 갱신해라: {unlisted:?}"
    );
}

/// **이름 면제가 덮던 우리 파일들이 이제 스캔된다.** 반대 극성이다 — 위 목록만 있고
/// 이름 면제가 남아 있으면 이 단언이 빨개진다.
#[test]
fn our_own_files_under_assets_are_scanned() {
    for rel in [
        "assets/linux/tasty.desktop",
        "assets/icons/tasty-melon.svg",
        "crates/tasty-plugin-markdown/assets/NOTICE.md",
    ] {
        assert!(
            is_scan_target(rel),
            "우리가 쓴 파일이 스캔 대상에서 빠졌다: {rel}"
        );
        let dir = rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
        let name = dir.rsplit('/').next().unwrap_or("");
        assert!(
            !is_pruned(name),
            "그 파일이 든 디렉토리가 이름으로 가지치기된다: {dir}"
        );
    }
    // 대조 — vendored 번들은 여전히 빠진다.
    assert!(!is_scan_target(
        "crates/tasty-plugin-markdown/assets/katex.min.js"
    ));
}

#[test]
fn prunes_build_outputs_and_local_dirs_but_not_assets() {
    assert!(is_pruned("target"));
    assert!(is_pruned("node_modules"));
    // vendored 번들은 **디렉토리 이름이 아니라 파일 열거**로 뺀다 — 그 디렉토리에는
    // 우리 파일도 산다(`the_vendored_list_matches_what_is_there` 참조).
    assert!(!is_pruned("assets"));
    // gitignored 로컬 폴더(선행 `.`).
    assert!(is_pruned(fx!(".", "claude")));
    assert!(is_pruned(fx!(".", "claude", "-workspace")));
    // 점 없는 같은 이름은 일반 디렉토리다.
    assert!(!is_pruned(fx!("claude")));
    assert!(!is_pruned("src"));
}

// ── 면제(ALLOWLIST) 를 겨냥한 변이 ──────────────────────────────────────
//
// 면제를 하나 두면 그 면제만큼 구멍이다. 면제 창 **안쪽**에 진짜 위반을 심었을 때
// 잡히는지를 묻는 것이 아래 셋이고, 셋 다 `violations_in_line` 에 합성 입력을 먹인다 —
// 레포에 위반을 심어 보는 방식이 아니라 판정기에 영구히 붙는 형태다.

#[test]
fn allowlist_exempts_only_the_named_pattern_not_the_whole_file() {
    // `CLAUDE.md` 는 P1·P4 만 면제다. 같은 파일에 P3(경로 인용)을 심으면 잡혀야 한다 —
    // 이 단언이 깨지는 형태가 곧 "파일 통째 면제" 로의 회귀다.
    let planted = fx!("claude", "-workspace/todo/3.md");
    let found = violations_in_line("CLAUDE.md", planted);
    assert!(
        found.iter().any(|v| v.starts_with("P3")),
        "면제 파일에 심은 비면제 패턴이 통과했다: {found:?}"
    );

    // 면제된 쪽은 그대로 통과한다(면제가 실제로 동작하는지의 반대편).
    assert!(violations_in_line("CLAUDE.md", fx!("see TODO", " 40")).is_empty());
    // 같은 줄이 면제 없는 파일에서는 잡힌다 — 통과가 패턴 고장이 아니라 면제 때문임을 가른다.
    assert!(
        violations_in_line("src/main.rs", fx!("see TODO", " 40"))
            .iter()
            .any(|v| v.starts_with("P1"))
    );

    // 반대 방향 — ADR 0027 은 P3·P6 면제이므로 P1 은 잡혀야 한다.
    let adr = "docs/adr/0027-figma-planning-sot-naming-derived-index.md";
    assert!(
        violations_in_line(adr, fx!("(TODO", "18)"))
            .iter()
            .any(|v| v.starts_with("P1"))
    );
}

#[test]
fn allowlist_paths_match_exactly_not_by_prefix_or_suffix() {
    assert!(allowed_patterns("CLAUDE.md").contains(&"P1"));
    // 창은 경로 하나다 — 접두/접미가 겹치는 다른 파일로 새지 않는다.
    assert!(allowed_patterns("docs/CLAUDE.md").is_empty());
    assert!(allowed_patterns("CLAUDE.md.bak").is_empty());
    assert!(allowed_patterns("crates/x/CLAUDE.md").is_empty());
    assert!(allowed_patterns("").is_empty());
    // 면제되지 않은 패턴은 면제 파일에서도 목록에 없다.
    assert!(!allowed_patterns("CLAUDE.md").contains(&"P3"));
    assert!(!allowed_patterns("CLAUDE.md").contains(&"P6"));
}

/// **이 초록이 뜻하는 것은 "면제가 아직 필요하다" 가 아니다.** 면제가 실재하는 파일과
/// 실재하는 패턴 id 를 가리킨다는 것뿐이다 — 참조 무결성이다.
///
/// 그 파일이 더 이상 그 패턴을 담지 않아 면제가 아무 일도 안 하게 된 상태는 여기서
/// 안 잡힌다. 그것을 잡으려면 항목을 빼고 가드를 돌려 빨개지는지 봐야 하고, 그 판정은
/// 가드 안에서 할 수 없다. 재는 절차는 `docs/dev-guide/guard-population.md`.
#[test]
fn allowlist_entries_point_at_things_that_exist() {
    // 경로가 썩으면 가드가 그 파일을 다시 잡아 **시끄럽게** 실패하지만, 패턴 id 가 썩으면
    // (오탈자·패턴 개명) 의도한 면제가 조용히 사라진 채 아무도 모른다. 뒤쪽을 여기서 잡는다.
    let root = &tasty_doc_guards::repo_root();
    let ids: Vec<&str> = PATTERNS.iter().map(|(id, _, _)| *id).collect();
    for (rel, pats) in ALLOWLIST {
        assert!(
            root.join(rel).exists(),
            "면제 항목이 가리키는 파일이 없다 — 옮겼거나 지웠으면 항목도 지워라: {rel}"
        );
        assert!(
            !pats.is_empty(),
            "빈 면제 목록은 항목을 지우라는 뜻이다: {rel}"
        );
        for pat in *pats {
            assert!(
                ids.contains(pat),
                "면제가 없는 패턴 id 를 가리킨다(오탈자·개명): {rel} → {pat}"
            );
        }
    }
    // 목록이 통째로 비면 위 루프가 아무것도 검사하지 않고 초록이 된다.
    //
    // ★ 여기 숫자를 두지 않는다. 전에는 `>= 3` 이었는데 그 3 은 어디서도 안 나온 값이고,
    // **여유가 0 이었다** — 실측 항목 수가 정확히 3 이다(2026-09-07). 문턱과 실측이 같으면
    // 면제를 하나 줄이는 **옳은** 커밋이 그 자리에서 빨개지고, 그때 가장 싼 초록화는
    // **죽은 면제를 하나 남겨 두는 것**이다. 그런데 이 파일의 doc 이 바로 그 상태
    // ("면제가 아무 일도 안 하게 된 상태")를 결함으로 적는다. 즉 문턱이 이 가드가
    // 지키려는 것의 반대를 보상하고 있었다.
    //
    // 이 자리가 물어야 하는 것은 "몇 개인가" 가 아니라 "루프가 돌 것이 있는가" 하나다.
    // 그 물음에는 도출된 답이 있고, 그것을 그대로 적는다 — 실측도 날짜도 필요 없다.
    assert!(
        !ALLOWLIST.is_empty(),
        "면제 목록이 비었다 — 위 루프가 한 번도 안 돌아 이 시험은 아무것도 안 본다. \
         면제가 정말 다 없어졌으면 이 시험도 함께 지워라(빈 명부를 지키는 시험은 \
         초록이 뜻을 잃는다)"
    );
}
