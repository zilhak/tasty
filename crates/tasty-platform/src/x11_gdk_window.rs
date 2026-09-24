//! X11 XID를 GdkWindow로 감싸되 NULL은 Err로 반환한다.
//! gdkx11의 foreign_new_for_display 바인딩은 NULL에 패닉하므로 직접 FFI를 호출해 검사한다.
//! winit과 GDK가 서로 다른 연결을 쓰는 생성 경로는 호출자가 XSync로 생성 완료를 확인해야 한다.
//! NULL 처리는 그 순서 보장과 별개로 유지한다.

use gtk::glib::translate::ToGlibPtr;

/// X11 XID 로 `GdkWindow` 를 얻는다. 서버에 그 창이 없으면 `Err`.
pub fn foreign_gdk_window(
    display: &gdkx11::X11Display,
    xid: std::os::raw::c_ulong,
) -> Result<gtk::gdk::Window, String> {
    // SAFETY: `display` 는 살아 있는 X11 GDK 디스플레이 참조이고, 이 함수는 GTK 를
    // 초기화한 winit main thread 에서만 불린다(GDK 는 단일 thread 규약).
    let raw = unsafe {
        gdkx11::ffi::gdk_x11_window_foreign_new_for_display(display.to_glib_none().0, xid)
    };
    wrap_foreign_window(raw, xid)
}

/// X 서버 없이 NULL 분기를 시험할 수 있도록 포인터 변환을 분리한다.
fn wrap_foreign_window(
    raw: *mut gtk::gdk::ffi::GdkWindow,
    xid: std::os::raw::c_ulong,
) -> Result<gtk::gdk::Window, String> {
    if raw.is_null() {
        return Err(format!(
            "X11 window 0x{xid:x} is not on the GDK display (the server has no such window)"
        ));
    }
    // SAFETY: NULL 이 아님을 방금 확인했고, `gdk_x11_window_foreign_new_for_display`
    // 는 새 참조를 돌려주므로(new 계열) full 소유권 규약이 맞다.
    Ok(unsafe { gtk::glib::translate::from_glib_full(raw) })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 컴파일된 크레이트 경로에서 저장소 루트를 찾고 표지 파일도 확인한다.
    /// 아래 소스 검사는 빈 파일 집합이 통과하지 않도록 별도 하한을 검사한다.
    fn repo_root() -> std::path::PathBuf {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .unwrap_or_else(|| panic!("CARGO_MANIFEST_DIR 에서 두 칸 올라갈 수 없다"))
            .to_path_buf();
        for marker in ["Cargo.toml", "docs/adr/index.md", "src/lib.rs"] {
            assert!(
                root.join(marker).exists(),
                "레포 루트로 잡은 {} 에 표지 {marker} 가 없다 — 경로가 틀어졌다",
                root.display()
            );
        }
        root
    }

    /// 한 소스 줄의 주석·문자열을 가린다. 검색 패턴 자체를 가진 시험 파일도 제외하지 않고 검사한다.
    fn mask_non_code(line: &str) -> String {
        let c: Vec<char> = line.chars().collect();
        let mut out = String::with_capacity(line.len());
        let mut i = 0;
        while i < c.len() {
            // 줄 주석 — 여기서 줄이 끝난다.
            if c[i] == '/' && c.get(i + 1) == Some(&'/') {
                break;
            }
            // raw 문자열 `r"..."` / `r#"..."#`. 앞이 식별자 문자면 `r` 은 접두사가
            // 아니라 이름의 끝이다.
            let prev_is_ident = i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '_');
            if c[i] == 'r' && !prev_is_ident {
                let mut hashes = 0usize;
                let mut j = i + 1;
                while c.get(j) == Some(&'#') {
                    hashes += 1;
                    j += 1;
                }
                if c.get(j) == Some(&'"') {
                    j += 1;
                    while j < c.len() {
                        if c[j] == '"' && c[j + 1..].iter().take(hashes).all(|h| *h == '#') {
                            j += 1 + hashes;
                            break;
                        }
                        j += 1;
                    }
                    i = j;
                    continue;
                }
            }
            // 보통 문자열 — `\` 는 다음 한 글자를 먹는다.
            if c[i] == '"' {
                i += 1;
                while i < c.len() {
                    if c[i] == '\\' {
                        i += 2;
                        continue;
                    }
                    if c[i] == '"' {
                        i += 1;
                        break;
                    }
                    i += 1;
                }
                continue;
            }
            out.push(c[i]);
            i += 1;
        }
        out
    }

    /// NULL 합성 입력은 패닉 없이 Err로 반환해야 한다.
    #[test]
    fn null_pointer_becomes_an_error_not_a_panic() {
        let out = wrap_foreign_window(std::ptr::null_mut(), 0x2a);
        let msg = out.expect_err("null 포인터는 Err 여야 한다");
        // 어느 창인지가 메시지에 남아야 로그만 보고 판정할 수 있다.
        assert!(msg.contains("0x2a"), "메시지에 XID 가 없다: {msg}");
    }

    /// 코드·주석·문자열 합성 입력으로 검색 범위를 확인한다.
    #[test]
    fn the_scan_separates_code_from_comments_and_literals() {
        let code = "let w = X11Window::foreign_new_for_display(&d, x);";
        let commented = "// X11Window::foreign_new_for_display(&d, x);";
        let literal = r#"if line.contains("::foreign_new_for_display") {"#;
        let ffi = "gdkx11::ffi::gdk_x11_window_foreign_new_for_display(p, x)";
        // raw 문자열 안 따옴표 수를 홀수로 두어 일반 문자열 처리와 구분한다.
        let raw = r##"let s = r#"q " ::foreign_new_for_display"#;"##;
        assert!(!mask_non_code(raw).contains("::foreign_new_for_display"));
        assert!(mask_non_code(code).contains("::foreign_new_for_display"));
        assert!(!mask_non_code(commented).contains("::foreign_new_for_display"));
        assert!(!mask_non_code(literal).contains("::foreign_new_for_display"));
        // 직접 호출하는 FFI 이름은 검색 패턴과 다르다.
        assert!(!mask_non_code(ffi).contains("::foreign_new_for_display"));
    }

    /// 창 생성과 조회가 서로 다른 연결을 쓰며 그 사이 XSync가 있는지 확인한다.
    /// XFlush는 송신만 하므로 생성 완료 확인을 대신할 수 없다. 연결 구조가 바뀌면 이 전제도 검토한다.
    #[test]
    fn adr_0159_two_connection_premise_still_holds() {
        let path = repo_root().join("src/host_api/webview/linux.rs");
        let text = std::fs::read_to_string(&path).expect("webview/linux.rs 를 읽지 못했다");
        let code: Vec<String> = text.lines().map(mask_non_code).collect();
        // 잘못된 경로의 빈 입력이 통과하지 않게 크기를 확인한다.
        assert!(
            code.len() > 200,
            "스캔한 줄이 {} 뿐이다 — 경로가 틀렸다",
            code.len()
        );
        let at = |needle: &str| code.iter().position(|l| l.contains(needle));

        let reopen = concat!(
            "— docs/design/systems/webview.md#생성--부모-handle-과-실패-분류 의 전제가 바뀌었다. 고치지 말고 ",
            "docs/design/systems/webview.md#생성--부모-handle-과-실패-분류 를 다시 열어라"
        );

        // ① 연결이 둘이다.
        assert!(
            at("display_handle()").is_some(),
            "창을 만드는 연결을 winit 에서 얻지 않는다 {reopen}"
        );
        assert!(
            at("gtk::gdk::Display::default()").is_some(),
            "조회하는 연결이 GDK 자기 디스플레이가 아니다 {reopen}"
        );

        // ② 생성과 조회 사이에 왕복이 있다.
        let create = at("XCreateSimpleWindow").unwrap_or_else(|| panic!("창 생성이 없다 {reopen}"));
        let sync = at("XSync").unwrap_or_else(|| panic!("생성과 조회 사이의 왕복이 없다 {reopen}"));
        let wrap = at("foreign_gdk_window(").unwrap_or_else(|| panic!("GDK 조회가 없다 {reopen}"));
        assert!(
            create < sync && sync < wrap,
            "왕복이 생성과 조회 사이에 있지 않다 (create={create}, sync={sync}, wrap={wrap}) {reopen}"
        );
    }

    /// foreign GdkWindow를 직접 연결하면 size_allocate도 직접 전달해야 한다.
    /// 두 동작 중 하나만 남은 경우를 검사한다. 둘을 함께 제거하는 설계 변경은 별도 검토가 필요하다.
    #[test]
    fn adr_0301_foreign_bind_and_explicit_allocation_move_together() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(std::path::Path::parent)
            .expect("crates/<크레이트> 아래에 있어야 한다")
            .join("src/host_api/webview/linux.rs");
        let text = std::fs::read_to_string(&path).expect("webview/linux.rs 를 읽지 못했다");
        let code: Vec<String> = text.lines().map(mask_non_code).collect();
        // 잘못된 경로의 빈 입력이 통과하지 않게 크기를 확인한다.
        assert!(
            code.len() > 200,
            "스캔한 줄이 {} 뿐이다 — 경로가 틀렸다",
            code.len()
        );
        let has = |needle: &str| code.iter().any(|l| l.contains(needle));

        let bind = has(".set_window(");
        let alloc = has(".size_allocate(");
        assert_eq!(
            bind, alloc,
            concat!(
                "foreign bind(set_window)와 명시적 allocation(size_allocate)이 따로 움직였다 ",
                "(bind={}, alloc={}) — 둘은 한 쌍이다. ",
                "docs/design/systems/webview.md#좌표--논리와-물리를-타입-이름에-남긴다 를 읽어라"
            ),
            bind, alloc
        );
    }

    /// src·crates의 Rust 소스에 NULL을 패닉으로 처리하는 바인딩 호출이 없는지 검사한다.
    #[test]
    fn panicking_binding_has_no_call_site() {
        let root = repo_root();
        let mut scanned = 0usize;
        let mut offenders = Vec::new();
        let mut stack = vec![root.join("src"), root.join("crates")];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    let Ok(text) = std::fs::read_to_string(&path) else {
                        continue;
                    };
                    scanned += 1;
                    for (i, line) in text.lines().enumerate() {
                        // :: 접두사로 직접 FFI 이름과 구분해 파일 전체 면제를 피한다.
                        if mask_non_code(line).contains("::foreign_new_for_display") {
                            offenders.push(format!("{}:{}", path.display(), i + 1));
                        }
                    }
                }
            }
        }
        // 빈 수집이 성공으로 처리되지 않도록 확인한다.
        assert!(
            scanned > 100,
            "스캔한 파일이 {scanned} 개뿐이다 (경로가 틀렸다)"
        );
        assert!(
            offenders.is_empty(),
            "패닉하는 바인딩을 직접 부르는 자리가 있다 — `foreign_gdk_window` 를 써라: {offenders:?}"
        );
    }
}
