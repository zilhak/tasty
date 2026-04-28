fn main() {
    // Embed the application icon into the Windows executable (.exe).
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icons/icon.ico");
        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=Failed to embed Windows icon: {e}");
        }
    }
}
