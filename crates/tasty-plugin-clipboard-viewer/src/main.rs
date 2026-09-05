#![forbid(unsafe_code)]

//! Tasty Clipboard Viewer plugin — 현재 시스템 클립보드(최신 하나)의 read-only 뷰어.
//!
//! Tools 메뉴 클릭(`[[contributes.tool]]`) 또는 단축키 커맨드(`[[contributes.commands]]`
//! `open_viewer`)가 `action = open_popup` 을 통해 host 로 하여금 popup 인스턴스를
//! 직접 열게 한다 — 호스트가 plugin 전용 이벤트를 발행하는 구식 경로는 없다. 클립보드는
//! plugin process 내에서 arboard 로 **직접** 읽으며 호스트 IPC 를 경유하지 않는다
//! (ADR-0009 상 first-party 직접 read).
//!
//! 렌더 경로는 **egui-mesh popup**(ADR-0028 / B4): plugin 이 자기 프로세스에서 popup
//! 콘텐츠(header/type-bar/body/footer)를 egui 로 tessellate 한 mesh 를 host 가
//! content 영역에 합성한다. host 는 Theme 스냅샷을 `popup.set_context` 에 실어 매 frame
//! 보내고, plugin 은 그것을 `Theme::with_colors_and_zoom` 으로 재구성해 디자인 토큰대로
//! 그린다. chrome(scrim/border/outside-click/Esc/단일 인스턴스 셸)은 host 소유 — plugin
//! 은 content 만 그린다.
//!
//! UI 는 header(아이콘+타이틀+snapshot 뱃지+close) → type-bar(타입 1개면 아이콘+뱃지,
//! 2개 이상이면 가로 세그먼트 스위치) → body(선택 타입의 상세) → footer(mime+Close)
//! 4단 수직 스택(rail 없음). Text/HTML 타입을 지원하고, 이미지/기타
//! 포맷/파일 목록 등은 `clipboard::ClipboardType` enum arm + reader 추가로 확장한다.
//! read-only 라 쓰기/붙여넣기/제거 액션은 없다.

// 이유: 테스트 본문의 `let _ =` 는 정책이 사유를 요구하지 않는 자리라
// `clippy::let_underscore_must_use` 명부에 섞이면 안 된다 — 그 명부는 프로덕션에서
// 값을 버리는 자리의 목록이고, 테스트가 늘 때마다 숫자만 흔들리면 새 프로덕션
// 자리가 그 안에 묻힌다(docs/dev-guide/error-handling.md). `cfg_attr(test, ..)` 라
// 라이브러리 타깃의 판정은 그대로다 — 프로덕션 자리는 여전히 명부에 오른다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

mod clipboard;
mod html_format;
mod raw_formats;
mod view;

use tasty_plugin_sdk::{
    Plugin, PluginEnv, PopupClosedCtx, PopupOpenCtx, PopupOpenResult, PopupSetContextCtx,
    SurfaceCreateCtx, SurfaceResult, Translator,
};

use crate::clipboard::{ClipboardType, ContentRepr};

#[cfg(any(unix, windows))]
use std::collections::{HashMap, HashSet};
#[cfg(any(unix, windows))]
use std::sync::Arc;

#[cfg(any(unix, windows))]
use tasty_plugin_protocol::ThemeWire;
#[cfg(any(unix, windows))]
use tasty_plugin_sdk::EguiMeshPopup;
#[cfg(any(unix, windows))]
use tasty_type_appearance::theme::Theme;

const PLUGIN_ID: &str = "com.tasty.clipboard-viewer";
const PLUGIN_VERSION: &str = env!("CARGO_PKG_VERSION");

/// open_popup 시점에 읽어둔 클립보드 스냅샷 + 좌측 선택 상태(paint 클로저에서 갱신).
pub(crate) struct ViewerState {
    pub(crate) available: Vec<(ClipboardType, ContentRepr)>,
    pub(crate) read_error: Option<String>,
    pub(crate) selected: Option<ClipboardType>,
    /// HTML 타입의 "Pretty print" 체크박스 상태. popup 인스턴스 생존 동안만
    /// 유지 — 설정에 영속화하지 않고 `load()`(popup 재오픈)마다 리셋된다.
    pub(crate) html_pretty: bool,
}

impl ViewerState {
    /// 현재 클립보드를 1회 읽어 스냅샷을 만든다. 첫 가용 타입을 기본 선택.
    fn load() -> Self {
        match clipboard::read_available() {
            Ok(available) => {
                let selected = available.first().map(|(ty, _)| *ty);
                Self {
                    available,
                    read_error: None,
                    selected,
                    html_pretty: false,
                }
            }
            Err(e) => {
                tracing::warn!("clipboard read failed: {e}");
                Self {
                    available: Vec::new(),
                    read_error: Some(e),
                    selected: None,
                    html_pretty: false,
                }
            }
        }
    }
}

struct ClipboardViewerPlugin {
    /// 단일 인스턴스 가드 — 주 인스턴스 id. 두 번째 open 은 "이미 열림" placeholder.
    primary_instance: Option<u64>,
    /// 주 인스턴스의 클립보드 스냅샷 + 선택 상태.
    state: Option<ViewerState>,
    /// instance_id → egui-mesh popup 렌더 상태(폰트 atlas·shared buffer 소유). unix 전용.
    #[cfg(any(unix, windows))]
    popups: HashMap<u64, EguiMeshPopup>,
    /// CJK fallback 폰트를 이미 설치한 instance_id — set_fonts 재업로드 방지.
    #[cfg(any(unix, windows))]
    fonts_installed: HashSet<u64>,
    tr: Translator,
}

impl ClipboardViewerPlugin {
    fn new(env: &PluginEnv) -> Self {
        Self {
            primary_instance: None,
            state: None,
            #[cfg(any(unix, windows))]
            popups: HashMap::new(),
            #[cfg(any(unix, windows))]
            fonts_installed: HashSet::new(),
            tr: Translator::from_plugin_env(env),
        }
    }
}

impl Plugin for ClipboardViewerPlugin {
    fn id(&self) -> &str {
        PLUGIN_ID
    }

    fn version(&self) -> &str {
        PLUGIN_VERSION
    }

    // popup-only plugin 이라 surface 콜백은 빈 결과.
    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult::default()
    }

    fn open_popup(&mut self, ctx: PopupOpenCtx) -> PopupOpenResult {
        // egui-mesh popup 은 tree(UiNode) 를 반환하지 않는다 — mesh 채널(paint_popup)로 그린다.
        // 첫 인스턴스면 클립보드 스냅샷을 적재하고 주 인스턴스로 등록. 그 외(이미 열려
        // 있는 상태의 재호출)는 주 인스턴스가 아니라 paint 시 "이미 열림" placeholder 로 그린다.
        if self.primary_instance.is_none() {
            self.primary_instance = Some(ctx.instance_id);
            self.state = Some(ViewerState::load());
        }
        PopupOpenResult::default()
    }

    fn paint_popup(&mut self, ctx: PopupSetContextCtx) {
        self.paint(ctx);
    }

    fn on_popup_closed(&mut self, ctx: PopupClosedCtx) {
        let iid = ctx.instance_id;
        #[cfg(any(unix, windows))]
        {
            self.popups.remove(&iid);
            self.fonts_installed.remove(&iid);
        }
        if self.primary_instance == Some(iid) {
            self.primary_instance = None;
            self.state = None;
        }
    }
}

impl ClipboardViewerPlugin {
    /// `popup.set_context` 한 frame 을 그려 host 에 popup mesh 를 회신한다.
    #[cfg(any(unix, windows))]
    fn paint(&mut self, ctx: PopupSetContextCtx) {
        let iid = ctx.params.instance_id;

        // host 가 Theme 을 아직 안 보냈으면(theme 미동봉) 토큰을 풀 수 없으므로 이 frame 은
        // 건너뛴다. host 는 테마 변경/입력 시 theme 을 동봉해 재forward 한다(markdown 동형).
        let Some(theme) = ctx.params.theme.as_ref().map(theme_from_wire) else {
            tracing::debug!("clipboard popup {iid}: set_context without theme — skipping paint");
            return;
        };

        // 서로소 필드를 지역 참조로 분리 — 클로저가 self 전체를 잡지 않게 한다.
        let Self {
            primary_instance,
            state,
            popups,
            fonts_installed,
            tr,
        } = self;
        let is_primary = *primary_instance == Some(iid);

        let popup = popups.entry(iid).or_insert_with(|| EguiMeshPopup::new(iid));
        // 한글/일문이 tofu(□) 되지 않도록 CJK fallback 을 popup Context 에 1회 설치한다.
        if fonts_installed.insert(iid) {
            install_fonts(popup.context());
        }

        // view::draw*는 header/footer Close 클릭 여부를 bool로 반환하지만, paint()의
        // run_ui 클로저 자체는 반환값이 없다(FnMut(&Context)) — markdown 확인 팝업과
        // 동형으로 클로저 밖에 캡처해둔 변수에 대입한 뒤, paint() 리턴 후 그 값을 보고
        // close_popup 을 호출한다.
        let mut close_requested = false;
        let result = popup.paint(&ctx.host, &ctx.params, |egui_ctx| {
            close_requested = match (is_primary, state.as_mut()) {
                (true, Some(st)) => view::draw(egui_ctx, &theme, st, tr),
                // 주 인스턴스가 아니거나(중복 open) 스냅샷이 없으면 "이미 열림" placeholder.
                _ => view::draw_already_open(egui_ctx, &theme, tr),
            };
        });
        if let Err(e) = result {
            tracing::warn!("clipboard popup {iid} paint failed: {e}");
        }
        if close_requested {
            close_popup(&ctx.host, iid);
        }
    }

    /// egui-mesh shared-buffer 송신을 지원하지 않는 exotic 타깃 — no-op(크로스플랫폼
    /// 컴파일 보장). Unix/Windows 는 위 실제 paint 를 쓴다.
    #[cfg(not(any(unix, windows)))]
    fn paint(&mut self, _ctx: PopupSetContextCtx) {}
}

/// wire 스냅샷을 host 와 동일한 `Theme` 인스턴스로 재구성 (sizing 은 zoom 으로 재도출).
#[cfg(any(unix, windows))]
fn theme_from_wire(w: &ThemeWire) -> Theme {
    Theme::with_colors_and_zoom(w.colors.clone(), w.is_light, w.ui_zoom)
}

/// host 에 팝업 인스턴스 닫기를 요청한다(셸 생명주기는 host 소유 — markdown 확인
/// 팝업과 동일 패턴).
#[cfg(any(unix, windows))]
fn close_popup(host: &tasty_plugin_sdk::HostHandle, instance_id: u64) {
    if let Err(e) = host.call(
        "popup.close",
        serde_json::json!({ "instance_id": instance_id }),
    ) {
        tracing::warn!("clipboard viewer popup close failed: {e}");
    }
}

/// popup Context 에 CJK fallback 을 설치한다(markdown `install_fonts` 미러). egui 기본
/// 폰트(Proportional/Monospace) 뒤에 시스템 CJK 폰트를 붙여 한글/일문/한자 tofu 를 막는다.
#[cfg(any(unix, windows))]
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    if let Some(bytes) = load_system_cjk_font_data() {
        fonts.font_data.insert(
            "system_cjk".to_owned(),
            Arc::new(egui::FontData::from_owned(bytes)),
        );
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(fam)
                .or_default()
                .push("system_cjk".to_owned());
        }
    }
    ctx.set_fonts(fonts);
}

/// 시스템 CJK 폰트 바이트 로드 (host `font_registry::load_system_cjk_font_data` 미러).
#[cfg(any(unix, windows))]
fn load_system_cjk_font_data() -> Option<Vec<u8>> {
    #[cfg(target_os = "windows")]
    {
        // host font_registry 미러 — 맑은 고딕(한글 tofu 방지). 없으면 None.
        if let Ok(data) = std::fs::read("C:/Windows/Fonts/malgun.ttf") {
            return Some(data);
        }
    }
    #[cfg(target_os = "macos")]
    {
        for path in &[
            "/System/Library/Fonts/AppleSDGothicNeo.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        for path in &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ] {
            if let Ok(data) = std::fs::read(path) {
                return Some(data);
            }
        }
    }
    None
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();
    let env = PluginEnv::load()?;
    let plugin = ClipboardViewerPlugin::new(&env);
    tasty_plugin_sdk::run(plugin)
}
