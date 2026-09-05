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

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::repo_root;

const CRATES_DIR: &str = "crates";
const PLUGIN_CRATE_PREFIX: &str = "tasty-plugin-";
const MANIFEST_NAME: &str = "tasty-plugin.toml";

/// 검사 대상 plugin 수의 하한 — **연기 검사**다. 목록이 비면 아래 판정은 아무것도
/// 안 보고 통과한다. 값의 근거: 2026-09-05 실측 9(매니페스트를 가진 크레이트).
const MIN_PLUGINS: usize = 8;

/// 개발자 표면 — 이 호출 안의 문구는 사용자에게 안 나간다.
fn is_diagnostic(line: &str) -> bool {
    const MARKERS: &[&str] = &[
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
    MARKERS.iter().any(|m| line.contains(m))
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF   // Hangul Jamo
        | 0x3040..=0x30FF // Hiragana · Katakana
        | 0x3130..=0x318F // Hangul Compatibility Jamo
        | 0x4E00..=0x9FFF // CJK Unified Ideographs
        | 0xAC00..=0xD7A3 // Hangul Syllables
    )
}

/// 한 소스에서 걸리는 (줄번호, 리터럴). **순수 함수** — 합성 입력으로 변이를 찌른다.
///
/// 세 가지를 뺀다: 주석 줄 · 진단 호출 줄 · `#[cfg(test)]` 이후 전부. 마지막 것은
/// 테스트가 로케일 문구를 단정에 그대로 쓰는 것이 정상이기 때문이다(그 단정이 곧
/// "이 로케일에서 이렇게 나온다" 는 검증이다).
pub(crate) fn locale_specific_literals(src: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    for (idx, line) in src.lines().enumerate() {
        if line.starts_with("#[cfg(test)]") {
            break;
        }
        let t = line.trim_start();
        if t.starts_with("//") {
            continue;
        }
        if is_diagnostic(line) {
            continue;
        }
        for lit in string_literals(line) {
            if lit.chars().any(is_cjk) {
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
            } else if p.extension().is_some_and(|e| e == "rs")
                && !p.to_string_lossy().ends_with("_tests.rs")
            {
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
    let mut files = 0usize;
    let mut hits: Vec<String> = Vec::new();
    for dir in &srcs {
        for f in rs_files(dir) {
            files += 1;
            let src = std::fs::read_to_string(&f)
                .unwrap_or_else(|e| panic!("{} 을 읽지 못했다: {e}", f.display()))
                .replace("\r\n", "\n");
            let rel = f
                .strip_prefix(repo_root())
                .unwrap_or(&f)
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
        "plugin 소스를 {files} 개밖에 못 걸었다 — 파일 수집이 죽었다"
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

/// 이스케이프된 따옴표가 리터럴 경계를 흔들지 않는다.
#[test]
fn an_escaped_quote_does_not_split_the_literal() {
    let hits = locale_specific_literals("let s = \"그는 \\\"안녕\\\" 이라 했다\";");
    assert_eq!(hits.len(), 1, "리터럴이 쪼개졌다: {hits:?}");
}
