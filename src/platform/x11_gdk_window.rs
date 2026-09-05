//! X11 XID 를 `GdkWindow` 로 감싸는 자리 — **NULL 을 값으로 되돌린다.**
//!
//! # 왜 바인딩을 안 쓰는가
//!
//! `gdkx11::X11Window::foreign_new_for_display` 는 C 함수가 NULL 을 돌려주면
//! 내부의 `gdk::Window::from_glib_full` 안에 있는 `assert!(!ptr.is_null())` 로
//! **프로세스를 죽인다.** 그런데 NULL 은 오류가 아니라 **정상 반환값**이다 —
//! `gdk_x11_window_foreign_new_for_display` 는 `XGetWindowAttributes` 로 창을
//! 조회하고, 서버에 그 창이 없으면 NULL 을 준다. 즉 "창이 없다" 를 알리는 유일한
//! 경로가 호출자에게 닿기 전에 즉사로 바뀐다.
//!
//! 그 즉사가 실제로 났다: html surface 를 만드는 경로에서 `XCreateSimpleWindow` 는
//! winit 의 Xlib 연결로, 조회는 GDK 자기 연결로 일어난다. 두 연결 사이에는 순서
//! 보장이 없어, 생성 요청이 서버에서 처리되기 전에 조회가 닿으면 NULL 이 나온다.
//! 원인 쪽은 호출부에서 `XSync` 왕복으로 막고(순서 불변식), 그래도 NULL 이 오면
//! 이 함수가 `Err` 로 되돌린다.
//!
//! # 되돌리지 마라
//!
//! 이 모듈이 있는 한 `foreign_new_for_display` 를 다시 부르면 안 된다.
//! [`tests::panicking_binding_has_no_call_site`] 가 그것을 검사한다.
//!
//! 결정 근거·대안·재검토 조건, 그리고 **이 가드들이 무엇을 안 지키는지**는
//! `docs/adr/0157-a-null-gdk-window-is-a-value-not-a-crash.md`.

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

/// 원시 포인터 판정만 떼어낸 순수 부분. X 서버 없이 NULL 분기를 검사할 수 있게
/// 나눠 둔 것이다 — 결함을 남겨 두지 않고도 회귀를 붙박기 위해서다.
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

    /// 한 줄에서 **코드가 아닌 부분**(줄 주석·문자열 리터럴)을 지운다.
    ///
    /// 문자열 리터럴을 지우는 이유는 검출을 깎기 위해서가 아니라 — 리터럴 안의 이름은
    /// 호출이 아니다 — 이 파일이 **자기 자신을 면제할 필요를 없애기** 위해서다. 아래
    /// 스캔 테스트가 찾는 패턴을 그 테스트가 리터럴로 들고 있으므로, 파일 통째 면제를
    /// 쓰면 이 파일의 다른 위반까지 같이 사라진다.
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

    /// NULL 이면 패닉이 아니라 `Err` 다. 합성 입력이라 레포에 결함이 남아 있지
    /// 않아도 성립한다.
    #[test]
    fn null_pointer_becomes_an_error_not_a_panic() {
        let out = wrap_foreign_window(std::ptr::null_mut(), 0x2a);
        let msg = out.expect_err("null 포인터는 Err 여야 한다");
        // 어느 창인지가 메시지에 남아야 로그만 보고 판정할 수 있다.
        assert!(msg.contains("0x2a"), "메시지에 XID 가 없다: {msg}");
    }

    /// 검출기가 코드와 비-코드를 실제로 가르는지 — 탐지기를 탐지기 자신이 아니라
    /// **합성 입력**으로 잰다. 이게 없으면 위 스캔의 0 건이 "위반이 없다" 인지
    /// "아무것도 못 본다" 인지 안 갈린다.
    #[test]
    fn the_scan_separates_code_from_comments_and_literals() {
        let code = "let w = X11Window::foreign_new_for_display(&d, x);";
        let commented = "// X11Window::foreign_new_for_display(&d, x);";
        let literal = r#"if line.contains("::foreign_new_for_display") {"#;
        let ffi = "gdkx11::ffi::gdk_x11_window_foreign_new_for_display(p, x)";
        // raw 문자열도 코드가 아니다. 이 줄이 첫 회차에 **거짓 양성으로 실제로
        // 걸렸다** — 검출기가 `r#"` 를 몰라 리터럴 안을 코드로 셌다.
        // 입력은 **소스 한 줄**이지 그 줄이 담은 값이 아니다. 그리고 안쪽 따옴표가
        // **홀수** 개여야 이 칸이 판별력을 갖는다: raw 처리를 지우면 따옴표 짝이
        // 어긋나 뒤가 코드로 드러난다. 짝수면 우연히 통과해서 변이가 안 죽는다
        // (앞선 두 안이 각각 그 두 함정에 걸렸다).
        let raw = r##"let s = r#"q " ::foreign_new_for_display"#;"##;
        assert!(!mask_non_code(raw).contains("::foreign_new_for_display"));
        assert!(mask_non_code(code).contains("::foreign_new_for_display"));
        assert!(!mask_non_code(commented).contains("::foreign_new_for_display"));
        assert!(!mask_non_code(literal).contains("::foreign_new_for_display"));
        // ffi 이름은 `::` 가 앞에 없으므로 대상이 아니다 — 이것이 면제가 아니라
        // 패턴의 성질이라는 것을 붙박는다.
        assert!(!mask_non_code(ffi).contains("::foreign_new_for_display"));
    }

    /// ADR-0157 의 **전제**를 검사한다. 결정이 아니라 전제라, 이게 깨지면
    /// "고쳐라" 가 아니라 **"ADR 을 다시 열어라"** 다.
    ///
    /// 전제 둘:
    /// ① 창을 만드는 연결과 그것을 조회하는 연결이 **다르다** — 만들 때는 winit 의
    ///    `display_handle()` 에서 얻은 `Display*` 를, 조회할 때는 GDK 자기
    ///    디스플레이(`gtk::gdk::Display::default()`)를 쓴다.
    /// ② 그래서 그 사이에 **왕복**(`XSync`)이 있어야 한다. `XFlush` 는 보내기만 한다.
    ///
    /// 연결이 하나가 되면 ②의 근거가 사라진다. 그때 `XSync` 를 그냥 지우는 것이
    /// 아니라 결정을 다시 여는 것이 맞다 — 왜 하나가 됐는지가 새 전제이기 때문이다.
    #[test]
    fn adr_0157_two_connection_premise_still_holds() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/host_api/webview/linux.rs");
        let text = std::fs::read_to_string(&path).expect("webview/linux.rs 를 읽지 못했다");
        let code: Vec<String> = text.lines().map(mask_non_code).collect();
        // 0 이 통과가 되지 않게 모수를 먼저 세운다.
        assert!(
            code.len() > 200,
            "스캔한 줄이 {} 뿐이다 — 경로가 틀렸다",
            code.len()
        );
        let at = |needle: &str| code.iter().position(|l| l.contains(needle));

        let reopen = "— ADR-0157 의 전제가 바뀌었다. 고치지 말고                       docs/adr/0157-a-null-gdk-window-is-a-value-not-a-crash.md 를 다시 열어라";

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
        let create = at("XCreateSimpleWindow").expect(&format!("창 생성이 없다 {reopen}"));
        let sync = at("XSync").expect(&format!("생성과 조회 사이의 왕복이 없다 {reopen}"));
        let wrap = at("foreign_gdk_window(").expect(&format!("GDK 조회가 없다 {reopen}"));
        assert!(
            create < sync && sync < wrap,
            "왕복이 생성과 조회 사이에 있지 않다 (create={create}, sync={sync}, wrap={wrap}) {reopen}"
        );
    }

    /// 패닉하는 바인딩(`foreign_new_for_display`)은 호출부가 없어야 한다.
    /// 이것이 진짜 불변식이다 — 위 테스트는 이 모듈만 지키지만, 이 검사는
    /// **레포 전체**가 그 바인딩을 다시 쓰지 못하게 한다.
    #[test]
    fn panicking_binding_has_no_call_site() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
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
                        // `::` 를 붙여 찾는다 — 이 파일이 부르는 ffi 함수
                        // `gdk_x11_window_foreign_new_for_display` 는 같은 꼬리를
                        // 갖지만 `::` 가 앞에 없다. 그래서 **파일 통째 면제가 필요
                        // 없다**(면제가 생기면 그 파일의 다른 위반도 같이 사라진다).
                        if mask_non_code(line).contains("::foreign_new_for_display") {
                            offenders.push(format!("{}:{}", path.display(), i + 1));
                        }
                    }
                }
            }
        }
        // 0 은 통과가 아니라 측정 실패일 수 있다 — 모수를 먼저 세운다.
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
