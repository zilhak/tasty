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
}
