//! 네 egui UI plugin 이 언어팩 폰트를 **같은 규약**으로 붙이는지 원문 대조 가드.
//!
//! 검증기(`install_locale_font_fallback`)는 하나지만, 그것을 부르는 **호출 규약**(어느
//! env 이름을 읽는가 · 그 헬퍼를 부르는가)은 네 plugin 의 `install_fonts` 에 붙여넣기로
//! 복제된다(각 plugin 구조가 달라 threading 대신 인라인으로 뒀다). 하나가 env 이름을
//! 오타내거나 헬퍼 호출을 빠뜨리면 **그 창만 조용히 tofu** 가 된다 — 다른 셋은 멀쩡해
//! 화면으로도 안 드러난다. 소스만으로 그 동일성을 지킬 것이 없어, 이 시험이 고정한다.
//! (같은 값을 host 쪽에서 지키는 것은 `src/boot/locale.rs` 의 `font_env_path` 단일 출처다.)
//!
//! **0=통과 함정 회피**(ADR-0133): 위반 목록이 비었다는 단언 앞에 "검사한 plugin 수 == 4"
//! 를 둔다 — 스캔이 죽어(경로 오타·크레이트 개명) 목록이 비면 위반 0 이 되고 0 은 언제나
//! 초록이기 때문이다. 하한이 무너지면 "깨끗하다" 가 아니라 "못 셌다" 로 실패한다.

use tasty_doc_guards::repo_root;

/// egui UI 를 그려 폰트 스택을 만드는 번들 plugin 넷. 이 목록이 곧 모수다.
const UI_PLUGINS: &[&str] = &[
    "tasty-plugin-clipboard-viewer",
    "tasty-plugin-git-viewer",
    "tasty-plugin-image",
    "tasty-plugin-markdown",
];

/// host 가 resolve 한 폰트 경로를 자식에 물려주는 env 이름. 넷이 이 철자를 읽어야 한다.
const LOCALE_FONT_ENV: &str = "TASTY_LOCALE_FONT";
/// 붙이기 전 ab_glyph 검증 + append 를 하는 공유 헬퍼. 넷이 이것을 불러야 한다(사본 금지).
const HELPER: &str = "install_locale_font_fallback";

#[test]
fn every_ui_plugin_reads_the_same_locale_font_env_and_calls_the_shared_helper() {
    let root = repo_root();
    let mut checked = 0usize;
    let mut missing_env = Vec::new();
    let mut missing_helper = Vec::new();

    for plugin in UI_PLUGINS {
        let path = root.join("crates").join(plugin).join("src/main.rs");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        checked += 1;
        if !src.contains(LOCALE_FONT_ENV) {
            missing_env.push(*plugin);
        }
        if !src.contains(HELPER) {
            missing_helper.push(*plugin);
        }
    }

    // 스캔이 죽으면 여기서 걸린다 — 아래 두 단언이 빈 목록으로 조용히 통과하기 전에.
    assert_eq!(
        checked,
        UI_PLUGINS.len(),
        "scanned {checked} plugin(s), expected {} — the scan is broken, not clean",
        UI_PLUGINS.len()
    );
    assert!(
        missing_env.is_empty(),
        "these UI plugins do not read {LOCALE_FONT_ENV}; their locale font would silently \
         not apply while the others render: {missing_env:?}"
    );
    assert!(
        missing_helper.is_empty(),
        "these UI plugins do not call {HELPER}; open-coding the validation would let a plugin \
         reject a font the host accepts (tofu in that window only): {missing_helper:?}"
    );
}
