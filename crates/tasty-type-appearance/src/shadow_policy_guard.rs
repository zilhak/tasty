//! 그림자 리터럴의 위치·기하 재대입·접근자 목록을 검사한다.
//! 줄 단위 검사이므로 별칭과 다른 생성·대입 형태까지 추적하지 않는다. 색 재대입은 허용한다.
//! 토큰 값은 shadow_parity가 검사하며 egui 기본 그림자 대입과 표면별 그림자 선택은 이 검사 범위 밖이다.
//! 선택 기준은 docs/design/systems/theme.md#떠-있는-표면의-그림자를 따른다.
//!
//! CI의 --lib --bins에도 실행되도록 lib 검사로 둔다. egui 없이 소스만 읽는다.
//! 컴파일 때의 CARGO_MANIFEST_DIR를 기준으로 읽으며 파일 수 하한으로 빈 수집을 거부한다.
//! 합성 입력으로 허용·거부 조건을 확인하고, 검사 파일 자체는 합성 코드 때문에 제외한다.

use std::fs;
use std::path::{Path, PathBuf};

/// 경로 오류나 불완전한 수집을 잡는 최소 파일 수. 파일 재구성에 여유를 둔 하한이다.
const SCAN_FILE_FLOOR: usize = 800;

/// 기하 재대입을 추적할 접근자 목록. 실제 Theme 접근자와도 대조한다.
const SHADOW_ACCESSORS: &[&str] = &["shadow_popover", "shadow_modal"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .canonicalize()
        .expect("repo root")
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_rs(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

fn scan_files() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = Vec::new();
    collect_rs(&root.join("src"), &mut files);
    if let Ok(cr) = fs::read_dir(root.join("crates")) {
        for e in cr.flatten() {
            let src = e.path().join("src");
            if src.is_dir() {
                collect_rs(&src, &mut files);
            }
        }
    }
    // 이 파일의 합성 위반 코드가 실제 위반으로 잡히므로 제외한다.
    // 제외가 필요한지는 합성 검사로 확인하며 이 파일에 제품 렌더 코드를 추가하지 않는다.
    let self_file = repo_root().join("crates/tasty-type-appearance/src/shadow_policy_guard.rs");
    files.retain(|f| f != &self_file);
    files
}

/// Shadow 구조체 생성 줄을 찾는다. 배경 scrim이나 다른 생성 함수는 검사하지 않는다.
/// 반환 타입 앞의 화살표만 제외해야 같은 줄의 실제 생성을 놓치지 않는다.
fn is_shadow_literal(line: &str) -> bool {
    let t = line.trim_end();
    if t.contains("= Shadow {") {
        return true;
    }
    for pat in ["egui::Shadow {", "epaint::Shadow {"] {
        if let Some(pos) = t.find(pat) {
            let before = &t[..pos];
            let is_return_type = before
                .rfind("->")
                .is_some_and(|a| !before[a..].contains('='));
            if !is_return_type {
                return true;
            }
        }
    }
    false
}

/// to_egui 함수의 시작과 끝을 중괄호 깊이로 찾아 허용 범위를 정한다.
fn to_egui_range(theme_src: &str) -> (usize, usize) {
    let lines: Vec<&str> = theme_src.lines().collect();
    let start = lines
        .iter()
        .position(|l| l.contains("fn to_egui"))
        .expect("to_egui 함수 정의를 찾지 못했다");
    let mut depth: i32 = 0;
    let mut opened = false;
    for (i, l) in lines.iter().enumerate().skip(start) {
        depth += l.matches('{').count() as i32;
        depth -= l.matches('}').count() as i32;
        if l.contains('{') {
            opened = true;
        }
        if opened && depth == 0 {
            return (start, i);
        }
    }
    panic!("to_egui 함수 끝을 찾지 못함");
}

/// 판정1: `Shadow {}` 리터럴이 허용 범위(theme.rs 의 `to_egui`) 밖이면 위반.
/// 반환 `(위반 목록, 허용 범위 내 리터럴 수)`.
fn shadow_literal_violations(
    rel: &str,
    text: &str,
    allowed_range: Option<(usize, usize)>,
) -> (Vec<String>, usize) {
    let mut v = Vec::new();
    let mut allowed = 0;
    for (i, line) in text.lines().enumerate() {
        if is_shadow_literal(line) {
            if allowed_range.is_some_and(|(s, e)| i >= s && i <= e) {
                allowed += 1;
            } else {
                v.push(format!("{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }
    (v, allowed)
}

/// 같은 줄의 let mut와 접근자 이름으로 변수를 찾고 offset·blur·spread 재대입을 검사한다.
/// color 변경은 페이드에 필요하므로 허용한다.
fn geom_reassign_violations(rel: &str, text: &str) -> Vec<String> {
    let mut vars: Vec<String> = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("let mut ") && SHADOW_ACCESSORS.iter().any(|a| t.contains(a)) {
            let name: String = t["let mut ".len()..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                vars.push(name);
            }
        }
    }
    let mut v = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        for var in &vars {
            for field in ["offset", "blur", "spread"] {
                let assign = format!("{var}.{field} =");
                let eq = format!("{var}.{field} ==");
                if t.contains(&assign) && !t.contains(&eq) {
                    v.push(format!("{rel}:{}: {}", i + 1, t));
                }
            }
        }
    }
    v
}

fn rel_of(root: &Path, f: &Path) -> String {
    f.strip_prefix(root)
        .unwrap_or(f)
        .to_string_lossy()
        .replace('\\', "/")
}

/// 인식 가능한 Shadow 생성이 공용 변환기 밖에 있는지 확인한다.
#[test]
fn shadow_creation_is_confined_to_the_token_converter() {
    let root = repo_root();
    let files = scan_files();
    assert!(
        files.len() >= SCAN_FILE_FLOOR,
        "스캔 파일 {}개가 하한 {}개보다 적다. 저장소 경로와 수집 결과를 확인한다.",
        files.len(),
        SCAN_FILE_FLOOR
    );

    let theme_rs = root.join("crates/tasty-type-appearance/src/theme.rs");
    let theme_src = fs::read_to_string(&theme_rs).expect("theme.rs");
    let range = to_egui_range(&theme_src);

    let mut violations = Vec::new();
    let mut allowed_total = 0;
    for f in &files {
        let rel = rel_of(&root, f);
        let text = fs::read_to_string(f).unwrap_or_default();
        let allowed_range = if *f == theme_rs { Some(range) } else { None };
        let (v, allowed) = shadow_literal_violations(&rel, &text, allowed_range);
        violations.extend(v);
        allowed_total += allowed;
    }

    assert!(
        violations.is_empty(),
        "공용 변환기 밖에서 Shadow를 생성했다. theme.shadow_popover()/shadow_modal()을 통해 변환해야 한다:\n{}",
        violations.join("\n")
    );
    assert_eq!(
        allowed_total, 1,
        "to_egui 안의 Shadow 생성이 정확히 1개여야 한다"
    );
}

/// 변환 결과의 offset·blur·spread를 다시 바꾸는 경우도 거부한다.
#[test]
fn shadow_accessor_result_is_not_geometrically_overridden() {
    let root = repo_root();
    let mut violations = Vec::new();
    for f in scan_files() {
        let rel = rel_of(&root, &f);
        let text = fs::read_to_string(&f).unwrap_or_default();
        violations.extend(geom_reassign_violations(&rel, &text));
    }
    assert!(
        violations.is_empty(),
        "그림자 접근자 결과의 기하 필드를 재대입한다(허용 함수 통과 ≠ 허용 값):\n{}",
        violations.join("\n")
    );
}

#[test]
fn mutation_catches_shadow_literal_outside_converter() {
    let text = "fn draw() {\n    let s = egui::Shadow {\n        blur: 4,\n    };\n}\n";
    let (v, _) = shadow_literal_violations("x.rs", text, None);
    assert_eq!(v.len(), 1, "범위 밖 Shadow 리터럴을 잡아야 한다");
}

#[test]
fn mutation_discriminates_when_converter_moves_out_of_range() {
    // to_egui 는 비어 있고(1~2줄) Shadow{} 는 다른 함수에 있다 = to_egui 를 옮긴 형태.
    let text =
        "pub fn to_egui() {\n}\n\nfn other() {\n    egui::Shadow {\n        blur: 1,\n    };\n}\n";
    let range = to_egui_range(text);
    let (v, allowed) = shadow_literal_violations("theme.rs", text, Some(range));
    assert_eq!(
        allowed, 0,
        "to_egui 밖으로 나간 리터럴은 허용 카운트에 안 든다"
    );
    assert_eq!(v.len(), 1, "to_egui 밖 Shadow 리터럴은 위반");
}

/// 실제 Theme 접근자와 검사 목록을 대조해 빠진 접근자를 찾는다.
#[test]
fn shadow_accessor_list_matches_theme_accessors() {
    let theme_src =
        fs::read_to_string(repo_root().join("crates/tasty-type-appearance/src/theme.rs"))
            .expect("theme.rs");
    let mut found: Vec<String> = Vec::new();
    for line in theme_src.lines() {
        let t = line.trim();
        // `pub fn shadow_xxx(&self) -> ShadowToken {`
        if let Some(rest) = t.strip_prefix("pub fn shadow_") {
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if !name.is_empty() {
                found.push(format!("shadow_{name}"));
            }
        }
    }
    found.sort();
    found.dedup();
    let mut listed: Vec<String> = SHADOW_ACCESSORS.iter().map(|s| (*s).to_string()).collect();
    listed.sort();
    assert_eq!(
        found, listed,
        "theme.rs의 shadow_*() 접근자와 SHADOW_ACCESSORS가 다르다. 빠진 접근자는 재대입을 검사하지 못한다."
    );
}

#[test]
fn mutation_catches_geometry_reassignment() {
    let text = "let mut shadow = theme.shadow_popover().to_egui();\nshadow.offset = [1, 2];\n";
    assert_eq!(
        geom_reassign_violations("x.rs", text).len(),
        1,
        "기하 재대입은 위반"
    );
}

#[test]
fn mutation_allows_color_fade_on_popover_shadow() {
    let text = "let mut shadow = theme.shadow_popover().to_egui();\n\
                shadow.color = shadow.color.gamma_multiply(opacity);\n";
    assert!(
        geom_reassign_violations("x.rs", text).is_empty(),
        "color 페이드는 허용해야 한다"
    );
}

#[test]
fn mutation_arrow_on_line_does_not_hide_a_real_creation() {
    // 반환 화살표가 같은 줄에 있어도 그 뒤의 구조체 생성은 검출해야 한다.
    let hidden = "    let f = || -> u32 { let s = egui::Shadow { blur: 4 }; 0 };";
    assert!(
        is_shadow_literal(hidden),
        "줄에 `->` 가 있어도 대입(=) 뒤 실제 생성은 위반이다"
    );
    let return_type = "    pub fn to_egui(self) -> egui::epaint::Shadow {";
    assert!(
        !is_shadow_literal(return_type),
        "반환 타입 선언은 구조체 생성으로 세지 않아야 한다"
    );
}

#[test]
fn mutation_catches_geometry_reassignment_on_modal_shadow() {
    let text = "let mut shadow = theme.shadow_modal().to_egui();\nshadow.blur = 4;\n";
    assert_eq!(
        geom_reassign_violations("x.rs", text).len(),
        1,
        "shadow_modal() 결과의 기하 재대입도 위반이다"
    );
}

#[test]
fn mutation_geometry_comparison_is_not_a_reassignment() {
    // 동등 비교를 재대입으로 오인하지 않는지 확인한다.
    let text = "let mut shadow = theme.shadow_popover().to_egui();\n\
                if shadow.offset == [0, 8] { paint(shadow); }\n";
    assert!(
        geom_reassign_violations("x.rs", text).is_empty(),
        "== 비교는 재대입으로 세지 않아야 한다"
    );
}

#[test]
fn mutation_self_file_is_excluded_but_would_otherwise_flag() {
    // 합성 위반을 포함한 이 파일은 그대로 검사하면 오탐하므로 제외한다.
    let self_src = fs::read_to_string(
        repo_root().join("crates/tasty-type-appearance/src/shadow_policy_guard.rs"),
    )
    .expect("self 파일 읽기");
    let (v, _) = shadow_literal_violations("self.rs", &self_src, None);
    assert!(
        !v.is_empty(),
        "이 검사 파일의 합성 코드는 제외하지 않으면 위반으로 잡혀야 한다"
    );
    assert!(
        !scan_files()
            .iter()
            .any(|f| f.ends_with("shadow_policy_guard.rs")),
        "scan_files 는 자기 파일을 제외해야 한다"
    );
}

#[test]
fn intended_miss_scrim_overlay_is_not_a_shadow_literal() {
    // scrim은 Shadow 구조체가 아니므로 이 검사의 대상이 아니다.
    assert!(!is_shadow_literal(
        "        p.rect_filled(rect, r, theme.scrim().to_egui());"
    ));
    assert!(!is_shadow_literal(
        "        let s = egui::Color32::from_black_alpha(128);"
    ));
}
