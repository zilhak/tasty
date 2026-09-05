//! Pure view 함수 + props/action — CSD 공통 titlebar 의 시각 / 입력 처리.
//!
//! 본 모듈은 `AppState` / `CoreState` / winit `Window` / 글로벌 `theme::theme()`
//! 에 접근하지 않는다. 호출처 wrapper (`titlebar::draw_titlebar`) 가 props 추출 +
//! action → winit window 조작 매핑을 담당한다. gallery 는 같은 view 를 mock props
//! 로 호출해 시각 검증한다 — props 분리 패턴(`docs/dev-guide/gallery-first.md`).

use crate::theme::Theme;

/// CSD titlebar 가 tasty 측에서 직접 그리는 윈도우 컨트롤 버튼.
///
/// Linux(P6)에서 DE(GNOME/KDE 등)별로 집합·순서가 달라지므로 데이터 드리븐으로
/// 둔다. macOS 는 네이티브 신호등을 유지(`controls: None`)하고 Windows 캡션은
/// P5 후속이라 이 enum 을 쓰지 않는다 — 그 빌드에선 variant 가 구성되지 않으므로
/// dead_code 를 허용한다(렌더/매핑 코드는 전 플랫폼 공통 컴파일).
// 이유: Linux DE 버튼만 이 enum 을 구성한다 — macOS 신호등·Windows 캡션 빌드엔 생성처가 없다.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowButton {
    Minimize,
    Maximize,
    Close,
}

/// 컨트롤 버튼 클러스터의 측면. Linux DE 관습(GNOME/KDE=우측, 일부 좌측).
// 이유: `WindowButton` 과 같다 — 이 측면 값을 만드는 곳이 Linux DE 프리셋뿐이다.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlSide {
    // 이유: DE 감지(GNOME=좌측 등) 전까지는 `titlebar/mod.rs::os_controls()` 가
    // 단일 우측 프리셋만 생성 — 후속 DE 프리셋 확장 시 첫 실사용처가 된다.
    #[allow(dead_code)]
    Left,
    Right,
}

/// tasty 가 그리는 DE 가변 버튼 묶음 (집합·순서·측면). 디자인 `titlebar_linux.jsx`
/// 의 `buttons`/`side` props 에 대응. 비어 있으면 버튼을 그리지 않는다.
#[derive(Debug, Clone)]
pub struct TitlebarControls {
    /// 좌→우(또는 측면 기준) 순서대로 그릴 버튼.
    pub buttons: Vec<WindowButton>,
    /// 클러스터를 titlebar 의 어느 쪽에 붙일지.
    pub side: ControlSide,
}

/// 공통 titlebar view 의 입력. 색은 P1 titlebar 토큰, 높이는 사전 해상.
pub struct TitlebarProps<'a> {
    pub theme: &'a Theme,
    /// 윈도우 포커스 여부 — active/inactive 디밍 결정.
    pub active: bool,
    /// titlebar 높이 (logical points = egui 좌표). theme `titlebar_height` 토큰.
    pub height: f32,
    /// 좌측 컨트롤 슬롯 폭 (logical points). macOS 네이티브 신호등
    /// (standardWindowButton)이 좌상단에 OS 렌더되는 영역 — 이 폭만큼은 드래그 hit 를
    /// 두지 않아 신호등 클릭이 드래그로 새지 않게 한다. 신호등 없는 OS 에서는 0.
    pub left_inset: f32,
    /// tasty 가 직접 그리는 윈도우 컨트롤 버튼 (Linux DE 가변). `None` 이면 그리지
    /// 않는다(macOS 네이티브 신호등; Windows 는 전용 caption.rs 로 그림).
    pub controls: Option<TitlebarControls>,
    /// 윈도우 maximize 상태 — Windows 캡션 버튼의 maximize↔restore 글리프 토글에
    /// 사용한다. 캡션 버튼이 없는 OS 에서는 무시된다.
    // 이유: 이 필드를 읽는 곳이 Windows 캡션 버튼의 글리프 토글뿐이다(위).
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    pub maximized: bool,
}

/// titlebar view 가 보고하는 사용자 의도. wrapper 가 winit window 조작 / app 이벤트로 변환.
///
/// `Minimize`/`Close` 는 Linux DE 버튼(`draw_window_buttons`)·Windows 캡션
/// (`caption.rs`)에서 생성된다. macOS 는 네이티브 신호등이라 StartDrag/ToggleMaximize
/// 만 생성하므로 그 빌드에선 미사용 variant 의 dead_code 를 허용한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
// 이유: macOS 는 네이티브 신호등이라 `Minimize`/`Close` 생성처가 그 빌드에 없다(위).
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub enum TitlebarAction {
    /// 비인터랙티브(드래그) 영역에서 드래그 시작 → 윈도우 이동.
    StartDrag,
    /// 드래그 영역 더블클릭 또는 maximize 버튼 → maximize 토글.
    ToggleMaximize,
    /// minimize 버튼 → 윈도우 최소화.
    Minimize,
    /// close 버튼 → 윈도우 닫기(quit/close 라이프사이클 라우팅).
    Close,
}

/// [`draw_titlebar_view`] 의 반환값 — 사용자 액션 + 가장자리 리사이즈 우선권 판정.
pub struct TitlebarDrawResult {
    /// wrapper 가 winit window 조작/app 이벤트로 변환할 사용자 의도.
    pub actions: Vec<TitlebarAction>,
    /// 마우스가 타이틀바의 실제 인터랙티브 버튼(창 컨트롤·Windows 캡션) 위인지.
    /// `AppState.resize_edge_widget_hovered` 로 흘러가 `try_begin_os_resize` 가
    /// 가장자리 margin 안에서 리사이즈를 양보할지 판단할 때 쓰인다 — 버튼이 없는
    /// 빈 타이틀바 여백은 여기 포함되지 않아 리사이즈가 항상 우선한다.
    pub resize_priority_hovered: bool,
}

/// 공통 CSD titlebar 를 `egui::TopBottomPanel::top` 으로 그린다.
///
/// full-width 상단 바 + 배경/하단 보더(active/inactive 디밍) + 드래그/더블클릭 보고.
/// `controls` 가 있으면 DE 가변 버튼(min·max·close)을 측면에 그리고 그 영역은
/// 드래그에서 카브-아웃한다(클릭이 드래그로 새지 않게). macOS 신호등 영역은
/// `left_inset` 으로 카브-아웃한다.
pub fn draw_titlebar_view(ctx: &egui::Context, props: &TitlebarProps) -> TitlebarDrawResult {
    let th = props.theme;
    let mut actions = Vec::new();
    let mut resize_priority_hovered = false;

    let bg = if props.active {
        th.titlebar_bg()
    } else {
        th.titlebar_bg_inactive()
    };

    egui::TopBottomPanel::top("tasty_titlebar")
        .exact_height(props.height)
        .frame(egui::Frame::new().fill(bg.to_egui()))
        .show_separator_line(false)
        .show(ctx, |ui| {
            let rect = ui.max_rect();

            // ── DE 가변 컨트롤 버튼(Linux, 있으면) 먼저 배치해 strip 폭을 확정한다 ──
            let mut left_controls_w = 0.0_f32;
            let mut right_controls_w = 0.0_f32;
            if let Some(controls) = &props.controls
                && !controls.buttons.is_empty()
            {
                let strip = draw_window_buttons(
                    ui,
                    rect,
                    props,
                    controls,
                    &mut actions,
                    &mut resize_priority_hovered,
                );
                match controls.side {
                    ControlSide::Left => left_controls_w = strip,
                    ControlSide::Right => right_controls_w = strip,
                }
            }

            // ── Windows 캡션 버튼(우측). 전용 caption.rs(46px, close-hover red)로 그리고
            //    그 폭을 우측 strip 으로 잡는다. Windows 는 controls=None 이라 위 DE 블록과
            //    배타적. ──
            #[cfg(target_os = "windows")]
            {
                let caption_w = super::caption::cluster_width(th);
                let caption_rect = egui::Rect::from_min_max(
                    egui::pos2(rect.right() - caption_w, rect.top()),
                    rect.max,
                );
                let caption_result = super::caption::draw_caption_buttons(ui, caption_rect, props);
                actions.extend(caption_result.actions);
                resize_priority_hovered |= caption_result.hovered;
                // Windows 는 controls=None(위 DE 블록과 배타)이라 사실상 = caption_w.
                // max 로 합성해 두 경로가 한 변수로 흐르게 한다 (dead-store 경고 방지 겸).
                right_controls_w = right_controls_w.max(caption_w);
            }

            // 드래그 영역 = 전체에서 좌측 inset(신호등/좌측버튼)·우측 strip(DE 버튼/Windows
            // 캡션)을 뺀 나머지. 버튼 rect 와 겹치지 않게 해 버튼 클릭이 드래그로 새는 것을
            // 막는다.
            let drag_left = rect.left() + props.left_inset + left_controls_w;
            let drag_right = rect.right() - right_controls_w;
            if drag_right > drag_left {
                let drag_rect = egui::Rect::from_min_max(
                    egui::pos2(drag_left, rect.top()),
                    egui::pos2(drag_right, rect.bottom()),
                );
                let resp = ui.interact(
                    drag_rect,
                    egui::Id::new("tasty_titlebar_drag"),
                    egui::Sense::click_and_drag(),
                );
                if resp.double_clicked() {
                    actions.push(TitlebarAction::ToggleMaximize);
                } else if resp.drag_started() {
                    actions.push(TitlebarAction::StartDrag);
                }
            }

            // 하단 1px 보더 (ui_kit `--tasty-titlebar-border`).
            ui.painter().hline(
                rect.x_range(),
                rect.bottom() - 0.5,
                egui::Stroke::new(th.border_width.value(), th.titlebar_border().to_egui()),
            );
        });

    TitlebarDrawResult {
        actions,
        resize_priority_hovered,
    }
}

/// DE 가변 버튼 클러스터를 측면에 그리고, 차지한 strip 폭(logical px)을 반환한다.
/// 클릭은 `actions` 로, 버튼 hover 는 `hovered` 로 보고한다(OR 누적 — 호출부가
/// 매 프레임 `false` 로 초기화한 값을 넘긴다).
fn draw_window_buttons(
    ui: &egui::Ui,
    rect: egui::Rect,
    props: &TitlebarProps,
    controls: &TitlebarControls,
    actions: &mut Vec<TitlebarAction>,
    hovered: &mut bool,
) -> f32 {
    let th = props.theme;
    let d = th.window_button_size.value(); // 원형 버튼 지름
    let edge_pad = th.spacing_sm.value(); // 측면 끝 여백
    let gap = th.spacing_xs.value(); // 버튼 간 간격
    let n = controls.buttons.len();
    let strip_w = edge_pad * 2.0 + d * n as f32 + gap * (n.saturating_sub(1)) as f32;
    let cy = rect.center().y;

    // 측면에 따라 첫 버튼의 중심 x 와 진행 방향을 정한다.
    let (mut cx, step) = match controls.side {
        ControlSide::Right => (rect.right() - edge_pad - d * 0.5, -(d + gap)),
        ControlSide::Left => (rect.left() + edge_pad + d * 0.5, d + gap),
    };

    // Right 측면일 때 buttons 의 마지막이 가장 바깥(끝)에 오도록 역순으로 그린다 —
    // [min, max, close] + Right → close 가 가장 우측. step 이 음수라 첫 그리기를
    // close 부터 하면 close 가 우측 끝. 따라서 Right 는 역순 순회.
    let order: Vec<&WindowButton> = match controls.side {
        ControlSide::Right => controls.buttons.iter().rev().collect(),
        ControlSide::Left => controls.buttons.iter().collect(),
    };

    for (i, &&button) in order.iter().enumerate() {
        let center = egui::pos2(cx, cy);
        let btn_rect = egui::Rect::from_center_size(center, egui::vec2(d, d));
        let id = egui::Id::new(("tasty_titlebar_btn", i, button as u8 as usize));
        let label = match button {
            WindowButton::Minimize => crate::i18n::t("titlebar.minimize"),
            WindowButton::Maximize => crate::i18n::t("titlebar.maximize"),
            WindowButton::Close => crate::i18n::t("titlebar.close"),
        };
        let resp = ui
            .interact(btn_rect, id, egui::Sense::click())
            .on_hover_text(label);
        *hovered |= resp.hovered();

        let is_close = matches!(button, WindowButton::Close);
        // hover/active 배경: close 는 시스템 red, 그 외는 overlay.
        let bg = if resp.is_pointer_button_down_on() {
            if is_close {
                Some(th.accent_window_close())
            } else {
                Some(th.active_overlay)
            }
        } else if resp.hovered() {
            if is_close {
                Some(th.accent_window_close())
            } else {
                Some(th.hover_overlay)
            }
        } else {
            None
        };
        if let Some(bg) = bg {
            ui.painter().circle_filled(center, d * 0.5, bg.to_egui());
        }

        // 글리프 색: close hover 시 white, 그 외 active/inactive 디밍.
        let fg = if is_close && resp.hovered() {
            th.text_on_window_close()
        } else if props.active {
            th.titlebar_fg()
        } else {
            th.titlebar_fg_inactive()
        };
        let stroke = egui::Stroke::new(th.border_width.value(), fg.to_egui());
        draw_button_glyph(ui.painter(), center, d, &button, stroke);

        if resp.clicked() {
            actions.push(match button {
                WindowButton::Minimize => TitlebarAction::Minimize,
                WindowButton::Maximize => TitlebarAction::ToggleMaximize,
                WindowButton::Close => TitlebarAction::Close,
            });
        }

        cx += step;
    }

    strip_w
}

/// 버튼 글리프(min=가로선, max=사각형, close=×)를 중심 기준으로 그린다.
fn draw_button_glyph(
    painter: &egui::Painter,
    center: egui::Pos2,
    d: f32,
    button: &WindowButton,
    stroke: egui::Stroke,
) {
    let g = d * 0.22; // 글리프 반경(중심에서의 extent)
    match button {
        WindowButton::Minimize => {
            painter.line_segment(
                [
                    egui::pos2(center.x - g, center.y),
                    egui::pos2(center.x + g, center.y),
                ],
                stroke,
            );
        }
        WindowButton::Maximize => {
            let r = egui::Rect::from_center_size(center, egui::vec2(g * 2.0, g * 2.0));
            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Inside);
        }
        WindowButton::Close => {
            painter.line_segment(
                [
                    egui::pos2(center.x - g, center.y - g),
                    egui::pos2(center.x + g, center.y + g),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(center.x - g, center.y + g),
                    egui::pos2(center.x + g, center.y - g),
                ],
                stroke,
            );
        }
    }
}
