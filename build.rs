fn main() {
    // winresource가 Cargo.toml의 이름·버전·메타데이터도 읽으므로 변경을 추적한다.
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");

    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/icon.ico");
        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=Failed to embed Windows icon: {e}");
        }
    }

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "linux" && std::env::var_os("CARGO_FEATURE_GUI").is_some() {
        webkit_selection_extension();
    }
}

/// Linux WebView의 선택 확인 web process 확장을 빌드한다. 본체가 바이트로 내장해 실행 시 꺼내 쓰므로
/// 별도 설치 단계가 없다. 계약: docs/design/systems/webview.md의 편집 액션 절.
fn webkit_selection_extension() {
    const SOURCE: &str = "src/host_api/webview/linux/selection_extension.c";
    println!("cargo:rerun-if-changed={SOURCE}");
    let library = pkg_config::Config::new()
        .cargo_metadata(false)
        .probe("webkit2gtk-web-extension-4.1")
        .unwrap_or_else(|e| {
            panic!("webkit2gtk-web-extension-4.1 not found (libwebkit2gtk-4.1-dev): {e}")
        });
    let out = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"))
        .join("libtasty-webkit-selection.so");
    let compiler = cc::Build::new().get_compiler();
    let mut command = compiler.to_command();
    command.args(["-shared", "-fPIC", "-O2", "-Wall", "-Wextra", "-Werror"]);
    for include in &library.include_paths {
        command.arg("-I").arg(include);
    }
    command.arg(SOURCE).arg("-o").arg(&out);
    for path in &library.link_paths {
        command.arg("-L").arg(path);
    }
    for lib in &library.libs {
        command.arg(format!("-l{lib}"));
    }
    let status = command
        .status()
        .unwrap_or_else(|e| panic!("failed to run the C compiler for {SOURCE}: {e}"));
    assert!(status.success(), "building {SOURCE} failed: {status}");
}
