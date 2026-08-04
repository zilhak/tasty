//! `scripts/build-macos-dmg.sh` 가 조립하는 `.app` 레이아웃이 실제로 서명 가능한지
//! 검증한다.
//!
//! 배경: plugin 을 `Contents/MacOS/plugins/<id>/` 에 staging 하던 시절, codesign 은
//! `Contents/MacOS/` 하위에 실행 파일이 든 디렉터리를 nested code 로 간주해 번들로
//! 파싱하려다 "bundle format unrecognized, invalid, or unsuitable" 로 실패했다
//! (그 디렉터리엔 `Contents/Info.plist` 가 없으니 유효한 번들이 아니다). 스크립트는
//! `set -euo pipefail` 이라 그 지점에서 죽었고, 서명되지 않은 반쪽 번들이 `dist/` 에
//! 남아 그대로 실행됐다. 서명이 없으면 macOS 가 TCC 승인을 앱에 귀속시키지 못해
//! 권한 프롬프트가 매 실행마다 다시 뜬다.
//!
//! CI 의 macOS 빌드 잡(`.github/workflows/build-check.yml`)은 `workflow_dispatch`
//! 수동 트리거라 이 실패를 자동으로 잡지 못했다. 이 테스트가 `cargo test --workspace`
//! 채널로 그 공백을 메운다 — 첫 테스트는 플랫폼 무관 정적 가드라 Linux CI 에서도
//! 돌고, 둘째는 macOS 에서 실제 `codesign` 을 돌려 레이아웃을 증명한다.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// 빌드 스크립트의 `PLUGINS_DIR=` 할당에서 `$APP_DIR/` 이후의 번들 내부 상대 경로를
/// 뽑는다 (예: `Contents/Resources/plugins`). 스크립트가 staging 위치를 바꾸면 이
/// 테스트가 따라 움직이므로, 검증 대상과 구현이 드리프트하지 않는다.
fn staged_plugins_rel_path() -> String {
    let script_path = repo_root().join("scripts/build-macos-dmg.sh");
    let script = std::fs::read_to_string(&script_path).expect("build-macos-dmg.sh read 실패");
    for line in script.lines() {
        let Some(rest) = line.trim_start().strip_prefix("PLUGINS_DIR=") else {
            continue;
        };
        let value = rest.trim().trim_matches('"');
        let rel = value.strip_prefix("$APP_DIR/").unwrap_or_else(|| {
            panic!(
                "PLUGINS_DIR 이 $APP_DIR 기준이 아님: {value} ({})",
                script_path.display()
            )
        });
        assert!(!rel.is_empty(), "PLUGINS_DIR 상대 경로가 빔");
        return rel.to_string();
    }
    panic!("PLUGINS_DIR 할당을 찾지 못함: {}", script_path.display());
}

/// plugin staging 위치가 `Contents/MacOS/` 밖인지 확인하는 정적 가드.
/// codesign 이 없는 플랫폼(Linux CI)에서도 도는 값싼 방어선.
#[test]
fn staged_plugins_are_outside_contents_macos() {
    let rel = staged_plugins_rel_path();
    assert!(
        rel.starts_with("Contents/"),
        "plugin staging 경로가 번들 Contents/ 밖이다: {rel}"
    );
    assert!(
        !rel.starts_with("Contents/MacOS/"),
        "plugin 을 Contents/MacOS/ 하위에 staging 하면 codesign 이 그 디렉터리를 \
         nested bundle 로 파싱하려다 실패한다 (bundle format unrecognized). \
         현재 경로: {rel}"
    );
}

#[cfg(target_os = "macos")]
const PROBE_INFO_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Probe</string>
    <key>CFBundleIdentifier</key>
    <string>com.zilhak.tasty.codesign-probe</string>
    <key>CFBundleExecutable</key>
    <string>probe</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleVersion</key>
    <string>1.0</string>
</dict>
</plist>
"#;

/// 빌드 스크립트가 쓰는 staging 위치를 그대로 재현한 최소 `.app` 을 만들어
/// ad-hoc 서명이 통과하는지 확인한다. 누군가 plugin 을 다시 `Contents/MacOS/`
/// 하위로 옮기면 여기서 실패한다.
#[cfg(target_os = "macos")]
#[test]
fn staged_layout_is_codesignable() {
    // 서명 대상으로 쓸 임의의 Mach-O. 시스템 바이너리를 복사해 쓴다.
    const DONOR_BIN: &str = "/bin/echo";

    let tmp = tempfile::tempdir().expect("tempdir 생성 실패");
    let app = tmp.path().join("Probe.app");
    let contents = app.join("Contents");
    std::fs::create_dir_all(contents.join("MacOS")).expect("Contents/MacOS 생성 실패");
    std::fs::copy(DONOR_BIN, contents.join("MacOS/probe")).expect("메인 실행 파일 복사 실패");
    std::fs::write(contents.join("Info.plist"), PROBE_INFO_PLIST).expect("Info.plist 쓰기 실패");

    // 스크립트가 지정한 위치에 plugin 하나를 staging 한 모양 (바이너리 + 매니페스트).
    let plugin_dir = app.join(staged_plugins_rel_path()).join("com.tasty.probe");
    std::fs::create_dir_all(&plugin_dir).expect("plugin staging 디렉터리 생성 실패");
    std::fs::copy(DONOR_BIN, plugin_dir.join("tasty-plugin-probe"))
        .expect("plugin 바이너리 복사 실패");
    std::fs::write(
        plugin_dir.join("tasty-plugin.toml"),
        "id = \"com.tasty.probe\"\n",
    )
    .expect("plugin 매니페스트 쓰기 실패");

    run_codesign(&["--force", "--sign", "-"], &app, "서명");
    run_codesign(&["--verify", "--deep", "--strict"], &app, "검증");

    // 서명이 실제로 봉인됐는지 — 링커 자동 서명은 _CodeSignature 를 남기지 않는다.
    assert!(
        contents.join("_CodeSignature").is_dir(),
        "codesign 이 성공했다는데 _CodeSignature/ 가 없다"
    );
}

#[cfg(target_os = "macos")]
fn run_codesign(args: &[&str], app: &std::path::Path, what: &str) {
    let output = std::process::Command::new("codesign")
        .args(args)
        .arg(app)
        .output()
        .expect("codesign 실행 실패 (Xcode Command Line Tools 필요)");
    assert!(
        output.status.success(),
        "codesign {what} 실패 ({}):\n{}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}
