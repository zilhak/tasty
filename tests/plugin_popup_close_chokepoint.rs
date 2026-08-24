//! plugin popup close 단일 초크포인트 가드.
//!
//! 배경: plugin popup 이 닫힐 때 자식 host `file_picker` 를 함께 취소하는 연쇄 정리
//! ([ADR-0084](../docs/adr/0084-plugin-triggered-host-popup-ownership.md) Decision 3)는
//! `App::dispatch_plugin_popup_events` 가 `AppState.plugin_popup_closes` 큐를 drain 할 때
//! `cancel_child_file_picker` 를 태우는 방식으로만 돈다. 그래서 **큐를 거치지 않고
//! `PluginManager::close_popup_instance` 를 직접 부르는 호출처는 그 정리를 통째로
//! 건너뛴다** — 실제로 plugin 자신의 `popup.close` 경로가 그랬고, 자식 피커가 부모 없이
//! 떠 있는 고아가 됐다. 단위 테스트가 `cancel_child_file_picker` 를 직접 호출해 사유
//! (`PopupCloseReason`) 무관함만 확인하고 있었기 때문에 "경로가 그 함수에 안 닿는다" 는
//! 진짜 결함은 잡히지 않았다.
//!
//! 이 가드는 그 결함의 **모양**(초크포인트 우회 호출처가 하나 더 생김)을 소스 수준에서
//! 막는다. 사유별 동작은 `src/state/popup_ownership_tests.rs` 가, 큐 합류 자체는
//! `App::enqueue_plugin_popup_close` 가 담당한다.
//!
//! 선례: `tests/no_emoji_in_source.rs` / `tests/design_token_adherence.rs`.

use std::path::{Path, PathBuf};

/// `close_popup_instance` 를 직접 불러도 되는 유일한 파일(repo-relative).
/// - drain 본체(`dispatch_plugin_popup_events`) — 연쇄 정리를 태운 **뒤** 매니저에 forward.
/// - `enqueue_plugin_popup_close` 의 no-window fallback — 큐를 가진 state 가 하나도 없어
///   drain 이 돌지 않는 경우로, 정리할 자식 피커도 함께 사라진 상황이다.
const CHOKEPOINT_FILE: &str = "src/app/dispatch/plugin_popup_events.rs";

/// 큐로 합류시키는 App-level glue. 우회 호출처를 고칠 때 안내할 이름.
const GLUE_FN: &str = "enqueue_plugin_popup_close";

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 주석 줄인가 — `//`, `///`, `//!`, 블록 주석 본문(`*`). 문서에서 이름을 언급하는 것은
/// 호출이 아니므로 스캔에서 뺀다.
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
        "src/ 아래 .rs 파일을 하나도 못 찾았다 — 가드가 헛돈다"
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
        "`close_popup_instance` 직접 호출은 `{CHOKEPOINT_FILE}` 밖에서 금지다 — \
         큐를 건너뛰면 `cancel_child_file_picker` 연쇄 정리(ADR-0084)가 안 돌아 자식 \
         `file_picker` 가 고아로 남는다. `App::{GLUE_FN}` 로 바꿔라:\n{}",
        offenders.join("\n")
    );
}

/// 초크포인트가 실제로 연쇄 정리를 태우는지 — drain 안에 `cancel_child_file_picker`
/// 호출이 남아 있어야 위 가드가 지키는 대상이 의미를 갖는다.
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

/// plugin 자신의 `popup.close`(release 경로)와 debug 강제 close 가 **둘 다** glue 를
/// 거치는지. 이 둘이 매니저를 직접 치던 것이 원 결함이었고, debug 쪽이 release 와 다른
/// 코드를 타면 debug IPC 로 하는 재현 검증 자체가 실제 동작을 못 비춘다.
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
            "{rel} ({what}) 가 `{GLUE_FN}` 을 거치지 않는다 — \
             매니저를 직접 치면 연쇄 정리를 건너뛴다(ADR-0084)"
        );
    }
}
