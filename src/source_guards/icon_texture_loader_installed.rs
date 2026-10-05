//! egui Context를 만드는 본체 창과 갤러리 자리에서 아이콘 텍스처 로더를 설치해야 한다.
//! 로더가 빠지면 같은 아이콘을 여러 크기로 그릴 때 첫 크기의 텍스처를 재사용해 흐려지고,
//! 아이콘이 가장자리 알파를 두 번 곱하는 egui_extras 0.31 SVG 래스터를 다시 탄다.
//!
//! 지정한 파일에서 `egui::Context::default()`를 `let`으로 받는 자리를 찾는다. 같은 fn 본문에
//! 그 변수를 첫 인자로 넘기는 `tasty_icons::install_texture_loader` 호출이 있는지 텍스트로 확인한다.
//! 호출이 조건 분기나 이른 반환 뒤에 있어 실행되지 않는 경우는 판단하지 않는다.
//! SITES 두 파일 밖에서 만드는 egui Context는 보지 않는다.

use tasty_doc_guards::source_text::mask_non_code;

use super::{enclosing_fn, first_arg, fn_spans, line_of, word_positions};

/// (파일, 설명). 앱이 아이콘을 그리는 egui Context를 만드는 곳이다.
const SITES: &[(&str, &str)] = &[
    ("src/gfx/gpu.rs", "본체 창 GpuState"),
    ("crates/tasty-gallery/src/main.rs", "갤러리"),
];

const CTX_NEW: &str = "egui::Context::default()";
const INSTALL: &str = "tasty_icons::install_texture_loader";

/// Context를 만들었지만 같은 fn에서 로더를 설치하지 않은 자리의 줄 번호.
/// Context 생성 자리가 없거나 바인딩 이름을 읽지 못하면 Err다.
fn missing_installs(src: &str) -> Result<Vec<usize>, String> {
    let masked = mask_non_code(src);
    let spans = fn_spans(&masked);
    let creations: Vec<usize> = masked.match_indices(CTX_NEW).map(|(at, _)| at).collect();
    if creations.is_empty() {
        return Err(format!("`{CTX_NEW}` 자리를 찾지 못했다"));
    }
    let mut missing = Vec::new();
    for at in creations {
        let line = line_of(&masked, at);
        let stmt_start = masked[..at].rfind(['\n', ';', '{']).map_or(0, |p| p + 1);
        let binding = masked[stmt_start..at]
            .trim()
            .strip_prefix("let ")
            .and_then(|rest| rest.strip_suffix('='))
            .map(|name| name.trim().trim_start_matches("mut ").trim().to_owned())
            .filter(|name| !name.is_empty() && !name.contains(':'))
            .ok_or_else(|| format!("{line}행: `let <이름> = {CTX_NEW}` 형태가 아니다"))?;
        let Some((_, open, close, _)) = enclosing_fn(&spans, at) else {
            return Err(format!("{line}행: 감싸는 fn을 찾지 못했다"));
        };
        let installed = word_positions(&masked[..*close], INSTALL)
            .into_iter()
            .filter(|&call| call > *open)
            .any(|call| {
                first_arg(&masked, call)
                    .is_some_and(|arg| arg.trim_start_matches('&').trim() == binding)
            });
        if !installed {
            missing.push(line);
        }
    }
    Ok(missing)
}

#[test]
fn every_icon_drawing_context_installs_the_icon_texture_loader() {
    let root = super::repo_root();
    for (rel, what) in SITES {
        let src = std::fs::read_to_string(root.join(rel))
            .unwrap_or_else(|e| panic!("{rel}({what})를 읽지 못했다: {e}"));
        let missing = missing_installs(&src).unwrap_or_else(|e| panic!("{rel}({what}): {e}"));
        assert!(
            missing.is_empty(),
            "{rel}({what}) {missing:?}행: egui Context를 만든 fn에서 {INSTALL}(&ctx)를 호출하지 않는다. \
             docs/design/systems/icons.md의 크기별 텍스처 절을 확인한다."
        );
    }
}

#[test]
fn a_context_without_the_install_is_reported() {
    let src = "fn a() {\n    let ctx = egui::Context::default();\n    egui_extras::install_image_loaders(&ctx);\n}\n";
    assert_eq!(missing_installs(src), Ok(vec![2]));
}

#[test]
fn an_install_on_another_context_or_in_another_fn_does_not_count() {
    let other_binding = "fn a() {\n    let ctx = egui::Context::default();\n    tasty_icons::install_texture_loader(&other);\n}\n";
    assert_eq!(missing_installs(other_binding), Ok(vec![2]));
    let other_fn = "fn a() {\n    let ctx = egui::Context::default();\n}\nfn b(ctx: &egui::Context) {\n    tasty_icons::install_texture_loader(&ctx);\n}\n";
    assert_eq!(missing_installs(other_fn), Ok(vec![2]));
}

#[test]
fn a_commented_out_install_does_not_count() {
    let src = "fn a() {\n    let ctx = egui::Context::default();\n    // tasty_icons::install_texture_loader(&ctx);\n}\n";
    assert_eq!(missing_installs(src), Ok(vec![2]));
}

#[test]
fn a_matching_install_passes_and_an_empty_file_is_an_error() {
    let src = "fn a() {\n    let egui_ctx = egui::Context::default();\n    tasty_icons::install_texture_loader(&egui_ctx);\n}\n";
    assert_eq!(missing_installs(src), Ok(vec![]));
    assert!(missing_installs("fn a() {}\n").is_err());
}
