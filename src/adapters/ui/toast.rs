//! 휘발성 인앱 알림(Toast) 시스템.
//!
//! 설계 문서: `docs/design/systems/toast.md`.
//!
//! - **사용자 행동에서만 발사**된다. CLI/IPC를 통한 에이전트 동작은 토스트를 만들지 않는다.
//! - 포커스를 받지 않으며 입력 이벤트를 소비하지 않는다 (마우스가 그대로 통과).
//! - 자동 소멸한다 (기본 2초).
//! - `LayoutContext`를 받아 스코프별 rect를 얻는다.
//!
//! ## Split: wrapper / view
//!
//! [`ToastManager`] 가 *상태 관리* (push / coalesce / 만료 정리 / fade alpha 계산 /
//! scope rect lookup) 를 담당하고, 순수 시각 [`draw_toast_view`] 가 미리 계산된
//! [`ToastEntryView`] (alpha 포함) 와 scope rect 만 받아 그린다. AppState/CoreState
//! 의존 없는 시각이라 gallery (`tasty-gallery`) 에서 mock props 로 검증 가능.

use std::time::{Duration, Instant};

use crate::theme;
use crate::theme::Theme;

use super::layout_context::LayoutContext;

pub use crate::model::toast_kind::{ToastKind, ToastScope};

/// 단일 토스트 인스턴스 상태.
#[derive(Debug, Clone)]
pub struct ToastState {
    pub id: u64,
    pub message: String,
    pub kind: ToastKind,
    pub scope: ToastScope,
    pub spawned_at: Instant,
    pub lifetime: Duration,
}

const DEFAULT_LIFETIME: Duration = Duration::from_millis(2000);
/// 같은 스코프·같은 메시지가 이 시간 내에 다시 발사되면 새 토스트를 만들지 않고
/// 기존 토스트의 수명만 갱신한다.
const COALESCE_WINDOW: Duration = Duration::from_millis(500);
/// 등장/소멸 페이드 시간.
const FADE_IN_MS: f32 = 80.0;
const FADE_OUT_MS: f32 = 160.0;
/// 스코프당 최대 동시 표시 개수.
const MAX_PER_SCOPE: usize = 5;
/// 토스트 본문 최대 문자 수(유니코드 char 기준, 바이트 아님). 초과 시 앞
/// `MAX_MESSAGE_CHARS` 자만 남기고 줄바꿈 + 안내 접미를 붙인다 — 비정상적으로
/// 긴 입력(경로/에러/plugin 텍스트)이 토스트를 세로로 폭주시키는 것을 막는다.
const MAX_MESSAGE_CHARS: usize = 200;
// 카드 구조 치수는 `tasty-ui-widgets::tokens` 가 단일 출처다 — 갤러리 specimen 이
// 같은 상수를 읽는다. 여기서 다시 정의하면 값이 갈릴 수 있는 구조가 되살아난다.
use tasty_ui_widgets::tokens::{
    TOAST_ACCENT_BAR_WIDTH as ACCENT_BAR_WIDTH, TOAST_GAP,
    TOAST_MIN_INNER_WIDTH as MIN_TOAST_INNER_WIDTH, TOAST_MIN_MAX_WIDTH,
    TOAST_PADDING_X as PADDING_X, TOAST_PADDING_Y as PADDING_Y, TOAST_SCOPE_MARGIN as SCOPE_MARGIN,
};

/// View 입력 — 그릴 준비가 끝난 토스트 1 개의 시각 데이터.
///
/// `alpha` 는 매니저가 lifetime/fade 로부터 미리 계산. view 는 시간 의존이 없다.
#[derive(Clone, Debug)]
pub struct ToastEntryView {
    pub kind: ToastKind,
    pub message: String,
    /// [0.0, 1.0] — 0 이면 view 가 스킵.
    pub alpha: f32,
}

/// View 입력 — 한 scope 의 토스트 그룹.
///
/// `entries` 는 *발사 순서* (id 오름차순) 로 정렬돼 있어야 한다 — view 는 그대로
/// 우측 하단부터 위로 쌓는다.
#[derive(Clone, Debug)]
pub struct ToastScopeView {
    pub scope_rect: egui::Rect,
    pub entries: Vec<ToastEntryView>,
}

/// View 입력 — 전체 scope 의 그룹 리스트 + theme.
pub struct ToastViewProps<'a> {
    pub theme: &'a Theme,
    pub scopes: &'a [ToastScopeView],
}

/// 순수 시각 view. AppState/CoreState/`theme::theme()` 비의존.
///
/// `ctx` 를 받는 이유는 토스트가 다른 UI 위에 떠야 하므로
/// `LayerId(Order::Tooltip, ...)` 의 layer painter 를 사용하기 때문.
/// 반환값 없음 — 토스트는 사용자 입력을 받지 않으며 (auto-dismiss) action 도 없다.
pub fn draw_toast_view(ctx: &egui::Context, props: &ToastViewProps<'_>) {
    if props.scopes.is_empty() {
        return;
    }

    let th = props.theme;
    let layer_id = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("toast_layer"));
    let painter = ctx.layer_painter(layer_id);

    for scope in props.scopes {
        let scope_rect = scope.scope_rect;
        // 스코프 경계로 클립 — 폭/세로 클램프 후에도 1px 단위로 새는 것을 막는
        // 안전망. 토스트는 자기 스코프 영역 안에 머물러야 한다(이웃 pane/탭바를
        // 덮지 않음).
        let painter = painter.with_clip_rect(scope_rect);
        let mut cursor_y = scope_rect.max.y - SCOPE_MARGIN;

        // 새것부터 그리며 위로 올라간다 (id 오름차순으로 받았으므로 reverse).
        for entry in scope.entries.iter().rev() {
            let alpha = entry.alpha;
            if alpha <= 0.0 {
                continue;
            }

            let body_text = entry.message.as_str();
            // 좁은 surface 에서 토스트가 좌측 경계를 넘지 않도록 max_width 를 surface
            // 안쪽 폭(width - 2*margin)으로 클램프한다. 정상 폭 surface 에서는
            // 0.8*width < width-2*margin (width>120) 이라 0.8 폭이 그대로 — 시각
            // 무변경이고, 좁은 surface 에서만 클램프가 발동한다.
            let inner_limit = (scope_rect.width() - SCOPE_MARGIN * 2.0).max(MIN_TOAST_INNER_WIDTH);
            let max_width = (scope_rect.width() * 0.8)
                .max(TOAST_MIN_MAX_WIDTH)
                .min(inner_limit);
            let font = egui::FontId::proportional(th.font_size_body.value());
            // wrap_width 음수 방지(클램프로 max_width 가 작아질 때).
            let wrap_width = (max_width - PADDING_X * 2.0 - ACCENT_BAR_WIDTH).max(1.0);

            let galley = ctx.fonts(|f| {
                f.layout(
                    body_text.to_string(),
                    font.clone(),
                    th.text_primary().into(),
                    wrap_width,
                )
            });

            let toast_w = (galley.size().x + PADDING_X * 2.0 + ACCENT_BAR_WIDTH).min(max_width);
            let toast_h = galley.size().y + PADDING_Y * 2.0;

            let max_x = scope_rect.max.x - SCOPE_MARGIN;
            let bottom_y = cursor_y;
            let top_y = bottom_y - toast_h;
            // 스택이 scope 상단을 넘으면 더 오래된(위쪽) 토스트는 그리지 않는다.
            // 새것부터 그리므로(reverse) break 가 곧 "넘치는 옛것 생략". 단일
            // 토스트가 scope 보다 높은 극단은 위의 clip 이 처리한다.
            if top_y < scope_rect.min.y {
                break;
            }
            let left_x = max_x - toast_w;

            let rect =
                egui::Rect::from_min_max(egui::pos2(left_x, top_y), egui::pos2(max_x, bottom_y));

            let bg = th.surface_raised().gamma_multiply(alpha);
            // divergence: toast_border()=surface0, 코드값(surface1) 보존
            let border = th.border_strong().gamma_multiply(alpha);
            let accent = accent_color(entry.kind, th).gamma_multiply(alpha);

            painter.rect_filled(rect, th.corner_radius.value(), bg);
            painter.rect_stroke(
                rect,
                th.corner_radius.value(),
                egui::Stroke::new(th.border_width.value(), border),
                egui::StrokeKind::Inside,
            );

            let bar_rect = egui::Rect::from_min_max(
                rect.min,
                egui::pos2(rect.min.x + ACCENT_BAR_WIDTH, rect.max.y),
            );
            let bar_radius = egui::CornerRadius {
                nw: th.corner_radius.value() as u8,
                sw: th.corner_radius.value() as u8,
                ne: 0,
                se: 0,
            };
            painter.rect_filled(bar_rect, bar_radius, accent);

            let text_pos = egui::pos2(
                rect.min.x + ACCENT_BAR_WIDTH + PADDING_X,
                rect.min.y + PADDING_Y,
            );
            painter.galley(
                text_pos,
                galley,
                th.text_primary().gamma_multiply(alpha).into(),
            );

            cursor_y = top_y - TOAST_GAP;
        }
    }
}

fn accent_color(kind: ToastKind, th: &Theme) -> egui::Color32 {
    match kind {
        ToastKind::Info => th.accent_primary().into(),
        ToastKind::Success => th.accent_success().into(),
        ToastKind::Warning => th.accent_warning().into(),
        ToastKind::Error => th.accent_danger().into(),
    }
}

pub struct ToastManager {
    toasts: Vec<ToastState>,
    next_id: u64,
    /// 새 토스트/coalesce 갱신에 부여할 수명. `Settings.overlay.toast_duration_ms`
    /// 에서 매 프레임 동기화된다(설정 미로드 시 [`DEFAULT_LIFETIME`]).
    lifetime: Duration,
}

impl ToastManager {
    pub fn new() -> Self {
        Self {
            toasts: Vec::new(),
            next_id: 1,
            lifetime: DEFAULT_LIFETIME,
        }
    }

    /// 토스트 수명을 설정값(ms)으로 동기화한다. 이미 떠 있는 토스트에는 소급하지
    /// 않고, 이후 push/coalesce 되는 토스트부터 적용된다. draw 직전에 호출한다.
    pub fn set_lifetime_ms(&mut self, ms: u64) {
        self.lifetime = Duration::from_millis(ms);
    }

    /// 토스트 발사. 사용자 행동에서만 호출되어야 한다.
    pub fn push(&mut self, message: impl Into<String>, kind: ToastKind, scope: ToastScope) {
        let message = truncate_message(message.into());
        let now = Instant::now();

        // Coalesce: 같은 스코프·같은 메시지가 짧은 시간 내에 또 오면 수명만 갱신한다.
        if let Some(existing) = self.toasts.iter_mut().rev().find(|t| {
            t.scope == scope
                && t.message == message
                && now.duration_since(t.spawned_at) < COALESCE_WINDOW
        }) {
            existing.spawned_at = now;
            existing.kind = kind;
            existing.lifetime = self.lifetime;
            return;
        }

        let id = self.next_id;
        self.next_id += 1;

        self.toasts.push(ToastState {
            id,
            message,
            kind,
            scope: scope.clone(),
            spawned_at: now,
            lifetime: self.lifetime,
        });

        // 스코프당 최대 개수 초과 시 가장 오래된 것 제거.
        let count_in_scope = self.toasts.iter().filter(|t| t.scope == scope).count();
        if count_in_scope > MAX_PER_SCOPE
            && let Some(idx) = self.toasts.iter().position(|t| t.scope == scope)
        {
            self.toasts.remove(idx);
        }
    }

    /// 지금 떠 있는 토스트 수. **테스트 전용** — 부수효과 게이트(예: 창 없는 parked
    /// engine 에는 토스트를 쌓지 않는다)를 단언하려면 개수를 볼 수 있어야 한다.
    /// 프로덕션 표면을 넓히지 않으려고 `cfg(test)` 로 묶는다.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.toasts.len()
    }

    /// 편의 헬퍼: Info 토스트.
    pub fn push_info(&mut self, message: impl Into<String>, scope: ToastScope) {
        self.push(message, ToastKind::Info, scope);
    }

    /// 매 프레임 호출. 만료된 토스트를 제거하고 살아있는 토스트를 그린다.
    /// `draw_ctx`는 PopupManager가 만든 것을 그대로 공유한다.
    ///
    /// `reduced_motion`이 true면 페이드 인/아웃을 0ms로 처리 (시각 자극 최소화).
    ///
    /// Wrapper: 상태 정리 + alpha 계산 + scope rect lookup → 순수
    /// [`draw_toast_view`] 로 위임.
    pub fn draw(&mut self, ctx: &egui::Context, draw_ctx: &LayoutContext, reduced_motion: bool) {
        let now = Instant::now();

        // 1) 만료된 토스트 제거.
        self.toasts.retain(|t| {
            let age = now.duration_since(t.spawned_at);
            age < t.lifetime + Duration::from_millis(FADE_OUT_MS as u64)
        });

        // 2) 스코프가 화면에서 사라진 토스트 제거.
        self.toasts
            .retain(|t| Self::scope_rect(&t.scope, draw_ctx, ctx).is_some());

        if self.toasts.is_empty() {
            return;
        }

        // 살아있는 동안에는 매 프레임 다시 그려야 페이드가 보인다.
        ctx.request_repaint();

        // 3) 스코프별로 그루핑하여 view 입력으로 변환.
        let mut by_scope: std::collections::HashMap<String, Vec<&ToastState>> =
            std::collections::HashMap::new();
        for t in &self.toasts {
            by_scope
                .entry(format!("{:?}", t.scope))
                .or_default()
                .push(t);
        }

        let mut scopes: Vec<ToastScopeView> = Vec::with_capacity(by_scope.len());
        for (_, mut group) in by_scope {
            group.sort_by_key(|t| t.id);
            let scope = &group[0].scope;
            let Some(scope_rect) = Self::scope_rect(scope, draw_ctx, ctx) else {
                continue;
            };
            let entries: Vec<ToastEntryView> = group
                .iter()
                .map(|t| ToastEntryView {
                    kind: t.kind,
                    message: t.message.clone(),
                    alpha: compute_alpha(t, now, reduced_motion),
                })
                .collect();
            scopes.push(ToastScopeView {
                scope_rect,
                entries,
            });
        }

        let th = theme::theme();
        let props = ToastViewProps {
            theme: &th,
            scopes: &scopes,
        };
        draw_toast_view(ctx, &props);
    }

    /// 스코프의 rect를 얻는다. Window/Workspace는 screen rect를 사용한다.
    fn scope_rect(
        scope: &ToastScope,
        draw_ctx: &LayoutContext,
        ctx: &egui::Context,
    ) -> Option<egui::Rect> {
        match scope {
            ToastScope::Window => Some(ctx.screen_rect()),
            ToastScope::Workspace(ws_idx) => {
                if *ws_idx == draw_ctx.active_workspace {
                    Some(ctx.screen_rect())
                } else {
                    None
                }
            }
            ToastScope::Pane(pane_id) => draw_ctx
                .pane_rects
                .iter()
                .find(|(id, _)| id == pane_id)
                .map(|(_, r)| *r),
            ToastScope::Surface(surface_id) => draw_ctx
                .surface_rects
                .iter()
                .find(|(id, _)| id == surface_id)
                .map(|(_, r)| *r),
        }
    }
}

/// 페이드 인/아웃 알파 계산. pub — 테스트 + wrapper 가 view 입력 변환 시 호출.
pub fn compute_alpha(t: &ToastState, now: Instant, reduced_motion: bool) -> f32 {
    let age_ms = now.duration_since(t.spawned_at).as_secs_f32() * 1000.0;
    let life_ms = t.lifetime.as_secs_f32() * 1000.0;

    if reduced_motion {
        return if age_ms < life_ms { 1.0 } else { 0.0 };
    }

    if age_ms < FADE_IN_MS {
        (age_ms / FADE_IN_MS).clamp(0.0, 1.0)
    } else if age_ms < life_ms {
        1.0
    } else {
        let fade_out = (age_ms - life_ms) / FADE_OUT_MS;
        (1.0 - fade_out).clamp(0.0, 1.0)
    }
}

/// 본문이 `MAX_MESSAGE_CHARS`(유니코드 char) 를 초과하면 앞부분만 남기고 줄바꿈
/// + 안내 접미(`toast.char_limit_notice`)를 붙인다.
///
/// - 길이는 `chars().count()`(문자 수), 자르기는 `chars().take(..)`(char 경계)로
///   처리해 멀티바이트(한글/일문 등)에서 바이트 슬라이싱 panic 을 피한다.
/// - 경계 정책: 원본이 `MAX_MESSAGE_CHARS` 를 *초과* 할 때만 자른다(정확히 같거나
///   이하는 변경 없음). 접미는 본문 200자 *바깥* 에 추가로 붙는다.
/// - coalesce 비교 이전(push 진입부)에 적용되므로 같은 긴 메시지는 동일하게
///   잘려 정상 coalesce 된다.
fn truncate_message(message: String) -> String {
    if message.chars().count() <= MAX_MESSAGE_CHARS {
        return message;
    }
    let truncated: String = message.chars().take(MAX_MESSAGE_CHARS).collect();
    let notice = crate::i18n::t("toast.char_limit_notice");
    format!("{truncated}\n{notice}")
}

impl Default for ToastManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    #[test]
    fn truncate_over_limit_keeps_200_chars_and_appends_notice() {
        // 멀티바이트 250자 → 첫 줄 200 char + 줄바꿈 + 접미.
        let out = truncate_message("あ".repeat(250));
        let first_line = out.lines().next().unwrap();
        assert_eq!(first_line.chars().count(), MAX_MESSAGE_CHARS);
        assert!(out.contains('\n'));
        // 접미 줄(번역값/키)이 존재한다(로케일 의존이라 내용은 단정하지 않음).
        assert!(!out.lines().last().unwrap().is_empty());
    }

    #[test]
    fn truncate_at_or_under_limit_unchanged() {
        let exact = "x".repeat(MAX_MESSAGE_CHARS);
        assert_eq!(truncate_message(exact.clone()), exact);
        let under = "y".repeat(10);
        assert_eq!(truncate_message(under.clone()), under);
    }

    #[test]
    fn truncate_no_panic_on_multibyte_boundary() {
        // 한글(자당 3바이트) 300자 — 바이트 슬라이싱이면 panic. char 경계라 안전.
        let out = truncate_message("한".repeat(300));
        assert_eq!(
            out.lines().next().unwrap().chars().count(),
            MAX_MESSAGE_CHARS
        );
    }

    fn mk_state(id: u64, kind: ToastKind, msg: &str) -> ToastState {
        ToastState {
            id,
            message: msg.to_string(),
            kind,
            scope: ToastScope::Window,
            spawned_at: Instant::now(),
            lifetime: DEFAULT_LIFETIME,
        }
    }

    #[test]
    fn compute_alpha_full_during_lifetime() {
        let t = mk_state(1, ToastKind::Info, "hello");
        // 100ms past spawn (FADE_IN_MS = 80) — full opacity
        let now = t.spawned_at + Duration::from_millis(100);
        let a = compute_alpha(&t, now, false);
        assert!((a - 1.0).abs() < 1e-3);
    }

    #[test]
    fn compute_alpha_fade_in() {
        let t = mk_state(1, ToastKind::Info, "hi");
        // 40ms past spawn → 40/80 = 0.5
        let now = t.spawned_at + Duration::from_millis(40);
        let a = compute_alpha(&t, now, false);
        assert!((a - 0.5).abs() < 1e-3);
    }

    #[test]
    fn compute_alpha_fade_out() {
        let t = mk_state(1, ToastKind::Info, "hi");
        // 2080ms past spawn → past lifetime, mid fade-out (80/160 = 0.5 done)
        let now = t.spawned_at + Duration::from_millis(2080);
        let a = compute_alpha(&t, now, false);
        assert!((a - 0.5).abs() < 1e-3);
    }

    #[test]
    fn compute_alpha_reduced_motion_no_fade() {
        let t = mk_state(1, ToastKind::Info, "hi");
        // 40ms past spawn — would be 0.5 with fade, but reduced_motion → 1.0
        let now_in = t.spawned_at + Duration::from_millis(40);
        assert!((compute_alpha(&t, now_in, true) - 1.0).abs() < 1e-3);
        // past lifetime → 0.0
        let now_out = t.spawned_at + Duration::from_millis(3000);
        assert_eq!(compute_alpha(&t, now_out, true), 0.0);
    }

    #[test]
    fn manager_push_assigns_unique_ids() {
        let mut mgr = ToastManager::new();
        mgr.push("a", ToastKind::Info, ToastScope::Window);
        mgr.push("b", ToastKind::Info, ToastScope::Window);
        assert_eq!(mgr.toasts.len(), 2);
        assert_ne!(mgr.toasts[0].id, mgr.toasts[1].id);
    }

    #[test]
    fn manager_coalesce_same_message_same_scope() {
        let mut mgr = ToastManager::new();
        mgr.push("dup", ToastKind::Info, ToastScope::Window);
        mgr.push("dup", ToastKind::Warning, ToastScope::Window);
        assert_eq!(mgr.toasts.len(), 1);
        // kind 갱신됨
        assert_eq!(mgr.toasts[0].kind, ToastKind::Warning);
    }

    #[test]
    fn manager_max_per_scope_evicts_oldest() {
        let mut mgr = ToastManager::new();
        for i in 0..(MAX_PER_SCOPE + 2) {
            mgr.push(format!("m-{i}"), ToastKind::Info, ToastScope::Window);
        }
        assert_eq!(mgr.toasts.len(), MAX_PER_SCOPE);
        assert!(mgr.toasts.iter().all(|t| t.message != "m-0"));
        assert!(mgr.toasts.iter().all(|t| t.message != "m-1"));
    }

    fn run_view(scopes: Vec<ToastScopeView>) {
        let ctx = egui::Context::default();
        let theme = test_theme();
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            let props = ToastViewProps {
                theme: &theme,
                scopes: &scopes,
            };
            draw_toast_view(ctx, &props);
        }));
    }

    #[test]
    fn view_empty_scopes_is_noop() {
        run_view(vec![]);
    }

    #[test]
    fn view_with_entries_does_not_panic() {
        let scopes = vec![ToastScopeView {
            scope_rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)),
            entries: vec![
                ToastEntryView {
                    kind: ToastKind::Info,
                    message: "info".into(),
                    alpha: 1.0,
                },
                ToastEntryView {
                    kind: ToastKind::Error,
                    message: "long error message that may wrap into multiple lines".into(),
                    alpha: 0.5,
                },
            ],
        }];
        run_view(scopes);
    }

    #[test]
    fn view_skips_zero_alpha_entries() {
        // alpha=0 인 entry 만 있어도 panic 없이 통과.
        let scopes = vec![ToastScopeView {
            scope_rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)),
            entries: vec![ToastEntryView {
                kind: ToastKind::Warning,
                message: "invisible".into(),
                alpha: 0.0,
            }],
        }];
        run_view(scopes);
    }
}
