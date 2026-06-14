//! Pure view 함수 + props/action — CSD 공통 titlebar 의 시각 / 입력 처리.
//!
//! 본 모듈은 `AppState` / `CoreState` / winit `Window` / 글로벌 `theme::theme()`
//! 에 접근하지 않는다. 호출처 wrapper (`titlebar::draw_titlebar`) 가 props 추출 +
//! action → winit window 조작 매핑을 담당한다. gallery 는 같은 view 를 mock props
//! 로 호출해 시각 검증한다 — Tier 3 패턴
//! (`.claude-workspace/conductor/tier-3-props-extraction-pattern.md`).

use crate::theme::Theme;

/// 공통 titlebar view 의 입력. 색은 P1 titlebar 토큰, 높이는 사전 해상.
pub struct TitlebarProps<'a> {
    pub theme: &'a Theme,
    /// 윈도우 포커스 여부 — active/inactive 디밍 결정.
    pub active: bool,
    /// titlebar 높이 (logical points = egui 좌표). theme `titlebar_height` 토큰.
    pub height: f32,
}

/// titlebar view 가 보고하는 사용자 의도. wrapper 가 winit window 조작으로 변환.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlebarAction {
    /// 비인터랙티브(드래그) 영역에서 드래그 시작 → 윈도우 이동.
    StartDrag,
    /// 드래그 영역 더블클릭 → maximize 토글.
    ToggleMaximize,
}

/// 공통 CSD titlebar 를 `egui::TopBottomPanel::top` 으로 그린다.
///
/// P3 범위: full-width 상단 바 + 배경/하단 보더(active/inactive 디밍) + 전체를
/// 드래그 영역으로 잡아 드래그/더블클릭 액션을 보고한다. OS별 컨트롤(신호등 /
/// 캡션 버튼)은 P4~P6 에서 좌/우 슬롯을 비-드래그 영역으로 카브-아웃한다.
pub fn draw_titlebar_view(ctx: &egui::Context, props: &TitlebarProps) -> Vec<TitlebarAction> {
    let th = props.theme;
    let mut actions = Vec::new();

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

            // 전체 바를 드래그 영역으로. P4~P6 에서 컨트롤 슬롯이 carve-out 되면
            // 그 rect 들은 별도 Sense::click 위젯이 우선 소비한다.
            let resp = ui.interact(
                rect,
                egui::Id::new("tasty_titlebar_drag"),
                egui::Sense::click_and_drag(),
            );
            if resp.double_clicked() {
                actions.push(TitlebarAction::ToggleMaximize);
            } else if resp.drag_started() {
                actions.push(TitlebarAction::StartDrag);
            }

            // 하단 1px 보더 (ui_kit `--tasty-titlebar-border`).
            ui.painter().hline(
                rect.x_range(),
                rect.bottom() - 0.5,
                egui::Stroke::new(th.border_width.value(), th.titlebar_border().to_egui()),
            );

            // OS별 컨트롤 슬롯(신호등 / 캡션 버튼)은 P4~P6 후속 — 현재는 빈 슬롯.
        });

    actions
}
