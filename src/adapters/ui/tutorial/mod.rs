//! 튜토리얼 — GUI 전용 인앱 가이드 투어. **6번째 오버레이(마커 오버레이)** + 안내
//! 말풍선(callout) + 주제 목록 팝업으로 구성된다. 진입점은 "도구" 메뉴 항목뿐이며
//! (최초 자동표시 없음), 진행은 사용자 Next 클릭으로만 일어난다 — IPC/CLI 발화 API
//! 없음(불가침 원칙 1: Toast/Banner/Modifier-hint 와 동일 계열).
//!
//! - [`marker`] — 좌표 위에 그리는 정적 마커 링(+스포트라이트 scrim). hit-transparent.
//! - [`callout`] — 안내 말풍선(edge-avoidance 배치). 자기 영역 마우스만 소비.
//! - [`topic_popup`] — 주제 목록 PopupDef(CenteredFocused).
//!
//! 상태머신: 목록팝업 --[진행]--> step0 --[Next]--> … --[Next on last]--> 목록 재open.
//! Skip/Esc(any step) → 목록 재open(완전 종료 아님). Back → 이전 step(첫 step 숨김).
//!
//! 마커 좌표는 매 프레임 `LayoutContext`/`terminal_rect`/`tab_bar_height` 에서
//! 재해석(정적 stale 없음). 첫 주제는 focused pane/surface 로 타겟을 해석한다.

pub mod callout;
pub mod marker;
pub mod topic_popup;

use tasty_type_appearance::theme::Theme;

use crate::adapters::ui::LayoutContext;
use crate::i18n::t;
use crate::intent::{OpenPopupMode, UiIntent};
use crate::state::AppState;

/// 마커가 가리키는 화면 지점의 개념적 타겟. 첫 주제는 focused pane/surface 로 해석.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerTarget {
    /// 워크스페이스 = 콘텐츠 전체영역(탭+서피스+상태바, 사이드바 제외).
    ContentArea,
    /// 탭 헤더 = focused pane 상단 `tab_bar_height` 띠.
    TabHeader,
    /// 패인 = focused pane rect.
    Pane,
    /// 서피스 = focused surface rect.
    Surface,
}

/// 한 step — 마커 타겟 + 제목/본문 i18n 키.
pub struct Step {
    pub target: MarkerTarget,
    pub title_key: &'static str,
    pub body_key: &'static str,
}

/// 한 주제 — 이름/설명 i18n 키 + step 목록(컴파일타임).
pub struct Topic {
    pub title_key: &'static str,
    pub desc_key: &'static str,
    pub steps: &'static [Step],
}

/// 첫 주제 = "워크스페이스 · 패인 · 탭 · 서피스" 4 step. (향후 주제는 여기 추가.)
static TOPICS: &[Topic] = &[Topic {
    title_key: "tutorial.topic_basics_title",
    desc_key: "tutorial.topic_basics_desc",
    steps: &[
        Step {
            target: MarkerTarget::ContentArea,
            title_key: "tutorial.step_workspace_title",
            body_key: "tutorial.step_workspace_body",
        },
        Step {
            target: MarkerTarget::TabHeader,
            title_key: "tutorial.step_tab_title",
            body_key: "tutorial.step_tab_body",
        },
        Step {
            target: MarkerTarget::Pane,
            title_key: "tutorial.step_pane_title",
            body_key: "tutorial.step_pane_body",
        },
        Step {
            target: MarkerTarget::Surface,
            title_key: "tutorial.step_surface_title",
            body_key: "tutorial.step_surface_body",
        },
    ],
}];

/// 전 주제 목록(정적).
pub fn all_topics() -> &'static [Topic] {
    TOPICS
}

/// 진행 중 튜토리얼 위치.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActiveTutorial {
    pub topic: usize,
    pub step: usize,
}

/// 런타임 상태(GUI 전용) — `AppState` 필드로 산다(`ModifierHintRuntime` 와 동형).
#[derive(Default)]
pub struct TutorialRuntime {
    /// 진행 중 튜토리얼(없으면 비활성).
    pub active: Option<ActiveTutorial>,
    /// 목록 팝업 "진행" 이 큐한 시작 요청(topic index). 다음 프레임 오버레이가 소비.
    pub pending_start: Option<usize>,
    /// 목록 팝업에서 선택된 주제 index.
    pub popup_selected: usize,
}

impl TutorialRuntime {
    /// 목록 팝업 "진행" — 다음 프레임 오버레이가 step0 으로 시작하도록 큐.
    pub fn request_start(&mut self, topic: usize) {
        self.pending_start = Some(topic);
    }
}

/// `MarkerTarget` → 절대 rect(logical) 해석. 순수 함수(단위 테스트 대상). 해석
/// 불가(pane/surface 없음)면 `None` → 호출자가 콘텐츠 영역으로 폴백.
pub fn resolve_marker_rect(
    target: MarkerTarget,
    content_area: egui::Rect,
    pane: Option<egui::Rect>,
    surface: Option<egui::Rect>,
    tab_bar_height: f32,
) -> Option<egui::Rect> {
    match target {
        MarkerTarget::ContentArea => Some(content_area),
        MarkerTarget::TabHeader => {
            pane.map(|p| egui::Rect::from_min_size(p.min, egui::vec2(p.width(), tab_bar_height)))
        }
        MarkerTarget::Pane => pane,
        MarkerTarget::Surface => surface,
    }
}

/// 튜토리얼 오버레이 렌더 + 상태 전이. 매 프레임 `overlay::draw_overlays` 말미에서 호출한다
/// (팝업/toast/banner/modhint 위 최상위 레이어). 마커/scrim 은 hit-transparent,
/// 말풍선만 마우스를 소비한다.
pub fn draw_tutorial_overlay(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &crate::core::CoreState,
    layout_ctx: &LayoutContext,
    content_area: egui::Rect,
    theme: &Theme,
) {
    // 1) 목록 팝업이 큐한 시작 요청 소비.
    if let Some(topic) = state.tutorial.pending_start.take() {
        state.tutorial.active = Some(ActiveTutorial { topic, step: 0 });
    }

    let Some(active) = state.tutorial.active else {
        return;
    };
    let topics = all_topics();
    let Some(topic) = topics.get(active.topic) else {
        state.tutorial.active = None;
        return;
    };
    let Some(step) = topic.steps.get(active.step) else {
        state.tutorial.active = None;
        return;
    };

    // 2) 마커 rect 해석(매 프레임 — stale 없음).
    let focused_pane_id = state.focused_pane(engine).map(|p| p.id);
    let pane_rect = focused_pane_id
        .and_then(|id| layout_ctx.pane_rects.iter().find(|(pid, _)| *pid == id))
        .map(|(_, r)| *r)
        .or_else(|| layout_ctx.pane_rects.first().map(|(_, r)| *r));
    let surface_rect = state
        .focused_surface_id(engine)
        .and_then(|id| layout_ctx.surface_rects.iter().find(|(sid, _)| *sid == id))
        .map(|(_, r)| *r)
        .or_else(|| layout_ctx.surface_rects.first().map(|(_, r)| *r));
    let tab_bar_height = theme.tab_bar_height.value();
    let marker = resolve_marker_rect(
        step.target,
        content_area,
        pane_rect,
        surface_rect,
        tab_bar_height,
    )
    .unwrap_or(content_area);

    // 3) 스포트라이트 scrim + 마커 링 (Order::Tooltip painter, hit-transparent).
    let screen = ctx.screen_rect();
    let marker_layer =
        egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("tutorial_marker_layer"));
    let painter = ctx.layer_painter(marker_layer);
    marker::paint_spotlight_scrim(&painter, screen, marker, theme);
    marker::paint_marker(&painter, marker, theme);

    // 4) 말풍선(edge-avoidance) — 자기 영역 마우스 소비.
    let title = t(step.title_key);
    let body = t(step.body_key);
    let size = egui::vec2(
        callout::CALLOUT_W.value(),
        callout::callout_height(ctx, theme, body),
    );
    let placement = callout::place_callout(
        marker,
        size,
        screen,
        theme.spacing_md.value(),
        theme.spacing_sm.value(),
    );
    let first = active.step == 0;
    let last = active.step + 1 >= topic.steps.len();
    let click = callout::draw_callout(
        ctx,
        theme,
        placement,
        active.step + 1,
        topic.steps.len(),
        title,
        body,
        first,
        last,
    );

    // 5) Esc → Skip.
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));

    // 6) 전이.
    use callout::CalloutClick;
    match (click, esc) {
        (CalloutClick::Next, _) => {
            if last {
                end_and_reopen(state, active.topic);
            } else {
                state.tutorial.active = Some(ActiveTutorial {
                    topic: active.topic,
                    step: active.step + 1,
                });
            }
        }
        (CalloutClick::Back, _) => {
            if active.step > 0 {
                state.tutorial.active = Some(ActiveTutorial {
                    topic: active.topic,
                    step: active.step - 1,
                });
            }
        }
        (CalloutClick::Skip, _) | (CalloutClick::None, true) => {
            end_and_reopen(state, active.topic);
        }
        (CalloutClick::None, false) => {}
    }
}

/// 튜토리얼 종료 + 주제 목록 팝업 재open(완전 종료 아님).
fn end_and_reopen(state: &mut AppState, topic: usize) {
    state.tutorial.active = None;
    state.tutorial.popup_selected = topic;
    state.dispatch_intent(
        UiIntent::OpenPopup {
            id: topic_popup::TUTORIAL_TOPICS_POPUP_ID,
            mode: OpenPopupMode::CenteredFocused,
        }
        .from_user_menu("tutorial.reopen"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
    }

    #[test]
    fn content_area_target_always_resolves() {
        let content = r(100.0, 0.0, 900.0, 600.0);
        let got = resolve_marker_rect(MarkerTarget::ContentArea, content, None, None, 24.0);
        assert_eq!(got, Some(content));
    }

    #[test]
    fn tab_header_is_top_strip_of_pane() {
        let content = r(100.0, 0.0, 900.0, 600.0);
        let pane = r(100.0, 0.0, 900.0, 580.0);
        let got = resolve_marker_rect(MarkerTarget::TabHeader, content, Some(pane), None, 24.0)
            .expect("resolves");
        assert_eq!(got.min, pane.min);
        assert_eq!(got.width(), pane.width());
        assert_eq!(got.height(), 24.0);
    }

    #[test]
    fn pane_and_surface_pass_through() {
        let content = r(100.0, 0.0, 900.0, 600.0);
        let pane = r(100.0, 24.0, 900.0, 556.0);
        let surface = r(108.0, 32.0, 884.0, 540.0);
        assert_eq!(
            resolve_marker_rect(MarkerTarget::Pane, content, Some(pane), Some(surface), 24.0),
            Some(pane)
        );
        assert_eq!(
            resolve_marker_rect(
                MarkerTarget::Surface,
                content,
                Some(pane),
                Some(surface),
                24.0
            ),
            Some(surface)
        );
    }

    #[test]
    fn unresolved_pane_surface_is_none() {
        let content = r(100.0, 0.0, 900.0, 600.0);
        assert_eq!(
            resolve_marker_rect(MarkerTarget::Pane, content, None, None, 24.0),
            None
        );
        assert_eq!(
            resolve_marker_rect(MarkerTarget::Surface, content, None, None, 24.0),
            None
        );
    }

    #[test]
    fn first_topic_has_four_steps() {
        assert_eq!(all_topics().len(), 1);
        assert_eq!(all_topics()[0].steps.len(), 4);
    }
}
