#[path = "sentinel.rs"]
mod sentinel;
macro_rules! record {
    ($($arg:tt)*) => { super::record(format_args!($($arg)*)) };
}
use sentinel::Peers;
use std::{
    path::PathBuf,
    sync::mpsc,
    time::{Duration, Instant},
};
use webview2_com::{Microsoft::Web::WebView2::Win32::*, *};
use windows::{
    Win32::{Foundation::*, System::Com::*, UI::WindowsAndMessaging::*},
    core::*,
};
// SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
unsafe extern "system" fn window_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    // SAFETY: Windows invokes this procedure with the current window's message arguments.
    unsafe { DefWindowProcW(hwnd, msg, w, l) }
}
fn pump<T>(rx: mpsc::Receiver<T>, stage: &str) -> std::result::Result<T, String> {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(value) = rx.try_recv() {
            return Ok(value);
        }
        if Instant::now() >= deadline {
            return Err(format!("{stage}: callback deadline"));
        }
        let mut msg = MSG::default();
        // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            // SAFETY: msg was initialized by PeekMessage on this same apartment thread.
            let _translated = unsafe { TranslateMessage(&msg) };
            // SAFETY: dispatch only the message just removed from this thread's queue.
            unsafe { DispatchMessageW(&msg) };
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
pub(super) fn run() -> std::result::Result<(), String> {
    let folder = PathBuf::from(std::env::var_os("U6_PROFILE").ok_or("missing isolated profile")?);
    if !folder.is_absolute() || folder.exists() {
        return Err("native fixture requires a new absolute private profile".into());
    }
    let socks = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let target = std::net::TcpListener::bind("127.0.0.1:0").map_err(|e| e.to_string())?;
    let proxy = format!(
        "socks5://127.0.0.1:{}",
        socks.local_addr().map_err(|e| e.to_string())?.port()
    );
    let url = format!(
        "http://127.0.0.1:{}/native-control",
        target.local_addr().map_err(|e| e.to_string())?.port()
    );
    let peers = Peers::start(socks, target, super::record)?;
    let wide = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let dir = wide(folder.to_str().ok_or("nonunicode profile")?);
    let flags = format!(
        r#"--proxy-server={proxy} --proxy-bypass-list=<-loopback> --host-resolver-rules="MAP * ~NOTFOUND" --disable-background-networking --disable-quic"#
    );
    record!("STAGE COM");
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok() }
        .map_err(|e| format!("COM: {e}"))?;
    let _apartment = Apartment;
    let options = CoreWebView2EnvironmentOptions::default();
    let flags = wide(&flags);
    let options: ICoreWebView2EnvironmentOptions = options.into();
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    unsafe { options.SetAdditionalBrowserArguments(PCWSTR(flags.as_ptr())) }
        .map_err(|e| format!("options: {e}"))?;
    let (tx, rx) = mpsc::channel();
    record!("STAGE ENVIRONMENT");
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    unsafe {
        CreateCoreWebView2EnvironmentWithOptions(
            PCWSTR::null(),
            PCWSTR(dir.as_ptr()),
            &options,
            &CreateCoreWebView2EnvironmentCompletedHandler::create(Box::new(move |hr, env| {
                tx.send((hr, env))
                    .map_err(|_| windows::core::Error::from_hresult(E_FAIL))?;
                Ok(())
            })),
        )
    }
    .map_err(|e| format!("environment call: {e}"))?;
    let (hr, env) = pump(rx, "environment")?;
    hr.map_err(|e| format!("environment callback: {e}"))?;
    let env = env.ok_or("environment missing")?;
    let mut version = PWSTR::null();
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    unsafe { env.BrowserVersionString(&mut version) }.map_err(|e| format!("version: {e}"))?;
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    let version_text = unsafe { version.to_string() }.unwrap_or_default();
    record!("RUNTIME {version_text}");
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    unsafe { CoTaskMemFree(Some(version.0.cast())) };
    record!("STAGE HIDDEN_CONTROLLER");
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    let instance = unsafe { windows::Win32::System::LibraryLoader::GetModuleHandleW(None) }
        .map_err(|e| e.to_string())?;
    let class = WNDCLASSW {
        lpfnWndProc: Some(window_proc),
        lpszClassName: w!("TastyHttpIsolatedProbe"),
        hInstance: instance.into(),
        ..Default::default()
    };
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    let atom = unsafe { RegisterClassW(&class) };
    if atom == 0 {
        return Err(format!(
            "register class: {}",
            windows::core::Error::from_win32()
        ));
    }
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    let hwnd = unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("TastyHttpIsolatedProbe"),
            w!("Tasty isolated HTTP diagnostic"),
            WS_OVERLAPPED,
            0,
            0,
            64,
            64,
            None,
            None,
            None,
            None,
        )
    }
    .map_err(|e| format!("hidden HWND: {e}"))?;
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    record!("HWND_VALID {}", unsafe { IsWindow(Some(hwnd)) }.as_bool());
    let env10: ICoreWebView2Environment10 =
        env.cast().map_err(|e| format!("environment10: {e}"))?;
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    let controller_options = unsafe { env10.CreateCoreWebView2ControllerOptions() }
        .map_err(|e| format!("controller options: {e}"))?;
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    unsafe { controller_options.SetIsInPrivateModeEnabled(true) }
        .map_err(|e| format!("inprivate: {e}"))?;
    let (tx, rx) = mpsc::channel();
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    unsafe {
        env10.CreateCoreWebView2ControllerWithOptions(
            hwnd,
            &controller_options,
            &CreateCoreWebView2ControllerCompletedHandler::create(Box::new(
                move |hr, controller| {
                    tx.send((hr, controller))
                        .map_err(|_| windows::core::Error::from_hresult(E_FAIL))?;
                    Ok(())
                },
            )),
        )
    }
    .map_err(|e| format!("controller call: {e}"))?;
    let result = (|| {
        let (hr, controller) = pump(rx, "controller")?;
        hr.map_err(|e| format!("controller callback: {e}"))?;
        let controller = controller.ok_or("controller missing")?;
        // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
        unsafe { controller.SetIsVisible(false) }.map_err(|e| format!("hidden: {e}"))?;
        // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
        let web = unsafe { controller.CoreWebView2() }.map_err(|e| format!("webview: {e}"))?;
        // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
        let settings = unsafe { web.Settings() }.map_err(|e| format!("settings: {e}"))?;
        // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
        unsafe { settings.SetIsScriptEnabled(false) }.map_err(|e| format!("script: {e}"))?;
        let (tx, rx) = mpsc::channel();
        let mut token = 0;
        let handler = NavigationCompletedEventHandler::create(Box::new(move |_, args| {
            if let Some(args) = args {
                let mut ok = BOOL::default();
                // SAFETY: WebView2 supplied this live apartment-local event and typed output.
                unsafe { args.IsSuccess(&mut ok) }?;
                let mut status = COREWEBVIEW2_WEB_ERROR_STATUS::default();
                // SAFETY: live native event writes its status to a correctly typed slot.
                unsafe { args.WebErrorStatus(&mut status) }?;
                record!("NAVIGATION success={} status={}", ok.as_bool(), status.0);
                tx.send(())
                    .map_err(|_| windows::core::Error::from_hresult(E_FAIL))?;
            }
            Ok(())
        }));
        // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
        unsafe { web.add_NavigationCompleted(&handler, &mut token) }
            .map_err(|e| format!("navigation handler: {e}"))?;
        // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
        let url = wide(&url);
        // SAFETY: URL storage is live and NUL-terminated; web belongs to this apartment.
        unsafe { web.Navigate(PCWSTR(url.as_ptr())) }.map_err(|e| format!("navigate: {e}"))?;
        let result = pump(rx, "navigation");
        // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
        unsafe { controller.Close() }.map_err(|e| format!("close: {e}"))?;
        result
    })();
    // SAFETY: this isolated fixture owns the arguments and calls native APIs on its STA thread.
    unsafe { DestroyWindow(hwnd) }.map_err(|e| format!("destroy HWND: {e}"))?;
    result?;
    peers.finish()
}

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: run successfully initialized this STA once; native owners have dropped first.
        unsafe { CoUninitialize() };
    }
}
