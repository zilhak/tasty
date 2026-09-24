//! 공유 로그 파일은 host로 역할을 결정한 뒤에만 열어야 한다.
//! CLI도 같은 바이너리를 사용하므로 공통 초기화에서 열면 실행 중인 host의 로그를 덮어쓸 수 있다.
//! 호출 위치와 분기를 소스에서 검사한다([ADR-0043](../../../docs/adr/0043-cli-errors-and-diagnostic-logs.md)).

use std::path::{Path, PathBuf};

/// 구현 파일 전체가 아니라 파일을 여는 함수 본문만 허용한다.
const IMPL_FILE: &str = "crates/tasty-platform/src/crash_report.rs";
/// boot 위임 래퍼도 해당 함수 본문만 허용한다.
const WRAPPER_FILE: &str = "src/boot/os.rs";
/// host 분기 안에 있는지 별도로 확인할 호출 파일.
const CALL_SITE_FILE: &str = "src/boot.rs";

const OPEN_FN: &str = "enable_host_file_log";

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// 줄 시작의 주석 표지를 찾는다. 전체 Rust 문법은 해석하지 않는다.
fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with('*')
}

/// 첫 // 뒤를 제거한다. 문자열 안의 //도 구별하지 않는다.
fn strip_line_comment(line: &str) -> &str {
    match line.find("//") {
        Some(i) => &line[..i],
        None => line,
    }
}

/// 헤더 이후 중괄호 깊이로 범위를 구한다. 단순한 텍스트 순서만 비교하면 뒤의 무관한 함수도 통과할 수 있다.
fn span_of(lines: &[&str], header: &str) -> Option<(usize, usize)> {
    let start = lines
        .iter()
        .position(|l| !is_comment_line(l) && l.contains(header))?;
    let (mut depth, mut opened) = (0i32, false);
    for (i, line) in lines.iter().enumerate().skip(start) {
        for ch in strip_line_comment(line).chars() {
            match ch {
                '{' => {
                    depth += 1;
                    opened = true;
                }
                '}' => depth -= 1,
                _ => {}
            }
        }
        if opened && depth <= 0 {
            return Some((start, i));
        }
    }
    None
}

fn fn_span(lines: &[&str], name: &str) -> Option<(usize, usize)> {
    span_of(lines, &format!("fn {name}("))
}

fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn log_file_is_opened_from_the_host_path_only() {
    let root = repo_root();
    let mut files = Vec::new();
    collect_rs_files(&root.join("src"), &mut files);
    // 로그 구현이 있는 플랫폼 크레이트도 함께 수집한다.
    collect_rs_files(&root.join("crates/tasty-platform/src"), &mut files);
    assert!(
        !files.is_empty(),
        "src와 tasty-platform/src에서 Rust 파일을 수집하지 못했다"
    );

    let mut offenders: Vec<String> = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        // 호출 파일은 host 분기 검사를 따로 적용한다.
        if rel == CALL_SITE_FILE {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        let allowed = if rel == IMPL_FILE || rel == WRAPPER_FILE {
            Some(fn_span(&lines, OPEN_FN).unwrap_or_else(|| {
                panic!("`{rel}` 에서 `fn {OPEN_FN}` 정의 범위를 못 찾았다 — 가드를 갱신한다")
            }))
        } else {
            None
        };
        for (i, line) in lines.iter().enumerate() {
            if !line.contains(OPEN_FN) || is_comment_line(line) {
                continue;
            }
            if allowed.is_some_and(|(start, end)| i >= start && i <= end) {
                continue;
            }
            offenders.push(format!("{rel}:{}: {}", i + 1, line.trim()));
        }
    }

    assert!(
        offenders.is_empty(),
        "{OPEN_FN} 호출이 허용 범위 밖에 있다:\n{}\nCLI가 host 로그를 열지 않도록 {CALL_SITE_FILE}에서 host 역할을 결정한 뒤 호출한다(ADR-0043).",
        offenders.join("\n")
    );
}

#[test]
fn the_call_site_sits_inside_the_host_arm() {
    let text = std::fs::read_to_string(repo_root().join(CALL_SITE_FILE))
        .expect("호출처 파일을 읽을 수 있어야 한다");
    let lines: Vec<&str> = text.lines().collect();

    let (arm_start, arm_end) = span_of(&lines, "Routed::Gui(cli) =>")
        .expect("`Routed::Gui` 분기 블록을 못 찾았다 — 라우팅 형태가 바뀌었으면 가드도 갱신한다");

    let calls: Vec<usize> = lines
        .iter()
        .enumerate()
        .filter(|(_, l)| !is_comment_line(l) && l.contains(&format!("{OPEN_FN}()")))
        .map(|(i, _)| i)
        .collect();

    assert_eq!(
        calls.len(),
        1,
        "`{OPEN_FN}()` 호출은 host 경로 한 곳뿐이어야 한다 — 찾은 줄: {:?}",
        calls.iter().map(|i| i + 1).collect::<Vec<_>>()
    );

    let call = calls[0];
    assert!(
        call > arm_start && call <= arm_end,
        "{OPEN_FN} 호출이 host 분기({}~{}행) 밖의 {}행에 있다. CLI가 로그 파일을 열지 않도록 Routed::Gui 안에서 호출한다.",
        arm_start + 1,
        arm_end + 1,
        call + 1
    );
}

/// 로그 파일명 사용이 파일 열기 함수의 정의보다 앞에 나오지 않는지 확인한다.
/// 이 검사는 텍스트 순서만 보므로 그 함수 본문 안에 있는지까지 보장하지 않는다.
#[test]
fn tracing_init_does_not_open_the_log_file() {
    let text = std::fs::read_to_string(repo_root().join(IMPL_FILE))
        .expect("구현 파일을 읽을 수 있어야 한다");
    let open_fn_at = text
        .find(&format!("pub fn {OPEN_FN}"))
        .unwrap_or_else(|| panic!("`{OPEN_FN}` 정의를 못 찾았다"));

    for (offset, _) in text.match_indices("log_file_name()") {
        let line_start = text[..offset].rfind('\n').map(|i| i + 1).unwrap_or(0);
        if text[line_start..offset].contains("fn ") {
            continue;
        }
        assert!(
            offset > open_fn_at,
            "{IMPL_FILE}에서 로그 파일명을 {OPEN_FN} 정의보다 앞에서 사용한다. 역할 판정 전 초기화에서 파일을 열지 않는지 확인한다(ADR-0043)."
        );
    }
}
