fn main() {
    // 이 스크립트의 입력 선언. 하나라도 선언하면 cargo 는 "패키지 안의 아무 파일이나 바뀌면
    // 다시 돈다" 는 기본값을 버리고 **선언한 것만** 본다 — 그래서 이 목록이 곧 계약이고,
    // 빠뜨린 입력은 바뀌어도 재실행을 못 일으킨다.
    //
    // 입력은 둘이다.
    //
    // - 아이콘: 아래 `set_icon` 이 읽는다.
    // - `Cargo.toml`: winresource 가 읽는다. `WindowsResource::new()` 가 `parse_cargo_toml` 을
    //   부르고, 그것은 기본 feature `toml` 로 켜져 있다(우리 의존은 `default-features` 를 끄지
    //   않고, `Cargo.lock` 의 winresource 항목에 `toml` 이 들어 있다). 거기서 exe 의
    //   VERSIONINFO 기본값(패키지 이름·버전)과 `[package.metadata.winresource]` 를 읽는다.
    //   그 섹션이 지금 없어도 **파일 자체는 입력이다** — 이름과 버전이 거기서 온다.
    //
    // 비-Windows 에서도 선언한다. 그쪽은 아래 블록이 통째로 빠져 본문이 빈 `main` 이라
    // 선언이 산출물을 바꾸지 않고, 바뀌지 않은 입력에 대해 빈 스크립트를 다시 돌리지 않게만
    // 한다.
    println!("cargo:rerun-if-changed=assets/icons/icon.ico");
    println!("cargo:rerun-if-changed=Cargo.toml");

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
