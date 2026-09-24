//! 플러그인 팝업 닫기가 공용 큐를 우회하지 않도록 직접 호출을 검사한다.
//! 큐를 처리할 때 자식 파일 피커도 취소하므로 매니저를 바로 부르면 자식 정리가 빠진다(ADR-0036).
//! 사유별 정리 동작은 src/state/popup_ownership_tests.rs에서 검사한다.
//! 여기서는 주석 줄을 제외한 호출 문자열과 진입 파일의 공용 함수 이름을 확인한다.

use std::path::{Path, PathBuf};

/// 직접 닫기를 허용할 파일. 큐를 처리한 뒤 매니저에 전달한다.
/// 창 상태가 하나도 없는 fallback은 큐 처리도 자식 피커 정리도 필요하지 않은 예외다.
const CHOKEPOINT_FILE: &str = "src/app/dispatch/plugin_popup_events.rs";

const GLUE_FN: &str = "enqueue_plugin_popup_close";

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// 주석으로 시작하는 줄을 제외한다. 문자열 내부까지 구별하는 렉서는 아니다.
fn is_comment_line(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("//") || t.starts_with('*')
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
fn close_popup_instance_is_only_called_from_the_drain() {
    let root = repo_root();
    let src = root.join("src");
    let mut files = Vec::new();
    collect_rs_files(&src, &mut files);
    assert!(
        !files.is_empty(),
        "src 아래에서 Rust 파일을 찾지 못했다. 경로와 순회를 확인한다."
    );

    let mut offenders: Vec<String> = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .to_string_lossy()
            .replace('\\', "/");
        if rel == CHOKEPOINT_FILE {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if line.contains("close_popup_instance(") && !is_comment_line(line) {
                offenders.push(format!("{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "close_popup_instance 직접 호출은 {CHOKEPOINT_FILE}에서만 허용한다. 자식 file_picker 정리를 거치도록 App::{GLUE_FN}을 사용한다(ADR-0036):\n{}",
        offenders.join("\n")
    );
}

/// main view와 parked state 정리를 위한 호출 문자열 수를 확인한다.
#[test]
fn the_drain_still_runs_the_cascade_cleanup() {
    let text = std::fs::read_to_string(repo_root().join(CHOKEPOINT_FILE))
        .expect("초크포인트 파일을 읽을 수 있어야 한다");
    let calls = text
        .lines()
        .filter(|l| l.contains("cancel_child_file_picker(") && !is_comment_line(l))
        .count();
    assert!(
        calls >= 2,
        "drain 이 `cancel_child_file_picker` 를 부르지 않는다 — \
         main view / parked state 양쪽 순회에 각각 있어야 한다 (found {calls})"
    );
}

/// 일반 popup.close와 debug.popup.close 진입 파일에 공용 함수 이름이 있는지 확인한다.
#[test]
fn both_close_entry_points_go_through_the_glue() {
    let root = repo_root();
    for (rel, what) in [
        ("src/app/dispatch/plugin_ipc.rs", "plugin 의 popup.close"),
        ("src/app/ipc/debug_methods.rs", "debug.popup.close"),
    ] {
        let text = std::fs::read_to_string(root.join(rel))
            .unwrap_or_else(|e| panic!("{rel} 를 읽을 수 없다: {e}"));
        assert!(
            text.contains(GLUE_FN),
            "{rel} ({what})에서 {GLUE_FN} 이름을 찾지 못했다. 자식 피커 정리를 거치는 호출 경로인지 확인한다(ADR-0036)."
        );
    }
}
