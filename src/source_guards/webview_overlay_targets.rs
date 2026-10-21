//! native WebView 위 오버레이 대상 목록(`TargetKind`)이 실제로 대상을 기록하는 그리기 위치와
//! 일치하는지 확인한다. 종류마다 기록하는 곳이 정확히 한 파일에 있어야 하고, 목록에 없는 종류를
//! 기록하는 곳이 있으면 안 된다. 프레임 시작·끝 처리가 그리기 전·후에 있는지도 확인한다.
//! 텍스트 위치를 비교하며 조건 분기의 실제 실행 여부는 판단하지 않는다.

use super::{fn_body, repo_root, strip_comments};

/// 종류별로 대상을 기록하는 파일. 새 종류를 더하면 여기와 `TargetKind` 를 함께 고친다.
const SITES: &[(&str, &str)] = &[
    ("HostPopup", "src/adapters/ui/popup/draw.rs"),
    ("PluginPopup", "src/plugin_bridge/popup_render.rs"),
    ("Dropdown", "src/state/webview_overlay.rs"),
];

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n");
    strip_comments(&src)
}

/// 시험 모듈을 뺀 본문. 시험 안의 표본 대상은 그리기 위치가 아니다. 시험 전용 `use` 는 남긴다.
fn non_test(src: &str) -> &str {
    src.split("#[cfg(test)]\nmod ").next().unwrap_or(src)
}

#[test]
fn target_kinds_match_the_draw_sites() {
    let model = read("src/state/webview_overlay.rs");
    let list_start = model
        .find("pub enum TargetKind {")
        .expect("TargetKind 정의가 없다");
    let list_end = list_start + model[list_start..].find('}').expect("TargetKind 끝");
    let list = &model[list_start + "pub enum TargetKind {".len()..list_end];
    let variants: Vec<&str> = list
        .split(',')
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect();
    assert_eq!(
        variants.len(),
        SITES.len(),
        "TargetKind 의 종류 {variants:?} 와 이 검사의 기록 위치 수가 다르다"
    );
    for (kind, home) in SITES {
        assert!(variants.contains(kind), "TargetKind 에 {kind} 가 없다");
        let pushed = format!("kind: crate::state::webview_overlay::TargetKind::{kind}");
        let local = format!("kind: TargetKind::{kind}");
        for (other, file) in SITES {
            let src = read(file);
            let body = non_test(&src);
            let found = body.contains(&pushed) || body.contains(&local);
            if other == kind {
                assert!(found, "{kind} 대상을 {file} 에서 기록하지 않는다");
            } else {
                assert!(!found, "{kind} 대상을 {home} 밖의 {file} 에서도 기록한다");
            }
        }
    }
}

#[test]
fn overlay_frame_brackets_every_draw() {
    let src = read("src/gfx/gpu/egui_bridge.rs");
    let body = fn_body(&src, "fn run_egui_frame").expect("run_egui_frame 본문");
    let begin = body
        .find("webview_overlay::begin_frame(")
        .expect("begin_frame 호출이 없다");
    let end = body
        .find("webview_overlay::end_frame(")
        .expect("end_frame 호출이 없다");
    for draw in [
        "titlebar::draw_titlebar(",
        "ui::draw_popups(",
        "popup_render::draw_plugin_popups(",
    ] {
        let at = body
            .find(draw)
            .unwrap_or_else(|| panic!("{draw} 호출이 없다"));
        assert!(begin < at, "{draw} 가 begin_frame 보다 앞에 있다");
        assert!(at < end, "{draw} 가 end_frame 보다 뒤에 있다");
    }
}
