//! 번들 plugin 의 **프로덕션 코드에 로케일 고정 문자열이 박혀 있는가.**
//!
//! plugin 이 사람이나 다른 에이전트에게 내보내는 문구는 그 plugin 의
//! `lang/{en,ko,ja}.toml` 을 거쳐야 한다([i18n](../../docs/dev-guide/i18n.md) 의
//! plugin 네임스페이스 절). 규칙은 원래부터 plugin 을 덮고 있었고 기계도 있었다 —
//! 그런데 지키는 것은 plugin 마다 갈렸다. 2026-09-05 실측: codex plugin 이 완료 알림
//! 문구 · 샌드박스 힌트 · reboot 안내 **셋을 한국어로 박아** 내보내고 있었고, 형제인
//! claude plugin 은 **같은 채널의 같은 성격 문구**를 `t()` 로 내보내고 있었다.
//!
//! ## 왜 하필 CJK 리터럴인가
//!
//! 이 가드는 "번역을 거쳤는가" 를 묻지 않는다 — 그건 호출 형태를 봐야 하고, 형태는
//! plugin 마다 다르다. 대신 **틀렸다는 것이 값만으로 확정되는 부분집합**을 본다:
//! 코드에 박힌 한글·가나·한자 리터럴은 어떤 로케일 설정에서도 그 언어로만 나가므로,
//! 그것이 사용자·에이전트에게 전달되는 문구라면 정의상 틀렸다.
//!
//! **못 잡는 것을 분명히 적는다** — 영어로 박힌 전달 문구는 이 가드가 못 본다.
//! 영어 리터럴은 코드 식별자·프로토콜 토큰·진단과 값만으로 구분되지 않기 때문이다.
//! 그쪽은 호출 형태를 보는 `tests/no_hardcoded_ui_strings.rs` 의 몫이고, 그 가드가
//! 아직 plugin 의 전달 채널 형태(`append_notify_line` · `terminal.tell` 파라미터)를
//! 목록에 안 넣은 것이 이 구멍이 열려 있던 이유다.
//!
//! 개발자 표면(assert / panic / `tracing::*`)은 대상이 아니다 — 이 저장소는 그 문구를
//! 한국어로 쓰는 것이 관례이고, 사용자에게 안 나간다.
//!
//! ## 무엇이 "프로덕션 코드" 인가 — 이름이 아니라 성질로 가른다
//!
//! 처음 판정은 세 자리를 **줄 위치·파일명**으로 어림잡았고, 셋 다 틀렸다:
//!
//! - **`#[cfg(test)]` 를 만나면 `break`** 했다. 파일 끝의 `mod tests` 만 있다고 가정한
//!   것인데, `#[cfg(test)] fn` 헬퍼가 파일 중간에 있으면 그 뒤의 **프로덕션 코드까지**
//!   통째로 시야 밖이 된다. 실측(2026-09-05): 첫 `#[cfg(test)]` 뒤 13864 줄 중
//!   **2606 줄이 프로덕션**이었고 그중 2369 줄이 markdown plugin 의 렌더러 한 파일이다.
//! - **테스트 전용 모듈을 `*_tests.rs` 라는 이름으로만** 알아봤다. `#[cfg(test)] mod
//!   tests;` 로 선언된 형제 테스트 파일은 그 이름에 안 걸려 **테스트 코드가 프로덕션으로**
//!   판정됐다 — markdown plugin 이 인라인 테스트 모듈을 그렇게 분리하자 그 파일의
//!   `assert!` 메시지 셋이 전달 문구로 신고됐다.
//! - **진단 호출을 한 줄로만** 알아봤다. 인자가 여럿인 `assert!` 는 rustfmt 가 메시지를
//!   다음 줄로 밀어내므로 그 줄만 보면 진단인 줄 모른다.
//!
//! 그래서 지금은 셋 다 성질로 판정한다 — `#[cfg(test)]` 가 **실제로 덮는 줄 범위**
//! ([`cfg_span`]), **선언상 출하되지 않는 파일**
//! ([`super::sloc_gate_skip_proxy::test_only_files`] 를 공유한다 — 전이 폐쇄와
//! `#[path]` · `cfg(all(test, …))` 까지 그쪽이 이미 본다), 그리고 괄호 수지로 잡은
//! **진단 호출 범위**. 셋 다 소스에서 도출하므로 손 목록이 없다.
//!
//! 가운데 것의 배선은 본문 단정이 지킨다 — 필터를 지우면 그 파일을 이름까지 짚어 운다.
//! 처음 넣을 때는 모수가 0 이라 공허했고, markdown plugin 이 인라인 테스트 모듈을 형제
//! 파일로 분리하면서 실효가 됐다.

use tasty_doc_guards::cfg_predicate as cfg_span;
/// `#[cfg(...)]` 의 실제 줄 범위. 판정기는 `tasty-doc-guards` 하나이고 여기는 부르기만
/// 한다 — 사본이 둘이면 갈리고, 갈린 쪽은 조용하다.
use tasty_doc_guards::source_text::is_locale_specific;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::repo_root;

const CRATES_DIR: &str = "crates";
const PLUGIN_CRATE_PREFIX: &str = "tasty-plugin-";
const MANIFEST_NAME: &str = "tasty-plugin.toml";

/// 검사 대상 plugin 수의 하한 — **연기 검사**다. 목록이 비면 아래 판정은 아무것도
/// 안 보고 통과한다. 값의 근거: 2026-09-05 실측 9(매니페스트를 가진 크레이트).
const MIN_PLUGINS: usize = 8;

/// 개발자 표면 — 이 호출 **안**의 문구는 사용자에게 안 나간다.
const DIAGNOSTIC_MARKERS: &[&str] = &[
    "assert",
    "panic!",
    "unreachable!",
    ".expect(",
    "tracing::",
    "warn!",
    "info!",
    "error!",
    "debug!",
    "trace!",
];

/// 한 줄의 괄호 수지. 문자열·문자 리터럴·줄 주석 안은 세지 않는다
/// ([`cfg_span::brace_delta`] 와 같은 이유 — 문자열 속 `)` 에 속으면 진단 호출이
/// 일찍 닫히고 그 뒤의 메시지 줄이 전달 문구로 보인다).
fn paren_delta(line: &str) -> i32 {
    let b = line.as_bytes();
    let mut depth = 0i32;
    let mut i = 0usize;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            if c == b'\\' {
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            break;
        }
        if c == b'"' {
            in_str = true;
        } else if c == b'\'' {
            let esc = i + 1 < b.len() && b[i + 1] == b'\\';
            let end = if esc { i + 3 } else { i + 2 };
            if end < b.len() && b[end] == b'\'' {
                i = end + 1;
                continue;
            }
        } else if c == b'(' {
            depth += 1;
        } else if c == b')' {
            depth -= 1;
        }
        i += 1;
    }
    depth
}

/// 진단 호출이 **덮는 줄**을 표시한다 — 여는 줄부터 괄호 수지가 0 으로 돌아올 때까지.
///
/// 한 줄만 보던 판정은 `assert!(\n    cond,\n    "메시지",\n);` 형태에서 메시지 줄을
/// 전달 문구로 오인했다. 인자가 여럿이면 rustfmt 가 늘 그 모양으로 접는다.
fn diagnostic_lines(lines: &[&str]) -> Vec<bool> {
    let mut out = vec![false; lines.len()];
    let mut i = 0usize;
    while i < lines.len() {
        if !DIAGNOSTIC_MARKERS.iter().any(|m| lines[i].contains(m)) {
            i += 1;
            continue;
        }
        out[i] = true;
        let mut depth = paren_delta(lines[i]);
        let mut j = i;
        while depth > 0 && j + 1 < lines.len() {
            j += 1;
            out[j] = true;
            depth += paren_delta(lines[j]);
        }
        i = j + 1;
    }
    out
}

/// 한 소스에서 걸리는 (줄번호, 리터럴). **순수 함수** — 합성 입력으로 변이를 찌른다.
///
/// 세 가지를 뺀다: 주석 줄 · **진단 호출 범위** · **`#[cfg(test)]` 가 덮는 범위**.
/// 뒤의 둘은 한 줄이 아니라 범위다 — 그 차이가 이 판정의 전부다(모듈 문서 참조).
pub(crate) fn locale_specific_literals(src: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = src.lines().collect();
    let gated = cfg_span::cfg_gated_lines(&lines, "test");
    let diagnostic = diagnostic_lines(&lines);
    let mut out = Vec::new();
    for (idx, line) in lines.iter().enumerate() {
        if gated[idx] || diagnostic[idx] {
            continue;
        }
        if line.trim_start().starts_with("//") {
            continue;
        }
        for lit in string_literals(line) {
            if lit.chars().any(is_locale_specific) {
                out.push((idx + 1, lit));
            }
        }
    }
    out
}

/// 한 줄의 문자열 리터럴들. 이스케이프된 따옴표는 건너뛴다.
fn string_literals(line: &str) -> Vec<String> {
    let b: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != '"' {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut cur = String::new();
        while j < b.len() {
            if b[j] == '\\' {
                j += 2;
                continue;
            }
            if b[j] == '"' {
                break;
            }
            cur.push(b[j]);
            j += 1;
        }
        if j >= b.len() {
            break;
        }
        out.push(cur);
        i = j + 1;
    }
    out
}

/// 매니페스트를 가진 번들 plugin 크레이트의 `src/`.
fn bundled_plugin_srcs() -> Vec<PathBuf> {
    let root = repo_root().join(CRATES_DIR);
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).expect("crates 디렉터리를 읽을 수 없다") {
        let entry = entry.expect("디렉터리 항목");
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(PLUGIN_CRATE_PREFIX)
            && path.join(MANIFEST_NAME).is_file()
            && path.join("src").is_dir()
        {
            out.push(path.join("src"));
        }
    }
    out.sort();
    out
}

fn rs_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|e| e == "rs") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn no_bundled_plugin_ships_a_locale_specific_literal() {
    let srcs = bundled_plugin_srcs();
    assert!(
        srcs.len() >= MIN_PLUGINS,
        "매니페스트를 가진 plugin 크레이트가 {} 개뿐이다(하한 {MIN_PLUGINS}, 2026-09-05 \
         실측 9). 탐색이 죽으면 이 가드는 아무것도 안 보고 통과한다",
        srcs.len()
    );
    // 출하 여부는 **선언**이 정한다 — 파일명이 아니라. 판정기는 형제 가드와 공유한다
    // (전이 폐쇄 · `#[path]` · `cfg(all(test, …))` 까지 본다).
    let root = repo_root();
    let test_only = super::sloc_gate_skip_proxy::test_only_files();
    let mut files = 0usize;
    let mut skipped = 0usize;
    let mut scanned: Vec<PathBuf> = Vec::new();
    let mut hits: Vec<String> = Vec::new();
    for dir in &srcs {
        for f in rs_files(dir) {
            // `test_only` 의 원소는 공용 스캐너가 낸 것이라 구분자가 언제나 `/` 다.
            // 여기서 손으로 만든 경로를 그대로 쓰면 Windows 에서 **한 건도 안 맞고**,
            // 그 어긋남은 예외가 아니라 조용한 0 이다 — 필터가 통째로 꺼진 채 초록이 된다.
            let rel =
                tasty_doc_guards::source_text::repo_relative(f.strip_prefix(&root).unwrap_or(&f));
            if test_only.contains(&rel) {
                skipped += 1;
                continue;
            }
            scanned.push(rel);
            files += 1;
            let src = std::fs::read_to_string(&f)
                .unwrap_or_else(|e| panic!("{} 을 읽지 못했다: {e}", f.display()))
                .replace("\r\n", "\n");
            let rel =
                tasty_doc_guards::source_text::repo_relative(f.strip_prefix(&root).unwrap_or(&f))
                    .to_string_lossy()
                    .to_string();
            for (line, lit) in locale_specific_literals(&src) {
                let shown: String = lit.chars().take(60).collect();
                hits.push(format!("{rel}:{line}  {shown}"));
            }
        }
    }
    assert!(
        files >= 10,
        "plugin 소스를 {files} 개밖에 못 걸었다 — 파일 수집이 죽었다 \
         (테스트 전용 선언으로 뺀 것 {skipped} 개)"
    );
    // 배선 자체를 못 박는다 — 위 필터를 지우면 이 단정이 운다.
    //
    // **실효 확인됨.** 이 단정은 처음 넣을 때 공허했다 — 번들 plugin 중 `#[cfg(test)]
    // mod x;` 로 파일을 분리한 것이 0 개라 필터를 지워도 교집합이 빈 채였다. markdown
    // plugin 이 인라인 테스트 모듈을 형제 파일로 옮기면서 모수가 1 이 됐고, 그 뒤
    // 필터를 지워 재보니 그 파일을 이름까지 짚어 운다. 공허했던 사실을 지우지 않고
    // 적어 두는 이유는 "초록" 과 "재지 못했다" 가 다른 사실이기 때문이다.
    let leaked: Vec<String> = scanned
        .iter()
        .filter(|p| test_only.contains(*p))
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    assert!(
        leaked.is_empty(),
        "`#[cfg(test)]` 로 선언된 파일이 프로덕션으로 스캔됐다 — 출하 여부 판정이 \
         배선에서 빠졌다:\n  {}",
        leaked.join("\n  ")
    );
    assert!(
        hits.is_empty(),
        "번들 plugin 프로덕션 코드에 로케일 고정(CJK) 문구가 박혀 있다. 그 문구는 어떤 \
         로케일에서도 그 언어로만 나간다 — plugin 의 `lang/{{en,ko,ja}}.toml` 에 키를 \
         만들고 `Translator::t` 로 읽어라(형제 plugin 의 `claude.notify.done_message` · \
         `claude.reboot.notice` 가 같은 형태다):\n  {}",
        hits.join("\n  ")
    );
}

/// 판정기가 무엇을 잡고 무엇을 빼는지 — 합성 입력으로 못 박는다.
#[test]
fn the_detector_skips_diagnostics_comments_and_tests() {
    let src = "\
// 주석 안의 \"한국어 문구\" 는 대상이 아니다
let msg = \"완료했습니다\";
tracing::warn!(\"경고 문구\");
assert!(x, \"단정 문구\");
let ok = \"plain english\";
#[cfg(test)]
mod tests {
    const T: &str = \"테스트 안의 문구\";
}
";
    let hits = locale_specific_literals(src);
    let lits: BTreeSet<&str> = hits.iter().map(|(_, l)| l.as_str()).collect();
    assert!(
        lits.contains("완료했습니다"),
        "전달 문구를 놓쳤다: {lits:?}"
    );
    assert!(
        !lits.contains("경고 문구") && !lits.contains("단정 문구"),
        "개발자 표면을 집었다: {lits:?}"
    );
    assert!(
        !lits.contains("한국어 문구"),
        "주석 안의 문구를 집었다: {lits:?}"
    );
    assert!(
        !lits.contains("테스트 안의 문구"),
        "`#[cfg(test)]` 이후를 집었다: {lits:?}"
    );
    assert!(!lits.contains("plain english"), "영어까지 집었다: {lits:?}");
}

/// **이 가드가 빨개졌던 이유** — `#[cfg(test)]` 뒤의 프로덕션 코드를 다시 본다.
///
/// 옛 판정은 첫 `#[cfg(test)]` 에서 `break` 했다. 파일 중간의 테스트 헬퍼 하나가
/// 그 뒤 전부를 시야에서 지웠다 — 실측 2606 줄(그중 2369 줄이 markdown plugin 의 렌더러).
#[test]
fn production_code_after_a_test_helper_is_still_scanned() {
    let src = "\
#[cfg(test)]
fn helper() -> &'static str { \"테스트 헬퍼 문구\" }

fn shipped() { notify(\"배포되는 문구\"); }
";
    let lits: BTreeSet<String> = locale_specific_literals(src)
        .into_iter()
        .map(|(_, l)| l)
        .collect();
    assert!(
        lits.contains("배포되는 문구"),
        "테스트 헬퍼 뒤의 프로덕션 문구를 놓쳤다 — 판정이 첫 `#[cfg(test)]` 에서 멈췄다: \
         {lits:?}"
    );
    assert!(
        !lits.contains("테스트 헬퍼 문구"),
        "게이트 안의 헬퍼를 집었다: {lits:?}"
    );
}

/// 진단 호출이 여러 줄에 걸쳐도 그 **안**이다.
///
/// train44 를 빨갛게 만든 형태가 이것이다: 인자가 여럿인 `assert!` 는 rustfmt 가 메시지를
/// 다음 줄로 접고, 한 줄만 보던 판정은 그 줄을 전달 문구로 읽었다.
#[test]
fn a_wrapped_assert_message_is_still_a_diagnostic() {
    let src = "\
fn f() {
    assert!(
        cond,
        \"삭제가 error 상태로 감지되어야 한다\"
    );
    notify(\"사용자에게 나가는 문구\");
}
";
    let lits: BTreeSet<String> = locale_specific_literals(src)
        .into_iter()
        .map(|(_, l)| l)
        .collect();
    assert!(
        !lits.contains("삭제가 error 상태로 감지되어야 한다"),
        "줄바꿈된 단정 메시지를 전달 문구로 읽었다: {lits:?}"
    );
    assert!(
        lits.contains("사용자에게 나가는 문구"),
        "단정이 닫힌 뒤까지 진단으로 삼켰다 — 반대 방향으로 눈이 멀었다: {lits:?}"
    );
}

/// 이스케이프된 따옴표가 리터럴 경계를 흔들지 않는다.
#[test]
fn an_escaped_quote_does_not_split_the_literal() {
    let hits = locale_specific_literals("let s = \"그는 \\\"안녕\\\" 이라 했다\";");
    assert_eq!(hits.len(), 1, "리터럴이 쪼개졌다: {hits:?}");
}
