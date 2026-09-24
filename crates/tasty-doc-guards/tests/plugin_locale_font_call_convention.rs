//! 네 egui 플러그인의 소스에 폰트 환경변수 이름과 공용 검증 함수 호출이 있는지 확인한다.
//! 동일한 폰트를 사용하도록 하는 규약이며 실제 화면의 글리프 렌더링까지 검사하지는 않는다.
//! 호스트의 환경변수 정의는 src/boot/locale.rs의 font_env_path에 있다.

use tasty_doc_guards::repo_root;

/// 플러그인과 폰트 설치 파일. 파일을 옮기면 이 목록도 갱신한다.
const UI_PLUGINS: &[(&str, &str)] = &[
    ("tasty-plugin-clipboard-viewer", "src/main.rs"),
    ("tasty-plugin-git-viewer", "src/main.rs"),
    ("tasty-plugin-image", "src/main.rs"),
    ("tasty-plugin-markdown", "src/popup.rs"),
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

    for (plugin, file) in UI_PLUGINS {
        let path = root.join("crates").join(plugin).join(file);
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

    assert_eq!(
        checked,
        UI_PLUGINS.len(),
        "scanned {checked} plugin(s), expected {} — the scan is broken, not clean",
        UI_PLUGINS.len()
    );
    assert!(
        missing_env.is_empty(),
        "UI plugins missing the locale font variable {LOCALE_FONT_ENV}: {missing_env:?}"
    );
    assert!(
        missing_helper.is_empty(),
        "UI plugins missing the shared font validation helper {HELPER}: {missing_helper:?}"
    );
}
