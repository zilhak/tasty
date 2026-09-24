//! IPC의 최상위 debug 조건부 항목이 별도 모듈에 모여 있는지 확인한다.
//! 파일 머리의 내부 cfg나 부모의 mod 선언을 읽고, 예외는 이름과 사유로 등록한다.
//! 사용자 조작 재현인지의 의미 판단과 release 빌드 제외 여부는 검사하지 않는다.
//! release 메서드 목록은 tests/ipc_release_table_excludes_input_reproduction.rs에서 별도로 확인한다.
//! cfg는 문자열로 판독하며 논리식을 계산하지 않는다. 배치 규칙은 docs/identity.md의 원칙 1을 따른다.

use std::path::{Path, PathBuf};
use tasty_doc_guards::temp_scratch::Scratch;

const SCAN_ROOT: &str = "src/adapters/ipc";

/// 2026-09-06 실측 30개에 여유를 둔 하한 25다. 미달하면 실제 항목 감소와 추출 실패를 구별한다.
const MIN_GATED_ITEMS: usize = 25;

/// 부모에 남겨야 하는 라우팅 항목만 이름과 사유로 허용한다.
const KNOWN_PARENT_SITES: &[(&str, &str)] = &[
    (
        "route_debug_handler",
        "라우팅 지점. 디스패치는 정의상 부모에 산다 — 자식으로 옮기면 부모가 자식을 부르고 \
         자식이 다시 부모의 표를 읽는 순환이 된다",
    ),
    (
        "PLATFORM_ONLY_MACOS_GUI",
        "라우팅 지점의 거절 문구. 쓰는 자리가 부모의 debug 라우터라 그 옆에 산다",
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

/// not(debug_assertions) 문자열을 제거한 뒤 debug_assertions가 남는지 본다. cfg 논리식을 계산하지 않는다.
fn is_debug_gate(line: &str) -> bool {
    let t = line.trim();
    if !(t.starts_with("#[cfg(") || t.starts_with("#![cfg(")) {
        return false;
    }
    t.replace("not(debug_assertions)", "")
        .contains("debug_assertions")
}

/// 파일 머리의 내부 cfg에서 debug_assertions 문자열을 찾는다. 부정 조건까지 구별하지는 않는다.
fn whole_file_is_debug(text: &str) -> bool {
    text.lines()
        .take_while(|l| {
            let t = l.trim();
            t.is_empty() || t.starts_with("//") || t.starts_with("#![")
        })
        .any(|l| l.trim().starts_with("#![cfg(") && l.contains("debug_assertions"))
}

/// 형제 디렉터리명.rs 또는 디렉터리/mod.rs에서 선언의 cfg를 찾는다. 못 찾으면 없음으로 판정한다.
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

/// 공백으로 들여쓴 항목을 제외해 최상위 항목만 추출한다.
fn gated_items(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("#![") || !is_debug_gate(line) || line.starts_with(' ') {
            continue;
        }
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
        // cfg가 붙은 mod 선언은 분리 규칙을 충족하는 항목으로 구분한다.
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
        let rel =
            tasty_doc_guards::source_text::repo_relative(file.strip_prefix(&root).unwrap_or(file))
                .display()
                .to_string();
        for (line, name) in items {
            if name.starts_with("mod ") || KNOWN_PARENT_SITES.iter().any(|(n, _)| *n == name) {
                continue;
            }
            exposed.push(format!("  {rel}:{line}  {name}"));
        }
    }

    assert!(
        gated >= MIN_GATED_ITEMS,
        "debug 조건부 항목을 {gated}개만 읽었다(하한 {MIN_GATED_ITEMS}). 실제 항목 수와 cfg 형태를 확인한 뒤 하한 변경 여부를 판단한다."
    );

    assert!(
        exposed.is_empty(),
        "debug 조건부 항목이 분리되지 않은 파일에 있다:\n{}\ncfg가 붙은 모듈로 옮기거나 새 모듈을 만든다. 헤드리스에서도 필요한 핸들러를 gui 전용 모듈로 옮기면 안 된다. 부모에 필요한 라우팅 항목만 KNOWN_PARENT_SITES에 이름과 사유를 등록한다.",
        exposed.join("\n")
    );
}

/// 오래된 예외가 같은 이름의 새 항목을 숨기지 않도록 부모에 항목이 남아 있는지 확인한다.
#[test]
fn every_allowed_parent_site_still_exists() {
    let root = repo_root();
    let text = std::fs::read_to_string(root.join("src/adapters/ipc/handler.rs"))
        .expect("handler.rs 를 읽지 못했다");
    let names: Vec<String> = gated_items(&text).into_iter().map(|(_, n)| n).collect();
    let dead: Vec<&str> = KNOWN_PARENT_SITES
        .iter()
        .map(|(n, _)| *n)
        .filter(|n| !names.contains(&(*n).to_string()))
        .collect();
    assert!(
        dead.is_empty(),
        "부모 파일에서 사라진 예외다: {dead:?}. 이동한 항목의 예외를 제거해 같은 이름의 새 항목이 통과하지 않게 한다."
    );
}

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

    let release_only = "#[cfg(not(debug_assertions))]\nfn only_in_release() {}\n";
    assert!(gated_items(release_only).is_empty());
}

#[test]
fn a_gate_inside_a_function_is_not_a_placement_question() {
    let text = "fn handle_request() {\n    #[cfg(debug_assertions)]\n    let x = 1;\n}\n";
    assert!(gated_items(text).is_empty());
}

#[test]
fn an_inner_attribute_marks_the_whole_file() {
    let text = "//! doc\n\n#![cfg(debug_assertions)]\n\nfn f() {}\n";
    assert!(whole_file_is_debug(text));
    assert!(!whole_file_is_debug("fn f() {}\n"));
}

/// 실제 IPC 파일과 무관한 합성 트리로 순회 범위와 두 부모 모듈 형식을 확인한다.
#[test]
fn the_walk_and_the_declaration_reader_answer_on_a_substituted_tree() {
    let probe = Scratch::new("debug-placement");
    let root = probe.path();
    std::fs::create_dir_all(root.join("outer/inner")).expect("합성 트리를 만들지 못했다");

    std::fs::write(
        root.join("outer.rs"),
        "#[cfg(debug_assertions)]\n\
         mod gated;\n\
         mod after_gated;\n\
         pub(crate) mod plain;\n\
         mod inner;\n",
    )
    .expect("합성 부모(.rs) 실패");
    std::fs::write(
        root.join("outer/inner/mod.rs"),
        "#[cfg(not(debug_assertions))]\n\
         pub mod release_only;\n\
         \n\
         #[cfg(debug_assertions)]\n\
         // 선언과 어트리뷰트 사이의 주석\n\
         \n\
         mod spaced;\n",
    )
    .expect("합성 부모(mod.rs) 실패");

    for f in [
        "outer/gated.rs",
        "outer/after_gated.rs",
        "outer/plain.rs",
        "outer/inner/release_only.rs",
        "outer/inner/spaced.rs",
        "outer/inner/orphan.rs",
    ] {
        std::fs::write(root.join(f), "fn x() {}\n").expect("합성 모듈 실패");
    }
    std::fs::write(root.join("outer/notes.md"), "#[cfg(debug_assertions)]\n")
        .expect("합성 문서 실패");

    let mut found = Vec::new();
    rs_files(root, &mut found);
    let mut rels: Vec<String> = found
        .iter()
        .map(|p| {
            tasty_doc_guards::source_text::repo_relative(p.strip_prefix(&root).unwrap_or(p))
                .display()
                .to_string()
        })
        .collect();
    rels.sort();
    assert_eq!(
        rels,
        vec![
            "outer.rs".to_string(),
            "outer/after_gated.rs".to_string(),
            "outer/gated.rs".to_string(),
            "outer/inner/mod.rs".to_string(),
            "outer/inner/orphan.rs".to_string(),
            "outer/inner/release_only.rs".to_string(),
            "outer/inner/spaced.rs".to_string(),
            "outer/plain.rs".to_string(),
        ],
        "순회가 합성 트리에서 다른 답을 냈다"
    );
    assert!(
        !rels.iter().any(|r| r.ends_with(".md")),
        "`.rs` 가 아닌 파일을 모았다 — 문서가 이 어트리뷰트를 인용만 해도 판정에 들어온다"
    );

    let gated = |rel: &str| declaration_is_debug_gated(root, &root.join(rel));

    assert!(
        gated("outer/gated.rs"),
        "형제 `<디렉토리>.rs` 부모에서 게이트된 선언을 못 찾았다"
    );
    assert!(
        gated("outer/inner/spaced.rs"),
        "어트리뷰트와 선언 사이의 주석·빈 줄을 거슬러 못 올라갔다 — 이 형태가 흔하다"
    );
    assert!(
        !gated("outer/after_gated.rs"),
        "앞 모듈 선언의 cfg를 뒤 선언의 조건으로 잘못 읽었다"
    );
    assert!(
        !gated("outer/plain.rs"),
        "cfg 가 없는 선언을 게이트된 것으로 셌다"
    );
    assert!(
        !gated("outer/inner/release_only.rs"),
        "`not(debug_assertions)` 를 debug 게이트로 셌다 — 그것은 반대 방향(release 전용)이다"
    );
    assert!(
        !gated("outer/inner/orphan.rs"),
        "부모 선언이 없는 파일을 cfg로 제한된 모듈로 판정했다"
    );
    assert!(
        !gated("outer/inner/mod.rs"),
        "`mod.rs` 가 자기 자신을 부모로 읽었다"
    );
}
