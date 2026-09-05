//! **debug 게이트된 핸들러는 모듈 선언에 cfg 가 붙은 파일에 산다** — 배치를 판정한다.
//!
//! 프로젝트 규칙(`docs/identity.md` 원칙 1 / `CLAUDE.md` "핵심 원칙" 1)은 두 문장으로
//! 돼 있고 **둘은 다른 물음**이다:
//!
//! - **판단 기준**(사람용): "에이전트가 자기 작업에 필요한가 vs 사용자 조작을 재현하는가".
//! - **집행 형태**(코드용): "debug 핸들러는 **모듈 선언에 cfg 가 붙은 별도 파일**로 모은다".
//!
//! 이 가드는 **뒤쪽만** 묻는다. 앞쪽은 의미 물음이라 텍스트로 안 갈리고, 갈리는 척하면
//! 이름 규약을 흉내 내게 된다(R475). 뒤쪽은 순수 배치 규칙이라 의미를 하나도 안 묻고
//! 판정된다 — 그래서 이 축은 만들 수 있고, 원칙 명부의 다른 [구두] 항목(2.3 의 셋)이
//! 못 만들어진 이유와 **다른 이유로** 못 만들어지고 있었다. 2.3 은 "보고 vs 선택" 이 같은
//! 식별자라 갈릴 수 없었고, 여기는 **의미 물음과 배치 물음을 안 갈랐던 것**뿐이다.
//!
//! # 판별식
//!
//! `src/adapters/ipc/` 아래 각 파일에서 `#[cfg(...debug_assertions...)]` 로 게이트된
//! **최상위 항목**을 센다. 그 파일이 아래 둘 중 하나면 통과다:
//!
//! - 파일 머리에 `#![cfg(debug_assertions)]` 가 있다(파일 통째가 debug).
//! - 부모 모듈 파일의 `mod <이름>;` 선언에 `debug_assertions` cfg 가 붙어 있다.
//!
//! 둘 다 아니면 **그 항목은 배치 축에서 노출된다** — 아래 [`KNOWN_PARENT_SITES`] 에
//! 자리로 등록돼 있지 않는 한 위반이다.
//!
//! # 이 가드가 단정하지 않는 것
//!
//! - **그 항목이 사용자 조작 재현인지.** 안 묻는다(위 참조). 배치만 본다.
//! - **release 에 안 나가는지.** 그건 `tests/ipc_release_table_excludes_input_reproduction.rs`
//!   가 `METHOD_TABLE` 로 답하는 다른 물음이다. 이 가드는 그것을 대체하지 않는다.
//! - **cfg 표현식의 의미.** `all(debug_assertions, feature = "gui")` 와 `debug_assertions`
//!   를 구별하지 않는다 — 둘 다 "debug 게이트" 로 센다. 그 구별이 필요한 자리는 아래
//!   허용 명부의 사유가 대신 적는다.

use std::path::{Path, PathBuf};

/// 스캔 뿌리 — 이 규칙이 말하는 영역.
const SCAN_ROOT: &str = "src/adapters/ipc";

/// 훑어야 할 최소 게이트 항목 수 — **모수가 살아 있다는 증거**.
///
/// 실측 30(2026-09-06). 여유를 두고 25 로 둔다 — 이것은 래칫이 아니라 **생존 바닥**이다.
/// 정당한 제거마다 수를 고치게 만들면 그 수정이 습관이 되고, 습관이 되면 스캔이 죽었을
/// 때도 같은 손이 움직인다(R499 가 경고하는 형태).
///
/// ★ 이 수를 **내려서 통과시키지 마라.** 내리면 "스캔이 죽었다" 와 "게이트된 항목이
/// 없다" 가 같은 초록이 된다. debug 핸들러가 실제로 줄어 이 하한이 걸리면, 값을 고치기
/// 전에 `rg 'cfg\(.*debug_assertions' src/adapters/ipc/` 로 **줄어든 자리를 먼저 세라**.
const MIN_GATED_ITEMS: usize = 25;

/// 부모 파일(모듈 선언에 cfg 가 없는 곳)에 사는 것이 **지금 허용되는** 자리.
///
/// 자리로 적는다 — 부류로 적으면 도망길이 된다(R505). 각 줄에 **왜 여기 있는가**를 붙인다.
const KNOWN_PARENT_SITES: &[(&str, &str)] = &[
    (
        "route_debug_handler",
        "라우팅 지점. 디스패치는 정의상 부모에 산다 — 자식으로 옮기면 부모가 자식을 부르고 \
         자식이 다시 부모의 표를 읽는 순환이 된다",
    ),
    (
        "handle_ui_state",
        "부채. gui 게이트 없는 debug 모듈이 없어서 부모에 있다 — 옮길 곳을 만들면 옮긴다",
    ),
    (
        "handle_debug_settings_apply",
        "부채. 같은 이유(headless 에서도 유효해야 하는데 `debug` 모듈은 gui 게이트다)",
    ),
    (
        "json_deep_merge",
        "부채. 위 핸들러의 헬퍼라 그것을 옮길 때 함께 옮긴다",
    ),
    (
        "PLATFORM_ONLY_MACOS_GUI",
        "라우팅 지점의 거절 문구. 쓰는 자리가 부모의 debug 라우터라 그 옆에 산다",
    ),
    (
        "handle_debug_gpu_stall",
        "부채. gui 게이트라 `debug` 모듈로 갈 수 있다 — 옮기는 것이 처방이다",
    ),
];

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rs_files(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path);
        }
    }
}

/// 그 줄이 debug 게이트 어트리뷰트인가.
///
/// `not(debug_assertions)` 는 **반대 방향**이다 — release 전용 코드이고 이 규칙의 대상이
/// 아니다. 그래서 그 형태를 먼저 지우고 남은 것을 본다. 안 지우면 release 전용 헬퍼가
/// debug 배치 위반으로 잡힌다(실측으로 그렇게 잡혔다).
fn is_debug_gate(line: &str) -> bool {
    let t = line.trim();
    if !(t.starts_with("#[cfg(") || t.starts_with("#![cfg(")) {
        return false;
    }
    t.replace("not(debug_assertions)", "")
        .contains("debug_assertions")
}

/// 파일 통째가 debug 인가 — 머리의 내부 어트리뷰트.
fn whole_file_is_debug(text: &str) -> bool {
    text.lines()
        .take_while(|l| {
            let t = l.trim();
            t.is_empty() || t.starts_with("//") || t.starts_with("#![")
        })
        .any(|l| l.trim().starts_with("#![cfg(") && l.contains("debug_assertions"))
}

/// 부모 모듈 파일에서 `mod <이름>;` 선언을 찾아 그 앞의 cfg 를 본다.
///
/// 부모는 형제 `<디렉토리>.rs` 또는 `<디렉토리>/mod.rs` 다. 못 찾으면 "cfg 없음" 으로
/// 센다 — 없는 쪽으로 세야 놓치지 않는다.
fn declaration_is_debug_gated(root: &Path, file: &Path) -> bool {
    let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
        return false;
    };
    let Some(dir) = file.parent() else {
        return false;
    };
    let candidates = [dir.with_extension("rs"), dir.join("mod.rs")];
    for parent in candidates {
        if !parent.starts_with(root) || parent == file {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&parent) else {
            continue;
        };
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim();
            let decl = t
                .trim_start_matches("pub(crate) ")
                .trim_start_matches("pub ");
            if decl != format!("mod {stem};") {
                continue;
            }
            // 선언 바로 위에 붙은 어트리뷰트 줄들을 거슬러 본다.
            let mut j = i;
            while j > 0 {
                j -= 1;
                let prev = lines[j].trim();
                if prev.starts_with("#[") {
                    if is_debug_gate(prev) {
                        return true;
                    }
                    continue;
                }
                if prev.starts_with("//") || prev.is_empty() {
                    continue;
                }
                break;
            }
        }
    }
    false
}

/// 한 파일의 **최상위** debug 게이트 항목 이름들 — 파일 순회와 분리된 판정기.
///
/// 최상위만 센다(들여쓰기 0). 함수 안의 게이트는 배치 물음이 아니라 그 함수 안의 분기다.
fn gated_items(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("#![") || !is_debug_gate(line) || line.starts_with(' ') {
            continue;
        }
        // 어트리뷰트 뒤의 첫 비-어트리뷰트/비-주석 줄이 그 항목이다.
        let mut j = i + 1;
        while j < lines.len() {
            let t = lines[j].trim();
            if t.starts_with("#[") || t.starts_with("//") || t.is_empty() {
                j += 1;
                continue;
            }
            break;
        }
        if j >= lines.len() {
            continue;
        }
        let head = lines[j].trim_start();
        // `mod x;` 선언은 **규칙이 요구하는 형태 그 자체**다 — 노출 대상이 아니라 통과 조건이다.
        let is_mod_decl = head
            .trim_start_matches("pub(crate) ")
            .trim_start_matches("pub ")
            .starts_with("mod ");
        let name = lines[j]
            .split(|c: char| !(c.is_alphanumeric() || c == '_'))
            .find(|w| !w.is_empty() && !matches!(*w, "pub" | "crate" | "fn" | "mod" | "const"))
            .unwrap_or("<이름 없음>")
            .to_string();
        out.push((
            i + 1,
            if is_mod_decl {
                format!("mod {name}")
            } else {
                name
            },
        ));
    }
    out
}

#[test]
fn every_debug_gated_item_lives_in_a_cfg_declared_file() {
    let root = repo_root();
    let mut files = Vec::new();
    rs_files(&root.join(SCAN_ROOT), &mut files);
    // 부모 파일 자신(`handler.rs`)도 대상이다.
    files.push(root.join("src/adapters/ipc/handler.rs"));
    files.sort();
    files.dedup();

    let mut gated = 0usize;
    let mut exposed = Vec::new();
    for file in &files {
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        let items = gated_items(&text);
        gated += items.len();
        if items.is_empty() || whole_file_is_debug(&text) || declaration_is_debug_gated(&root, file)
        {
            continue;
        }
        let rel = file
            .strip_prefix(&root)
            .unwrap_or(file)
            .display()
            .to_string();
        for (line, name) in items {
            // `mod` 선언은 그 자체가 게이트다 — 부모에 있는 것이 정상이다.
            if name.starts_with("mod ") || KNOWN_PARENT_SITES.iter().any(|(n, _)| *n == name) {
                continue;
            }
            exposed.push(format!("  {rel}:{line}  {name}"));
        }
    }

    assert!(
        gated >= MIN_GATED_ITEMS,
        "debug 게이트 항목을 {gated} 개만 찾았다(하한 {MIN_GATED_ITEMS}) — 스캔이 죽었거나 \
         게이트 형태가 바뀌었다. 그러면 아래 판정은 빈 집합을 훑고 조용히 통과한다. \
         ★ 수를 내려서 통과시키지 마라: 줄어든 자리를 먼저 세라."
    );

    assert!(
        exposed.is_empty(),
        "debug 로 게이트된 항목이 **모듈 선언에 cfg 가 없는 파일**에 있다:\n{}\n\n\
         규칙의 집행 형태는 배치다 — debug 전용 코드는 `#[cfg(debug_assertions)]` 가 붙은 \
         `mod` 선언을 가진 파일에 모은다. 그래야 그 파일을 통째로 지우는 것만으로 release \
         표면이 깨끗이 사라지는지 눈으로 확인된다.\n  \
         고치는 길 둘: (가) 그 항목을 이미 cfg 선언된 모듈로 옮겨라. (나) 옮길 곳이 없으면 \
         **새 debug 모듈을 만들어라** — `debug` 모듈은 gui 게이트라 headless 에서 사라지므로, \
         headless 에서도 살아야 하는 핸들러는 그쪽으로 못 간다. 선례가 있다: `debug_nav` · \
         `debug_terminal` · `debug_plugin` 은 gui 게이트 없이 `#[cfg(debug_assertions)]` 만 \
         붙어 선언돼 있다.\n  \
         ★ 라우팅 지점은 예외다(부모에 살아야 한다). 그런 자리는 이 파일의 \
         `KNOWN_PARENT_SITES` 에 **자리와 사유**로 등록한다 — 부류로 넓히지 마라.",
        exposed.join("\n")
    );
}

/// 허용 명부가 **살아 있는가** — 등록만 해 두고 자리가 사라진 항목을 잡는다.
///
/// 죽은 예외는 다음 사람에게 "이 부류는 봐준다" 로 읽힌다. 실제로 그 이름이 부모에
/// 남아 있을 때만 명부에 있어야 한다.
#[test]
fn every_allowed_parent_site_still_exists() {
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("src/adapters/ipc/handler.rs"))
        .expect("handler.rs 를 읽지 못했다");
    let names: Vec<String> = gated_items(&text).into_iter().map(|(_, n)| n).collect();
    // `mod` 선언은 허용 명부의 대상이 아니다 — 위 판정에서 이미 통과 조건이다.
    let dead: Vec<&str> = KNOWN_PARENT_SITES
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !names.contains(&(*n).to_string()))
        .collect();
    assert!(
        dead.is_empty(),
        "허용 명부에 있는데 부모 파일에 그 자리가 없다: {dead:?}\n  \
         옮겼으면 명부에서도 지워라 — 남겨 두면 다음에 같은 이름이 부모에 생겼을 때 \
         조용히 통과한다."
    );
}

/// 판독기가 **양쪽 답을 다 낸다**.
#[test]
fn the_reader_answers_both_yes_and_no() {
    let gated = "#[cfg(debug_assertions)]\nfn handle_x() {}\n";
    assert_eq!(gated_items(gated), vec![(1, "handle_x".to_string())]);

    let plain = "fn handle_x() {}\n";
    assert!(gated_items(plain).is_empty());

    let feature_only = "#[cfg(feature = \"gui\")]\nfn handle_x() {}\n";
    assert!(gated_items(feature_only).is_empty());

    let combined = "#[cfg(all(debug_assertions, feature = \"gui\"))]\nmod debug;\n";
    assert_eq!(gated_items(combined), vec![(1, "mod debug".to_string())]);

    // 반대 방향 — release 전용은 이 규칙의 대상이 아니다.
    let release_only = "#[cfg(not(debug_assertions))]\nfn only_in_release() {}\n";
    assert!(gated_items(release_only).is_empty());
}

/// 함수 **안**의 게이트는 배치 물음이 아니다 — 들여쓰기로 가른다.
#[test]
fn a_gate_inside_a_function_is_not_a_placement_question() {
    let text = "fn handle_request() {\n    #[cfg(debug_assertions)]\n    let x = 1;\n}\n";
    assert!(gated_items(text).is_empty());
}

/// 파일 통째가 debug 인 형태를 알아본다 — `popup.rs` 가 그 형태다.
#[test]
fn an_inner_attribute_marks_the_whole_file() {
    let text = "//! doc\n\n#![cfg(debug_assertions)]\n\nfn f() {}\n";
    assert!(whole_file_is_debug(text));
    assert!(!whole_file_is_debug("fn f() {}\n"));
}
