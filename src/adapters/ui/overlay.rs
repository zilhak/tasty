//! Toast → banner → modifier-hint → tutorial 오버레이 체인. ADR-0024 가 이 4개를
//! Modal/Popup 과 다른 개념(마우스 소비·포커스·인터랙션 3축이 다름)으로 분리했으므로
//! `popup/` 아래가 아니라 여기 독립 모듈에 둔다. `draw_ctx`(`LayoutContext`)는
//! `popup::frame::draw_popup_layer` 와 이 체인이 같은 프레임에 공유하므로 호출자
//! (`ui::draw_popups`)가 한 번만 만들어 양쪽에 넘긴다.

use crate::state::AppState;

/// "더보기" 컨텍스트 메뉴가 열려 있는 배너 스코프(없으면 `None`). popup open 여부는
/// `AppState.popups`, 대상 surface 는 `dialogs` 타깃 필드가 따로 갖고 있어 조립이
/// 필요하다 — `draw_overlays` 본문에 인라인하면 인지 복잡도 예산을 넘어 별도 함수로 뺐다.
fn mouse_capture_more_menu_open_for(
    state: &AppState,
) -> Option<crate::adapters::ui::banner::BannerScope> {
    if !state
        .popups
        .is_open(crate::adapters::ui::mouse_capture_menu::MOUSE_CAPTURE_BANNER_MENU_POPUP_ID)
    {
        return None;
    }
    state
        .dialogs
        .mouse_capture_banner_menu_target
        .map(crate::adapters::ui::banner::BannerScope::Surface)
}

/// toast → banner → modifier-hint → tutorial 순서로 그린다(호출 순서가 곧 z-order —
/// 뒤에 그릴수록 위 레이어). 순서를 바꾸면 뒤 레이어가 앞 레이어를 가리는 시각적
/// 회귀가 생긴다.
pub(crate) fn draw_overlays(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    draw_ctx: &crate::adapters::ui::LayoutContext,
    terminal_rect: crate::model::PhysicalRect,
    scale_factor: f32,
) {
    // Toast 렌더링 (popup 위 레이어). 같은 LayoutContext를 공유한다.
    let reduced_motion = engine.settings.accessibility.reduced_motion;
    state
        .toasts
        .set_lifetime_ms(engine.settings.overlay.toast_duration_ms);
    state.toasts.draw(ctx, draw_ctx, reduced_motion);

    // Banner 렌더링 (toast 와 동일 LayoutContext). 배너는 스코프 콘텐츠 최상단(탭바
    // 아래)에 뜨며 자기 영역의 마우스를 소비한다 — `banner_hovered` 로 하위 레이어
    // 전파를 막는다(포커스는 받지 않음). View 스코프 배너는 각 View 가 지정한
    // 플레이스홀더에 뜬다 — 화면 상단(탭바 아래)을 기본 플레이스홀더로 둔다.
    let th = crate::theme::theme();
    let screen = ctx.screen_rect();
    let view_placeholder = Some(egui::Rect::from_min_max(
        egui::pos2(screen.left(), screen.top() + th.tab_bar_height.value()),
        screen.max,
    ));
    // "더보기" 컨텍스트 메뉴가 열려 있는 배너 스코프 — 열려 있는 동안 ⋯ 트리거를
    // hover 와 무관하게 active 강조 상태로 유지한다(디자인 확정값 §6-1). `BannerManager`
    // 자신은 popup 시스템을 모르므로 여기서 조립해 넘긴다(helper 로 분리 —
    // `draw_overlays` 의 인지 복잡도 예산).
    let more_menu_open_for = mouse_capture_more_menu_open_for(state);
    let banner_result = state.banners.draw(
        ctx,
        draw_ctx,
        &th,
        view_placeholder,
        reduced_motion,
        more_menu_open_for.as_ref(),
    );
    state.banner_hovered = banner_result.hovered;
    state.banner_layer = Some(banner_result.layer);
    if let Some((scope, trigger_rect)) = banner_result.more_clicked {
        crate::adapters::ui::mouse_capture_menu::open(state, ctx, &scope, trigger_rect);
    }

    // modifier-hint 오버레이 (toast/banner 인접 최상위 레이어). modifier 500ms 홀드 후
    // 표시, 마우스만 소비(키보드 포커스 불가 — 원칙3). 홀드 상태는 winit ModifiersChanged
    // (실사용자 입력)만 반영(원칙1). 놓는 시점의 지오메트리를 UpdateSettings 로 영속한다.
    let hint_result = crate::adapters::ui::modifier_hint_overlay::draw_modifier_hint(
        ctx,
        &mut state.modifier_hint,
        &engine.settings,
        &th,
        reduced_motion,
    );
    state.modifier_hint_hovered = hint_result.hovered;
    state.modifier_hint_layer = hint_result.layer;

    // 튜토리얼 오버레이 (마커 오버레이 + 안내 말풍선) — 팝업/toast/banner/modhint 위
    // 최상위 레이어. 마커/scrim 은 hit-transparent, 말풍선만 마우스 소비. 진입·진행은
    // 사용자 클릭으로만(원칙 1). 마커 좌표는 draw_ctx/terminal_rect 로 매 프레임 재해석.
    let content_area = egui::Rect::from_min_size(
        egui::pos2(
            terminal_rect.x.value() / scale_factor,
            terminal_rect.y.value() / scale_factor,
        ),
        egui::vec2(
            terminal_rect.width.value() / scale_factor,
            terminal_rect.height.value() / scale_factor,
        ),
    );
    crate::adapters::ui::tutorial::draw_tutorial_overlay(
        ctx,
        state,
        engine,
        draw_ctx,
        content_area,
        &th,
    );

    if let Some((pos, size)) = hint_result.persist {
        // 사용자 드래그/리사이즈 결과 → Settings 영속(사이드바 폭 등과 동일 성질,
        // 전역 공유 + last-write-wins). from_user_menu = 사용자 직접 조작 origin.
        let mut new_settings = engine.settings.clone();
        new_settings.modifier_hint.pos = Some(pos);
        new_settings.modifier_hint.size = Some(size);
        state.dispatch_intent(
            crate::core::intent::DomainIntent::UpdateSettings(new_settings)
                .from_user_menu("modifier_hint.geometry"),
        );
    }
}
