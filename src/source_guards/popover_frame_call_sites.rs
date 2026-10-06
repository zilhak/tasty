//! 버튼에 붙는 egui 팝오버는 `tasty_egui_theme::with_popover_frame` 클로저 안에서 열어야 메뉴 틀
//! (menu-bg · menu-border · menu-radius)로 그려진다. 전역 스타일은 tooltip 토큰을 담으므로 감싸지 않은
//! 팝오버는 tooltip 틀로 그려진다(docs/design/systems/theme.md).
//!
//! 주석을 가린 본체·크레이트 소스에서 세 진입 호출을 찾고, 바로 앞 코드가 래퍼 호출의 `|ui| {` 클로저
//! 시작인지 텍스트로 확인한다. 래퍼를 다른 함수로 한 번 더 감싼 호출, `Frame::popup`을 직접 그리는 경로,
//! 진입 호출을 `use`로 줄여 부른 형태는 보지 않는다.

use super::rust_sources;

const ENTRIES: &[&str] = &[
    "egui::popup_below_widget(",
    "egui::popup::popup_above_or_below_widget(",
    "egui::ComboBox::from_id_salt(",
];
const WRAPPER: &str = "with_popover_frame(";
const GUARD_DIR: &str = "src/source_guards/";
/// 현재 감싼 호출부는 25곳이다. 순회가 비거나 진입 문자열이 낡아 0건으로 통과하는 것을 막는 하한이다.
const MIN_SITES: usize = 20;

/// `at`의 진입 호출이 래퍼 호출의 클로저 본문 첫 식인지 본다.
fn opened_inside_the_wrapper(code: &str, at: usize) -> bool {
    let Some(head) = code[..at].trim_end().strip_suffix('{') else {
        return false;
    };
    let Some(head) = head.trim_end().strip_suffix("|ui|") else {
        return false;
    };
    head.rfind(WRAPPER)
        .is_some_and(|w| !head[w..].contains([';', '{', '}']))
}

/// 감싸지 않은 진입 호출의 줄 번호와 검사한 진입 호출 수.
fn judge(src: &str) -> (Vec<usize>, usize) {
    let code = tasty_doc_guards::source_text::mask_comments_aligned(src);
    let mut bare = Vec::new();
    let mut seen = 0;
    for entry in ENTRIES {
        for (at, _) in code.match_indices(entry) {
            seen += 1;
            if !opened_inside_the_wrapper(&code, at) {
                bare.push(code[..at].matches('\n').count() + 1);
            }
        }
    }
    (bare, seen)
}

#[test]
fn every_button_popover_opens_inside_the_menu_frame_wrapper() {
    let mut bare = Vec::new();
    let mut seen = 0;
    for (rel, src) in rust_sources() {
        let rel = rel.to_string_lossy().replace('\\', "/");
        if rel.starts_with(GUARD_DIR) {
            continue;
        }
        let (lines, n) = judge(&src);
        seen += n;
        bare.extend(lines.into_iter().map(|l| format!("{rel}:{l}")));
    }
    assert!(
        seen >= MIN_SITES,
        "팝오버 진입 호출을 {seen}곳만 찾았다(하한 {MIN_SITES}). 순회 범위와 진입 문자열을 확인한다."
    );
    assert!(
        bare.is_empty(),
        "`with_popover_frame` 밖에서 연 팝오버가 있다. 감싸지 않으면 메뉴 틀 대신 tooltip 틀로 그려진다:\n{}",
        bare.join("\n")
    );
}

#[test]
fn the_judge_tells_a_wrapped_popover_from_a_bare_one() {
    let wrapped = "tasty_egui_theme::with_popover_frame(ui, &th, |ui| {\n    egui::ComboBox::from_id_salt(\"a\").show_ui(ui, |ui| {});\n});";
    assert_eq!(judge(wrapped), (vec![], 1));
    let bare = "let x = 1;\negui::ComboBox::from_id_salt(\"a\").show_ui(ui, |ui| {});";
    assert_eq!(judge(bare), (vec![2], 1));
    // 래퍼 클로저 안이라도 앞에 다른 문장이 있으면 진입 호출이 래퍼의 첫 식이 아니다.
    let later = "with_popover_frame(ui, th, |ui| {\n    let y = 2;\n    egui::popup_below_widget(ui, id, &r, c, |ui| {});\n});";
    assert_eq!(judge(later), (vec![3], 1));
    // 다른 함수의 `|ui| {` 클로저는 래퍼로 치지 않는다.
    let other = "with_popover_frame(ui, th, |ui| {});\nui.horizontal(|ui| {\n    egui::popup::popup_above_or_below_widget(ui, id, &r, a, b, |ui| {});\n});";
    assert_eq!(judge(other), (vec![3], 1));
    // 주석 안의 진입 문자열은 세지 않는다.
    assert_eq!(judge("// egui::popup_below_widget(\n"), (vec![], 0));
}
