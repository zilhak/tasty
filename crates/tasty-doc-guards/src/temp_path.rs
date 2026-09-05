//! **공유 temp 아래 고정 이름 임시 경로**가 새로 생기는 것을 집는다 — ADR-0129 형태 B.
//!
//! `std::env::temp_dir()` 아래에 경로를 지으면 그것은 이 머신의 **모든 프로세스·모든
//! 완주가 공유하는 이름공간**이다. 거기에 인스턴스/완주를 가르는 성분 없이 고정 이름을
//! 쓰면, 같은 프로필 두 벌(또는 동시 두 완주)이 같은 파일을 truncate 하거나 서로의
//! 디렉터리를 지운다. 실측 사고: `disk_scrollback` 이 `surface-<id>` 만 써서 인스턴스
//! 둘이 스크롤백을 0 으로 만들었다.
//!
//! ## 극성 — "유일해야 하는가" 가 아니라 "유니크화됐는가" 를 묻는다
//!
//! "이 이름은 유일해야 하는가?" 는 **의도**를 읽어야 답한다 — 소스만 보고 못 푼다
//! (사용자 config 는 일부러 공유한다). 그래서 방향을 뒤집는다: **기본값은 "유니크화돼야
//! 한다"** 이고, 의도된 공유는 **예외**다. 예외는 명부가 아니라 **그 자리에 사유**로
//! 적는다(`이유:`/`reason:`, [`crate::…`] 가 아니라 `check-allow-reason` 이 쓰는 마커
//! 관례를 그대로 빌린다). 의미 판단이 가드에서 소스로 옮겨가고, 그 자리가 그 판단을 할
//! 수 있는 유일한 자리다. 명부를 안 쓰는 이유: 명부는 자기 대상을 이름으로 지목해
//! "쓰이는 것" 으로 만들고(R395), 정당한 예외가 구조적인 곳에서는 지키려는 표보다 빨리
//! 썩는다(R380).
//!
//! ## 유니크화로 인정하는 것 (R405 규율 3)
//!
//! 아래 성분이 경로 짓는 창(temp_dir 호출 줄 ~ +6 줄) 안에 있으면 유니크화로 본다:
//! - `TempDir`/`tempfile`/`tempdir(`/`NamedTempFile` — **OS 가 유일 이름을 준다. 최선**
//!   (재사용 위험 없음).
//! - `process::id`/`pid` — 살아 있는 프로세스마다 유일. pid 는 프로세스가 죽은 뒤
//!   **재사용될 수 있으나**, 임시 스크래치는 Drop/TTL 로 청소되므로 그 창에서 충돌하지
//!   않는다. 그래서 인정하되 최선은 아니다.
//! - `path_for` — 공유 헬퍼(`prompt_file`)가 pid 로 유니크화한다.
//! - `nanos`/`SystemTime` — 시간 nonce. 동시성에서 같은 눈금에 겹칠 창이 있어 **가장
//!   약하다** — 새 코드는 위 둘 중 하나를 쓰는 게 낫다.
//!
//! ## 잡지 못하는 것 (R16)
//!
//! - `temp_dir()` 를 받아 **멀리서** 고정 이름을 붙이는 형태(창 밖에서 `.join`) — 창 안에
//!   `.join(` 이 없으면 읽기 전용 사용과 구분되지 않아 보지 않는다.
//! - 소스에 리터럴로 안 보이는 이름(런타임 조합 문자열, 외부 도구가 짓는 이름)은 텍스트
//!   스캔의 사거리 밖이다.

use std::path::Path;

use crate::source_text::{mask_literals, mask_non_code, rust_sources};

/// 경로 짓는 창 안에 있으면 유니크화로 인정하는 성분.
const UNIQ_TOKENS: &[&str] = &[
    "TempDir",
    "tempfile",
    "tempdir(",
    "NamedTempFile",
    "process::id",
    "pid",
    "path_for",
    "nanos",
    "SystemTime",
];

/// 의도된 공유임을 그 자리에 밝히는 사유 마커(`check-allow-reason` 과 같은 관례).
const REASON_TOKENS: &[&str] = &["이유:", "reason:", "사유:"];

/// `temp_dir()` 호출 줄에서 경로를 짓는 `.join(` 을 찾을 때 보는 창.
const JOIN_WINDOW: usize = 6;
/// 사유 마커를 찾을 때 그 자리 위로 보는 창(붙은 주석 블록).
const REASON_LOOKBACK: usize = 4;

/// 한 파일을 분류한 결과. 줄 번호는 0 기반(`temp_dir()` 이 있는 줄).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct FileClass {
    /// 경로를 짓는(`.join` 이 창 안에 있는) `temp_dir()` 자리 전부.
    pub sites: Vec<usize>,
    /// 그중 유니크화된 줄.
    pub uniquified: Vec<usize>,
    /// 그중 사유로 공유가 명시된 줄.
    pub reasoned: Vec<usize>,
    /// 그중 유니크화도 사유도 없는 줄 — 고정 이름 공유(위반).
    pub silent: Vec<usize>,
}

/// masked 코드 줄들과 masked 주석 줄들로 temp 경로 자리를 분류한다.
///
/// `code` 는 [`mask_non_code`](crate::source_text::mask_non_code)(주석·문자열 덮음),
/// `comments` 는 [`mask_literals`](crate::source_text::mask_literals)(문자열만 덮고 주석은
/// 남김)의 결과다 — 앞은 코드 토큰, 뒤는 사유 마커를 읽는다. 둘 다 줄 수가 같아야 한다.
pub fn classify(code: &[&str], comments: &[&str]) -> FileClass {
    assert_eq!(code.len(), comments.len(), "두 마스크의 줄 수가 다르다");
    let mut out = FileClass::default();
    for idx in 0..code.len() {
        if !code[idx].contains("temp_dir()") {
            continue;
        }
        let hi = (idx + JOIN_WINDOW).min(code.len() - 1);
        // 창 안에서 경로를 짓는가. 안 지으면(읽기 전용·먼 곳에서 join) 보지 않는다.
        let builds_path = (idx..=hi).any(|j| code[j].contains(".join("));
        if !builds_path {
            continue;
        }
        out.sites.push(idx);

        let uniquified = (idx..=hi).any(|j| UNIQ_TOKENS.iter().any(|t| code[j].contains(t)));
        if uniquified {
            out.uniquified.push(idx);
            continue;
        }
        // 사유는 붙은 주석 블록(위) + 경로 짓는 창(아래)에서 찾는다.
        let lo = idx.saturating_sub(REASON_LOOKBACK);
        let reasoned = (lo..=hi).any(|j| REASON_TOKENS.iter().any(|t| comments[j].contains(t)));
        if reasoned {
            out.reasoned.push(idx);
        } else {
            out.silent.push(idx);
        }
    }
    out
}

/// 워크스페이스 전역 census.
#[derive(Debug, Default)]
pub struct Census {
    pub files_scanned: usize,
    pub sites: usize,
    pub uniquified: usize,
    pub reasoned: usize,
    /// `"레포상대경로:1기반줄: 원문"` 형태의 위반 목록.
    pub silent: Vec<String>,
}

/// `scan_roots` 아래를 훑어 census 를 만든다.
pub fn census(root: &Path, scan_roots: &[&str]) -> Census {
    let sources = rust_sources(root, scan_roots);
    let mut c = Census::default();
    for (rel, raw) in &sources {
        c.files_scanned += 1;
        let code_src = mask_non_code(raw);
        let comment_src = mask_literals(raw);
        let code: Vec<&str> = code_src.lines().collect();
        let comments: Vec<&str> = comment_src.lines().collect();
        let raw_lines: Vec<&str> = raw.lines().collect();
        let fc = classify(&code, &comments);

        c.sites += fc.sites.len();
        c.uniquified += fc.uniquified.len();
        c.reasoned += fc.reasoned.len();
        for &idx in &fc.silent {
            let text = raw_lines.get(idx).map(|s| s.trim()).unwrap_or("");
            c.silent
                .push(format!("{}:{}: {text}", rel.display(), idx + 1));
        }
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify_src(src: &str) -> FileClass {
        let code_src = mask_non_code(src);
        let comment_src = mask_literals(src);
        let code: Vec<&str> = code_src.lines().collect();
        let comments: Vec<&str> = comment_src.lines().collect();
        classify(&code, &comments)
    }

    /// 고정 이름을 공유 temp 에 지으면 — 유니크화도 사유도 없으면 — 잡는다.
    #[test]
    fn a_fixed_name_under_temp_dir_is_caught() {
        let fc = classify_src(
            "fn f() {\n    let p = std::env::temp_dir().join(\"tasty-thing.toml\");\n}",
        );
        assert_eq!(fc.sites.len(), 1);
        assert_eq!(
            fc.silent.len(),
            1,
            "유니크화·사유 없는 고정 이름을 잡아야 한다"
        );
    }

    /// pid 를 섞으면 유니크화로 통과한다.
    #[test]
    fn a_pid_keyed_name_passes() {
        let fc = classify_src(
            "fn f() {\n    let p = std::env::temp_dir()\n        .join(format!(\"x-{}.txt\", std::process::id()));\n}",
        );
        assert!(fc.silent.is_empty());
        assert_eq!(fc.uniquified.len(), 1);
    }

    /// 여러 줄 join 체인의 뒤쪽에 pid 가 있어도 창이 닿는다(disk_scrollback 형태).
    #[test]
    fn a_uniquifier_further_down_the_build_is_reached() {
        let fc = classify_src(
            "fn f() {\n    let dir = std::env::temp_dir().join(\"tasty-scrollback\");\n    let p = dir.join(format!(\n        \"surface-{}-{}\",\n        std::process::id(),\n        id\n    ));\n}",
        );
        assert!(fc.silent.is_empty(), "체인 아래 pid 를 창이 봐야 한다");
        assert_eq!(fc.uniquified.len(), 1);
    }

    /// 사유(`이유:`)를 그 자리에 적으면 의도된 공유로 통과한다.
    #[test]
    fn a_reasoned_shared_path_passes() {
        let fc = classify_src(
            "fn f() {\n    // 이유: 사용자 config 라 의도된 공유다.\n    let p = std::env::temp_dir().join(\"tasty-config.toml\");\n}",
        );
        assert!(fc.silent.is_empty(), "사유가 붙으면 통과");
        assert_eq!(fc.reasoned.len(), 1);
    }

    /// 영문 `reason:` 마커도 인정한다.
    #[test]
    fn an_english_reason_marker_also_passes() {
        let fc = classify_src(
            "fn f() {\n    // reason: shared on purpose.\n    let p = std::env::temp_dir().join(\"shared\");\n}",
        );
        assert!(fc.silent.is_empty());
    }

    /// 경로를 안 짓는 `temp_dir()`(읽기 전용·전달)은 보지 않는다.
    #[test]
    fn a_bare_temp_dir_without_join_is_ignored() {
        let fc =
            classify_src("fn f() {\n    let dir = std::env::temp_dir();\n    read_only(dir);\n}");
        assert!(fc.sites.is_empty(), "join 이 없으면 자리로 세지 않는다");
    }

    /// 사유가 문자열 안에만 있으면 인정하지 않는다(주석 마스크가 문자열을 덮는다).
    #[test]
    fn a_reason_inside_a_string_does_not_count() {
        let fc = classify_src(
            "fn f() {\n    let msg = \"이유: not a real marker\";\n    let p = std::env::temp_dir().join(\"fixed\");\n}",
        );
        assert_eq!(fc.silent.len(), 1, "문자열 속 이유: 는 사유가 아니다");
    }
}
