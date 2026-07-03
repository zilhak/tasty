//! Modifier-hint 오버레이 **본체** — 홀드→표시 수명주기 + 드래그/리사이즈/영속 + 커스텀 draw.
//!
//! 4분류(Popup/Toast/Banner/Modal) 어디에도 안 맞는 **신규 오버레이 요소**다:
//! **키보드 포커스 없음 + 마우스 인터랙티브(드래그 이동·테두리/코너 리사이즈·X) + 홀드 수명**.
//! modifier 를 500ms 이상 홀드하면 200ms 페이드(opacity 0.2→1.0)로 등장하고, 키를 떼면
//! 즉시 소멸한다. 콘텐츠 모델은 [`super::input::shortcuts::modifier_hint`](modifier-hint-02),
//! 시각 토큰은 `Theme::modhint_*()`(modifier-hint-01 슬롯 + design-token-mapping).
//!
//! ## 불가침 원칙
//! - **원칙1(사용자↔에이전트 분리)**: 홀드 상태는 winit `ModifiersChanged`(실제 사용자
//!   입력)만 반영한다. IPC/CLI 로 강제 표시할 수 없다. X dismiss·드래그·리사이즈는 사용자
//!   마우스만.
//! - **원칙3(포커스 독립성)**: 키보드 포커스를 **절대** 취득하지 않는다. 마우스만 소비하며,
//!   `AppState::modifier_hint_hovered` 가드가 `mouse.rs` 4지점에서 하위 surface 로의 전파
//!   (click-to-activate/휠/드래그)를 막는다.
//!
//! ## Model / Runtime / View 분리
//! - [`hold_reveal_alpha`] / [`default_rect`] / [`clamp_rect`] / [`resize_to`] — 순수 함수,
//!   테스트로 고정.
//! - [`ModifierHintRuntime`] — `AppState` 에 사는 홀드 상태 + 진행 중 드래그 working rect.
//! - [`draw_modifier_hint`] — 매 프레임 draw. `draw_popups` 가 toast/banner 인접에서 호출한다.

use std::time::Instant;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use super::input::shortcuts::modifier_hint::{
    HeldModifier, HintRole, HintRowSource, HintSection, build_hint_sections,
};

/// 진행 중 드래그/리사이즈 모드.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DragMode {
    /// 드래그 스트립 → 패널 이동.
    Move,
    /// 우하단 코너 그립 → 리사이즈(min 클램프).
    Resize,
}

/// modifier-hint 오버레이의 런타임 상태. `AppState` 필드(gui 전용).
///
/// 홀드 상태(시작 시각·anchor modifier·세션 dismiss)와 진행 중 드래그 working rect 를 담는다.
/// 지오메트리 영속값은 `Settings::modifier_hint`(pos/size)에 있고, working 은 드래그 중의
/// 임시 실시간 rect 다(놓는 시점에 Settings 로 커밋).
#[derive(Debug, Clone, Default)]
pub struct ModifierHintRuntime {
    /// 단일 modifier 홀드 시작 시각. `None` = 홀드 아님.
    hold_since: Option<Instant>,
    /// 현재 홀드의 anchor modifier(이 값을 포함하는 모든 조합을 노출).
    held: Option<HeldModifier>,
    /// 이번 홀드 세션 X dismiss 여부. **전 modifier 를 떼면** `false` 로 리셋된다.
    dismissed: bool,
    /// 드래그/리사이즈 진행 중 실시간 rect(logical, 미클램프 원본) + 모드. `None` = 유휴.
    working: Option<(egui::Rect, DragMode)>,
}

impl ModifierHintRuntime {
    /// 홀드 상태를 갱신한다. `ModifiersChanged` 훅이 플랫폼 정규화된 축 bool 로 호출한다.
    ///
    /// - 하나도 안 눌림 → 전부 clear(+ dismissed 리셋). 반환 `true`(표시 갱신 필요).
    /// - 하나 이상 눌림 → anchor 가 아직 눌려 있으면 유지, 아니면 우선순위(Ctrl<Alt<Option
    ///   <Shift)로 재선정. 타이머(`hold_since`)는 "하나라도 눌린 동안" 유지 — 조합이 바뀌어도
    ///   리셋하지 않고 콘텐츠만 따라 바뀐다(anchor 조합이 이미 모든 상위 조합을 포함).
    ///
    /// 반환 = 상태가 바뀌어 redraw 가 필요한지.
    pub fn update_hold(&mut self, ctrl: bool, alt: bool, option: bool, shift: bool) -> bool {
        let any = ctrl || alt || option || shift;
        if !any {
            let changed = self.hold_since.is_some() || self.held.is_some() || self.dismissed;
            self.hold_since = None;
            self.held = None;
            self.dismissed = false;
            self.working = None;
            return changed;
        }
        let axis_down = |m: HeldModifier| match m {
            HeldModifier::Ctrl => ctrl,
            HeldModifier::Alt => alt,
            HeldModifier::Option => option,
            HeldModifier::Shift => shift,
        };
        let anchor_valid = self.held.is_some_and(axis_down);
        let prev = self.held;
        if !anchor_valid {
            self.held = pick_anchor(ctrl, alt, option, shift);
        }
        if self.hold_since.is_none() {
            self.hold_since = Some(Instant::now());
        }
        prev != self.held
    }

    /// 창 포커스 상실 등에서 홀드 상태를 전부 비운다(switch-overlay clear 와 동반).
    pub fn clear(&mut self) {
        self.hold_since = None;
        self.held = None;
        self.dismissed = false;
        self.working = None;
    }
}

/// 눌린 축 중 anchor 로 쓸 modifier 를 우선순위(Ctrl<Alt<Option<Shift)로 선정.
fn pick_anchor(ctrl: bool, alt: bool, option: bool, shift: bool) -> Option<HeldModifier> {
    if ctrl {
        Some(HeldModifier::Ctrl)
    } else if alt {
        Some(HeldModifier::Alt)
    } else if option {
        Some(HeldModifier::Option)
    } else if shift {
        Some(HeldModifier::Shift)
    } else {
        None
    }
}

/// 홀드 경과시간(ms) → 오버레이 alpha. `None` = 아직 표시 안 함(지연 전).
///
/// - `held_ms < delay_ms` → `None`(홀드 지연 게이트 통과 전 — 실수 스침 억제).
/// - `reduced_motion` → 게이트 통과 즉시 `Some(1.0)`(페이드 생략, **지연은 유지**).
/// - 그 외 → `[delay, delay+fade]` 구간에서 opacity **0.2→1.0** 선형 페이드.
///
/// 디자인 확정: 등장은 투명도 80%→0%(alpha 0.2→1.0). `delay_ms`/`fade_ms` 는 Theme 토큰
/// (`motion_hold_reveal_ms` / `motion_ui_fade_ms`)에서 주입 — 순수 함수라 테스트로 고정한다.
pub fn hold_reveal_alpha(
    held_ms: f32,
    delay_ms: f32,
    fade_ms: f32,
    reduced_motion: bool,
) -> Option<f32> {
    if held_ms < delay_ms {
        return None;
    }
    if reduced_motion {
        return Some(1.0);
    }
    let t = held_ms - delay_ms;
    if t >= fade_ms {
        Some(1.0)
    } else {
        // 0.2 → 1.0 선형.
        Some((0.2 + 0.8 * (t / fade_ms)).clamp(0.2, 1.0))
    }
}

/// 저장된 위치/크기가 없을 때의 기본 지오메트리 — **사이드바 하단 anchor**(접힘/펼침 무관
/// 동일). 화면 좌하단에 margin 을 두고 220×400(min 아님, 기본값)으로 배치한다.
pub fn default_rect(screen: egui::Rect, theme: &Theme) -> egui::Rect {
    let w = theme.modhint_width().value();
    let h = theme.modhint_height().value();
    let margin = theme.spacing_sm.value();
    let left = screen.left() + margin;
    let bottom = screen.bottom() - margin;
    egui::Rect::from_min_size(egui::pos2(left, bottom - h), egui::vec2(w, h))
}

/// rect 를 화면 안으로 클램프한다 — **저장값은 불변, 렌더만 클램프**(윈도우 축소 대응).
/// 크기가 화면보다 크면 크기도 줄인다. 위치는 화면 안쪽으로 평행이동.
pub fn clamp_rect(rect: egui::Rect, screen: egui::Rect) -> egui::Rect {
    let w = rect.width().min(screen.width());
    let h = rect.height().min(screen.height());
    let x = rect
        .left()
        .clamp(screen.left(), (screen.right() - w).max(screen.left()));
    let y = rect
        .top()
        .clamp(screen.top(), (screen.bottom() - h).max(screen.top()));
    egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))
}

/// 코너 리사이즈 — 좌상단(min)을 고정하고 우하단을 `delta` 만큼 이동, min 크기로 클램프.
pub fn resize_to(rect: egui::Rect, delta: egui::Vec2, min_w: f32, min_h: f32) -> egui::Rect {
    let w = (rect.width() + delta.x).max(min_w);
    let h = (rect.height() + delta.y).max(min_h);
    egui::Rect::from_min_size(rect.min, egui::vec2(w, h))
}

// ── View (draw) ──────────────────────────────────────────────────────────────

use std::time::Duration;

use crate::i18n::t;
use tasty_settings::Settings;
use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant, kbd};

use crate::adapters::ui::icons;

/// `draw_modifier_hint` 결과 — 입력 레이어 배선 + 지오메트리 영속.
#[derive(Default, Clone, Copy)]
pub struct HintDrawResult {
    /// 마우스가 패널 위 → 하위 surface 전파 차단(`AppState::modifier_hint_hovered`).
    pub hovered: bool,
    /// 드래그/리사이즈를 놓은 시점의 (pos, size). `Some` 이면 호출자가 `UpdateSettings` 로
    /// 영속한다(사용자 행동 → `from_user_menu`). `None` = 이번 프레임 변경 없음.
    pub persist: Option<((LogicalPx, LogicalPx), (LogicalPx, LogicalPx))>,
}

/// modifier-hint 오버레이를 매 프레임 그린다. `draw_popups` 가 toast/banner 인접에서 호출.
///
/// 표시 조건: `enabled` && 홀드 중 && 홀드 500ms 경과 && !dismissed && 섹션 비어있지 않음.
/// 표시 안 하는 프레임엔 필요한 만큼만 `request_repaint(_after)` 를 예약해 유휴 CPU 낭비를
/// 막는다(지연 도달 시점 1회 예약 · 페이드 중에만 매 프레임).
pub fn draw_modifier_hint(
    ctx: &egui::Context,
    rt: &mut ModifierHintRuntime,
    settings: &Settings,
    theme: &Theme,
    reduced_motion: bool,
) -> HintDrawResult {
    let mut result = HintDrawResult::default();

    if !settings.modifier_hint.enabled {
        rt.working = None;
        return result;
    }
    let (Some(held), Some(since)) = (rt.held, rt.hold_since) else {
        return result;
    };
    if rt.dismissed {
        return result; // 이번 홀드 세션은 X 로 닫힘 — 재홀드(전 modifier release) 전까지 미표시.
    }

    let held_ms = since.elapsed().as_secs_f32() * 1000.0;
    let delay = theme.motion_hold_reveal_ms();
    let fade = theme.motion_ui_fade_ms();
    let Some(alpha) = hold_reveal_alpha(held_ms, delay, fade, reduced_motion) else {
        // 아직 지연 게이트 전 — 500ms 도달 시점에 깨어나도록 정확히 예약(busy-loop 아님).
        let remain = (delay - held_ms).max(1.0);
        ctx.request_repaint_after(Duration::from_millis(remain as u64));
        return result;
    };

    let sections = build_hint_sections(
        held,
        &settings.keybindings,
        &settings.general.link_click_modifier,
        settings.general.workspace_categories_enabled,
        &[], // plugin_bindings: PluginManager 는 App 소유라 draw 경로 미도달 → 후속 배선(open).
    );
    if sections.is_empty() {
        return result;
    }

    let screen = ctx.screen_rect();
    let base = rt
        .working
        .map(|(r, _)| r)
        .unwrap_or_else(|| rect_from_settings(settings, screen, theme));
    let render_rect = clamp_rect(base, screen);

    let layer = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("modhint_layer"));
    let mut ui = ui_at(ctx, layer, render_rect);

    draw_shell(&ui, theme, render_rect, alpha);
    draw_content(&mut ui, theme, render_rect, held, &sections, alpha);

    // ── 인터랙션: 드래그 스트립(이동) · 코너 그립(리사이즈) · X(dismiss) ──
    let strip_h = theme.modhint_strip_height().value();
    let x_zone = strip_h; // 우측 X 버튼 폭 만큼 드래그에서 제외.
    let strip_drag = egui::Rect::from_min_max(
        render_rect.min,
        egui::pos2(render_rect.right() - x_zone, render_rect.top() + strip_h),
    );
    let move_resp = ui
        .interact(
            strip_drag,
            egui::Id::new("modhint_move"),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::Move);
    if move_resp.drag_started() {
        rt.working = Some((render_rect, DragMode::Move));
    }
    if move_resp.dragged()
        && let Some((r, DragMode::Move)) = rt.working.as_mut()
    {
        *r = clamp_rect(r.translate(move_resp.drag_delta()), screen);
    }

    let grip = theme.modhint_grip_size().value();
    let grip_rect = egui::Rect::from_min_max(
        egui::pos2(render_rect.right() - grip, render_rect.bottom() - grip),
        render_rect.max,
    );
    let resize_resp = ui
        .interact(
            grip_rect,
            egui::Id::new("modhint_resize"),
            egui::Sense::drag(),
        )
        .on_hover_cursor(egui::CursorIcon::ResizeNwSe);
    if resize_resp.drag_started() {
        rt.working = Some((render_rect, DragMode::Resize));
    }
    if resize_resp.dragged()
        && let Some((r, DragMode::Resize)) = rt.working.as_mut()
    {
        let resized = resize_to(
            *r,
            resize_resp.drag_delta(),
            theme.modhint_min_width().value(),
            theme.modhint_min_height().value(),
        );
        *r = clamp_rect(resized, screen);
    }

    if (move_resp.drag_stopped() || resize_resp.drag_stopped())
        && let Some((r, _)) = rt.working.take()
    {
        let c = clamp_rect(r, screen);
        result.persist = Some((
            (LogicalPx(c.left()), LogicalPx(c.top())),
            (LogicalPx(c.width()), LogicalPx(c.height())),
        ));
    }

    // X 버튼 — 우상단 strip 안. 클릭 시 이번 홀드 세션 dismiss.
    let x_rect = egui::Rect::from_min_size(
        egui::pos2(render_rect.right() - x_zone, render_rect.top()),
        egui::vec2(x_zone, strip_h),
    );
    let mut x_ui = ui.new_child(egui::UiBuilder::new().max_rect(x_rect).layout(
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
    ));
    let x_clicked = IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .show(&mut x_ui, theme, &|ui, r, c| {
            icons::CLOSE.image(r.height(), c).paint_at(ui, r);
        })
        .on_hover_text(t("modifier_hint.hide_tooltip"))
        .clicked();
    if x_clicked {
        rt.dismissed = true;
        rt.working = None;
    }

    // hover 판정(입력 레이어). 렌더 rect 전체를 소비 zone 으로.
    result.hovered = ctx
        .pointer_hover_pos()
        .is_some_and(|p| render_rect.contains(p));

    // 페이드 중에만 매 프레임 재그리기(완전 표시 후엔 입력 구동 repaint 에 맡겨 유휴 CPU 절약).
    if alpha < 1.0 {
        ctx.request_repaint();
    }

    result
}

/// 저장된 pos/size(있으면) 또는 기본 anchor 로 base rect 를 만든다.
fn rect_from_settings(settings: &Settings, screen: egui::Rect, theme: &Theme) -> egui::Rect {
    let def = default_rect(screen, theme);
    let size = settings
        .modifier_hint
        .size
        .map(|(w, h)| egui::vec2(w.value(), h.value()))
        .unwrap_or_else(|| def.size());
    let min = settings
        .modifier_hint
        .pos
        .map(|(x, y)| egui::pos2(x.value(), y.value()))
        .unwrap_or(def.min);
    egui::Rect::from_min_size(min, size)
}

/// 셸(그림자 + 불투명 fill + 1px 보더)을 painter 로 그린다 — 고정 크기라 Frame(콘텐츠 맞춤)
/// 대신 painter 직접 사용. 색은 `alpha` 곱(페이드).
fn draw_shell(ui: &egui::Ui, theme: &Theme, rect: egui::Rect, alpha: f32) {
    let radius = theme.corner_radius.value();
    let bw = theme.border_width.value();
    let painter = ui.painter();
    let mut shadow = theme.shadow_popover().to_egui();
    shadow.color = shadow.color.gamma_multiply(alpha);
    painter.add(shadow.as_shape(rect, radius));
    painter.rect_filled(
        rect,
        radius,
        theme.modhint_bg().to_egui().gamma_multiply(alpha),
    );
    painter.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(bw, theme.modhint_border().to_egui().gamma_multiply(alpha)),
        egui::StrokeKind::Inside,
    );
}

/// 스트립("held" 조합) + 스크롤 리스트(섹션) 를 그린다. X 버튼은 호출측이 인터랙션과 함께 그린다.
fn draw_content(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    held: HeldModifier,
    sections: &[HintSection],
    alpha: f32,
) {
    let w = rect.width();
    let strip_h = theme.modhint_strip_height().value();
    let bw = theme.border_width.value();

    // 드래그 스트립 배경 + 하단 separator.
    let strip_rect = egui::Rect::from_min_size(rect.min, egui::vec2(w, strip_h));
    ui.painter().rect_filled(
        strip_rect,
        0.0,
        theme.modhint_strip_bg().to_egui().gamma_multiply(alpha),
    );
    ui.painter().hline(
        strip_rect.x_range(),
        strip_rect.bottom() - bw * 0.5,
        egui::Stroke::new(
            bw,
            theme.modhint_separator().to_egui().gamma_multiply(alpha),
        ),
    );

    // 스트립 내용: held 조합 Kbd + "held" 라벨 (좌). X 는 호출측.
    let pad_l = theme.modhint_pad().value();
    let strip_inner = egui::Rect::from_min_max(
        egui::pos2(strip_rect.left() + pad_l, strip_rect.top()),
        egui::pos2(strip_rect.right() - strip_h, strip_rect.bottom()),
    );
    let mut strip_ui = ui.new_child(egui::UiBuilder::new().max_rect(strip_inner));
    strip_ui.set_opacity(alpha);
    strip_ui.horizontal_centered(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        kbd(ui, theme, held_label(held));
        ui.label(
            egui::RichText::new(t("modifier_hint.held"))
                .size(theme.font_size_caption.value())
                .color(theme.modhint_held_fg().to_egui()),
        );
    });

    // 스크롤 리스트.
    let list_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), strip_rect.bottom()),
        egui::pos2(rect.right(), rect.bottom()),
    );
    let mut list_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(list_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    list_ui.set_opacity(alpha);
    let pad = theme.modhint_pad().value();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(&mut list_ui, |ui| {
            // 좌우/상하 패딩은 Frame inner_margin 으로(ScrollArea 뷰포트 내부 → 절대 rect
            // 수동 배치 대신 idiomatic flow). 세로 섹션 간격은 spacing.
            egui::Frame::new()
                .inner_margin(egui::Margin::same(pad as i8))
                .show(ui, |ui| {
                    ui.set_width((list_rect.width() - pad * 2.0).max(0.0));
                    ui.spacing_mut().item_spacing.y = theme.modhint_section_gap().value();
                    for (i, sec) in sections.iter().enumerate() {
                        if i > 0 {
                            ui.add_space(theme.modhint_section_gap().value());
                        }
                        draw_section(ui, theme, sec);
                    }
                });
        });
}

/// 한 조합 섹션 — ChordHead(Kbd + separator) + HintRow* + RoleRow*.
fn draw_section(ui: &mut egui::Ui, theme: &Theme, sec: &HintSection) {
    // ChordHead.
    kbd(ui, theme, &combo_keys(sec));
    ui.add_space(theme.modhint_row_gap().value());
    let w = ui.available_width();
    let (hr, _) = ui.allocate_exact_size(
        egui::vec2(w, theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        hr.x_range(),
        hr.center().y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.modhint_separator().to_egui(),
        ),
    );
    ui.add_space(theme.modhint_row_gap().value());

    ui.spacing_mut().item_spacing.y = theme.modhint_row_gap().value();
    for row in &sec.rows {
        draw_row(ui, theme, row);
    }
    for role in &sec.roles {
        draw_role_row(ui, theme, *role);
    }
}

/// 액션 행 — (plugin 이면 agent dot) + 라벨 + 우측 Kbd.
fn draw_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    row: &super::input::shortcuts::modifier_hint::HintRow,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        let (label, is_plugin) = row_label(&row.source);
        if is_plugin {
            let d = theme.status_dot_size().value();
            let (r, _) = ui.allocate_exact_size(egui::vec2(d, d), egui::Sense::hover());
            ui.painter()
                .circle_filled(r.center(), d * 0.5, theme.modhint_agent_dot().to_egui());
        }
        ui.label(
            egui::RichText::new(label)
                .size(theme.font_size_body.value())
                .color(theme.modhint_row_fg().to_egui()),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            kbd(ui, theme, &prettify_binding(&row.binding));
        });
    });
}

/// 특수 역할 행 — washed 배경 + leading 글리프(role-fg) + 설명.
fn draw_role_row(ui: &mut egui::Ui, theme: &Theme, role: HintRole) {
    egui::Frame::new()
        .fill(theme.modhint_role_bg().to_egui())
        .corner_radius(theme.corner_radius_sm.value())
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_sm.value() as i8,
            theme.modhint_row_gap().value() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                let gsz = theme.icon_glyph_size_xs.value();
                let (r, _) = ui.allocate_exact_size(egui::vec2(gsz, gsz), egui::Sense::hover());
                let col = theme.modhint_role_fg().to_egui();
                match role {
                    HintRole::MouseCaptureBypass => {
                        icons::MOUSE.image(gsz, col).paint_at(ui, r);
                    }
                    // 카테고리 전환 → folder 글리프(디자인 E, mhIc.folder).
                    HintRole::CategorySwitch => {
                        icons::FOLDER.image(gsz, col).paint_at(ui, r);
                    }
                    // tab/workspace/link 역할 → 숫자 오버레이 의미의 "#" 글리프.
                    HintRole::TabSwitch | HintRole::WorkspaceSwitch | HintRole::LinkClick => {
                        ui.painter().text(
                            r.center(),
                            egui::Align2::CENTER_CENTER,
                            "#",
                            egui::FontId::monospace(gsz),
                            col,
                        );
                    }
                }
                ui.label(
                    egui::RichText::new(t(role.desc_key()))
                        .size(theme.font_size_body.value())
                        .color(theme.modhint_row_fg().to_egui()),
                );
            });
        });
}

/// 홀드 anchor modifier 의 키캡 라벨(플랫폼 표기). `"alt"` 토큰은 macOS 에서 물리 Cmd.
fn held_label(held: HeldModifier) -> &'static str {
    match held {
        HeldModifier::Ctrl => "Ctrl",
        #[cfg(target_os = "macos")]
        HeldModifier::Alt => "Cmd",
        #[cfg(not(target_os = "macos"))]
        HeldModifier::Alt => "Alt",
        HeldModifier::Option => "Option",
        HeldModifier::Shift => "Shift",
    }
}

/// 섹션 조합 → `"Ctrl+Shift"` 형태 키캡 문자열(우선순위 순서, 플랫폼 표기).
fn combo_keys(sec: &HintSection) -> String {
    let c = &sec.combo;
    let mut parts: Vec<&str> = Vec::new();
    if c.ctrl {
        parts.push("Ctrl");
    }
    if c.alt {
        parts.push(if cfg!(target_os = "macos") {
            "Cmd"
        } else {
            "Alt"
        });
    }
    if c.option {
        parts.push("Option");
    }
    if c.shift {
        parts.push("Shift");
    }
    parts.join("+")
}

/// 바인딩 문자열(`"ctrl+shift+t"`) → 키캡 표기(`"Ctrl+Shift+T"`). 세그먼트별 첫 글자 대문자.
fn prettify_binding(binding: &str) -> String {
    binding
        .split('+')
        .map(|seg| {
            let mut chars = seg.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// 행 출처 → (표시 라벨, plugin 여부). 라벨 해석은 03 책임(모델은 키만 반환).
fn row_label(source: &HintRowSource) -> (String, bool) {
    match source {
        HintRowSource::Host { label_key } => (t(label_key).to_string(), false),
        // ScriptRegistry 이름 해석은 draw 경로에 미도달 → script_id 표기(후속 배선 대상).
        HintRowSource::Script { script_id } => (script_id.clone(), false),
        HintRowSource::Plugin {
            plugin_id,
            title_i18n_key,
        } => (format!("{plugin_id}: {}", t(title_i18n_key)), true),
    }
}

/// 주어진 rect 에 Foreground layer child Ui 를 만든다(오버레이는 다른 UI 위).
fn ui_at(ctx: &egui::Context, layer_id: egui::LayerId, rect: egui::Rect) -> egui::Ui {
    egui::Ui::new(
        ctx.clone(),
        egui::Id::new("modhint_overlay"),
        egui::UiBuilder::new().layer_id(layer_id).max_rect(rect),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const DELAY: f32 = 500.0;
    const FADE: f32 = 200.0;

    #[test]
    fn alpha_hidden_before_delay() {
        assert_eq!(hold_reveal_alpha(0.0, DELAY, FADE, false), None);
        assert_eq!(hold_reveal_alpha(499.0, DELAY, FADE, false), None);
    }

    #[test]
    fn alpha_starts_at_0_2_and_reaches_1_0() {
        // t=500 → 0.2, t=600 → 0.6, t=700 → 1.0, 이후 1.0 고정.
        assert_eq!(hold_reveal_alpha(500.0, DELAY, FADE, false), Some(0.2));
        let mid = hold_reveal_alpha(600.0, DELAY, FADE, false).unwrap();
        assert!((mid - 0.6).abs() < 1e-4, "t=600 alpha={mid}");
        assert_eq!(hold_reveal_alpha(700.0, DELAY, FADE, false), Some(1.0));
        assert_eq!(hold_reveal_alpha(1000.0, DELAY, FADE, false), Some(1.0));
    }

    #[test]
    fn alpha_reduced_motion_snaps_at_delay_boundary() {
        // 지연은 유지(499 → None), 게이트 통과 즉시 1.0(페이드 생략).
        assert_eq!(hold_reveal_alpha(499.0, DELAY, FADE, true), None);
        assert_eq!(hold_reveal_alpha(500.0, DELAY, FADE, true), Some(1.0));
        assert_eq!(hold_reveal_alpha(600.0, DELAY, FADE, true), Some(1.0));
    }

    #[test]
    fn update_hold_anchors_first_axis_and_keeps_timer() {
        let mut rt = ModifierHintRuntime::default();
        // Ctrl 누름 → anchor=Ctrl, 타이머 시작.
        assert!(rt.update_hold(true, false, false, false));
        assert_eq!(rt.held, Some(HeldModifier::Ctrl));
        let t0 = rt.hold_since;
        assert!(t0.is_some());
        // Ctrl 유지하며 Shift 추가 → anchor 유지(Ctrl still down), 타이머 리셋 안 함.
        rt.update_hold(true, false, false, true);
        assert_eq!(rt.held, Some(HeldModifier::Ctrl));
        assert_eq!(rt.hold_since, t0, "조합 변경 시 타이머 리셋 금지");
    }

    #[test]
    fn update_hold_reanchors_when_anchor_released() {
        let mut rt = ModifierHintRuntime::default();
        rt.update_hold(true, false, false, true); // Ctrl+Shift → anchor Ctrl
        assert_eq!(rt.held, Some(HeldModifier::Ctrl));
        let t0 = rt.hold_since;
        // Ctrl 뗌, Shift 유지 → anchor 를 Shift 로 재선정, 타이머 유지(여전히 홀드 중).
        rt.update_hold(false, false, false, true);
        assert_eq!(rt.held, Some(HeldModifier::Shift));
        assert_eq!(rt.hold_since, t0);
    }

    #[test]
    fn update_hold_clears_and_resets_dismiss_on_full_release() {
        let mut rt = ModifierHintRuntime::default();
        rt.update_hold(true, false, false, false);
        rt.dismissed = true;
        // 전부 뗌 → clear + dismissed 리셋.
        assert!(rt.update_hold(false, false, false, false));
        assert!(rt.held.is_none());
        assert!(rt.hold_since.is_none());
        assert!(!rt.dismissed, "전 modifier release 시 dismiss 리셋");
    }

    #[test]
    fn clamp_keeps_rect_inside_screen() {
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0));
        // 우하단 밖으로 나간 rect → 안쪽으로 이동, 크기 유지.
        let r = egui::Rect::from_min_size(egui::pos2(700.0, 500.0), egui::vec2(220.0, 400.0));
        let c = clamp_rect(r, screen);
        assert!(c.max.x <= screen.right() + 0.01);
        assert!(c.max.y <= screen.bottom() + 0.01);
        assert_eq!(c.size(), egui::vec2(220.0, 400.0));
    }

    #[test]
    fn clamp_shrinks_rect_bigger_than_screen() {
        let screen = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(150.0, 150.0));
        let r = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(220.0, 400.0));
        let c = clamp_rect(r, screen);
        assert_eq!(c.size(), egui::vec2(150.0, 150.0));
    }

    #[test]
    fn resize_clamps_to_min() {
        let r = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(220.0, 400.0));
        // 크게 줄이는 delta → min 200×240 로 클램프.
        let shrunk = resize_to(r, egui::vec2(-500.0, -500.0), 200.0, 240.0);
        assert_eq!(shrunk.size(), egui::vec2(200.0, 240.0));
        assert_eq!(shrunk.min, r.min, "좌상단 고정");
        // 늘리는 delta → 그대로 반영.
        let grown = resize_to(r, egui::vec2(30.0, 20.0), 200.0, 240.0);
        assert_eq!(grown.size(), egui::vec2(250.0, 420.0));
    }

    // 빈 섹션 억제·정렬은 modifier-hint-02(build_hint_sections)에서 이미 테스트됨. 여기서는
    // 03 이 그 결과를 그대로 소비함만 확인(계약 회귀 방지).
    #[test]
    fn consumes_build_hint_sections() {
        use tasty_settings::KeybindingSettings;
        let kb = KeybindingSettings::preset_tasty();
        let sections: Vec<HintSection> =
            build_hint_sections(HeldModifier::Ctrl, &kb, "ctrl", false, &[]);
        assert!(!sections.is_empty());
    }
}
