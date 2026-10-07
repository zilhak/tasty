//! `render_document` 의 HTML 을 실제 WebKitGTK 에 띄워 주소창 배치를 잰다(Linux 전용).
//!
//! 생성 CSS 문자열 시험은 선언이 있는지만 본다. 여기서는 엔진이 그 CSS 로 주소창을 문서 어느
//! 위치에서도 상단에 붙이는지를 `getBoundingClientRect()` 로 판정한다. GTK3 는 디스플레이가 있어야
//! 하므로 `#[ignore]` 로 두고, 디스플레이를 주는 게이트가 이름으로 부른다
//! (`docs/dev-guide/ci-gates.md`). 실행:
//! `xvfb-run -a cargo test -p tasty-plugin-markdown -- --ignored --exact render::webview_layout_tests::address_bar_stays_on_top_in_webkitgtk`

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use javascriptcore::ValueExt as _;
use tasty_plugin_sdk::Translator;
use tasty_type_appearance::theme::Theme;
use webkit2gtk::{LoadEvent, WebView, WebViewExt as _};

use super::{DocumentInput, render_document};

/// 측정 뷰포트. 회귀를 처음 잰 값과 같게 둔다.
const VIEW_W: i32 = 1000;
const VIEW_H: i32 = 800;
/// 엔진 응답 한 번을 기다리는 상한. 넘기면 엔진이 멈춘 것으로 보고 실패한다.
const WAIT: Duration = Duration::from_secs(20);

fn document(source: &str) -> String {
    let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
    let tr = Translator::default();
    render_document(DocumentInput {
        theme: &theme,
        tr: &tr,
        file_path: "/a/current.md",
        source,
        load_error: None,
        base_dir: Some(Path::new("/a")),
        recent: &[],
        remote: None,
    })
}

/// 화면보다 몇 배 긴 문서. 가운데 제목은 앵커 이동 목적지다.
fn long_source() -> String {
    let mut s = String::new();
    for i in 0..120 {
        s.push_str(&format!(
            "## Section {i}\n\nParagraph {i} with enough words to wrap across the line once or twice.\n\n"
        ));
    }
    s
}

fn spin_until(done: impl Fn() -> bool, what: &str) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < WAIT,
            "{what}: WebKitGTK 가 {WAIT:?} 안에 답하지 않았다"
        );
        gtk::main_iteration_do(false);
        std::thread::sleep(Duration::from_millis(2));
    }
}

struct Page {
    _window: gtk::OffscreenWindow,
    view: WebView,
}

impl Page {
    fn load(html: &str) -> Self {
        let window = gtk::OffscreenWindow::new();
        window.set_default_size(VIEW_W, VIEW_H);
        let view = WebView::new();
        window.add(&view);
        window.show_all();
        let finished = Rc::new(Cell::new(false));
        let flag = finished.clone();
        view.connect_load_changed(move |_, event| {
            if event == LoadEvent::Finished {
                flag.set(true);
            }
        });
        view.load_html(html, None);
        spin_until(|| finished.get(), "문서 적재");
        Page {
            _window: window,
            view,
        }
    }

    /// 스크립트의 값을 JSON 문자열로 돌려받아 숫자로 읽는다.
    fn number(&self, script: &str) -> f64 {
        let out: Rc<RefCell<Option<Result<String, String>>>> = Rc::new(RefCell::new(None));
        let slot = out.clone();
        let wrapped = format!("JSON.stringify((() => {{ {script} }})())");
        self.view.evaluate_javascript(
            &wrapped,
            None,
            None,
            None::<&gtk::gio::Cancellable>,
            move |r| {
                *slot.borrow_mut() =
                    Some(r.map(|v| v.to_str().to_string()).map_err(|e| e.to_string()));
            },
        );
        spin_until(|| out.borrow().is_some(), script);
        let text = out
            .borrow_mut()
            .take()
            .expect("filled above")
            .unwrap_or_else(|e| panic!("{script}: 스크립트 실패: {e}"));
        let value: serde_json::Value =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{script}: {text}: {e}"));
        value
            .as_f64()
            .unwrap_or_else(|| panic!("{script}: 숫자가 아니다: {text}"))
    }
}

const BAR_TOP: &str =
    "return document.getElementById('tasty-addr-bar').getBoundingClientRect().top;";

/// 주소창은 긴 문서의 끝과 앵커 이동 뒤에도 뷰포트 맨 위(`rect.top == 0`)에 있고,
/// 짧은 문서에서는 `body` 상자가 뷰포트 높이를 채운다.
///
/// `body{height:100%}` 로 되돌리면 첫 단언이, `html` 의 확정 높이를 빼면 마지막 단언이 실패한다.
#[test]
#[ignore = "WebKitGTK 를 띄울 디스플레이가 필요하다 — 게이트가 xvfb-run 으로 이름을 지정해 실행한다"]
fn address_bar_stays_on_top_in_webkitgtk() {
    let display = std::env::var("DISPLAY").unwrap_or_default();
    assert!(
        !display.is_empty(),
        "DISPLAY 가 없다. `xvfb-run -a cargo test -p tasty-plugin-markdown -- --ignored --exact \
         render::webview_layout_tests::address_bar_stays_on_top_in_webkitgtk` 로 실행한다"
    );
    // 사용자 세션의 WAYLAND_DISPLAY 가 있어도 지정한 X 디스플레이에 띄운다.
    gtk::gdk::set_allowed_backends("x11");
    gtk::init().expect("GTK 초기화");

    let long = Page::load(&document(&long_source()));
    let viewport = long.number("return window.innerHeight;");
    assert_eq!(viewport, f64::from(VIEW_H), "뷰포트 높이");
    let scroll_height = long.number("return document.scrollingElement.scrollHeight;");
    assert!(
        scroll_height > 4.0 * viewport,
        "문서가 화면보다 충분히 길어야 끝까지 스크롤한 결과가 의미 있다: {scroll_height}"
    );

    let scrolled = long.number(
        "const s = document.scrollingElement; s.scrollTop = s.scrollHeight; return s.scrollTop;",
    );
    assert!(
        scrolled > scroll_height - 2.0 * viewport,
        "문서 끝까지 스크롤되지 않았다: scrollTop {scrolled}, scrollHeight {scroll_height}"
    );
    assert_eq!(
        long.number(BAR_TOP),
        0.0,
        "문서 끝에서 주소창이 상단에 붙어 있지 않다 (scrollTop {scrolled})"
    );

    let anchored = long.number(
        "document.scrollingElement.scrollTop = 0; \
         document.getElementById('section-60').scrollIntoView(); \
         return document.scrollingElement.scrollTop;",
    );
    assert!(
        anchored > viewport,
        "앵커 이동이 일어나지 않았다: scrollTop {anchored}"
    );
    assert_eq!(
        long.number(BAR_TOP),
        0.0,
        "앵커 이동 뒤 주소창이 상단에 붙어 있지 않다 (scrollTop {anchored})"
    );

    let short = Page::load(&document("# Short\n\nOne line."));
    assert_eq!(
        short.number("return document.body.getBoundingClientRect().height;"),
        f64::from(VIEW_H),
        "짧은 문서의 body 가 뷰포트 높이를 채우지 않는다"
    );
}
