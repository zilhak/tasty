//! 사용자 행동의 결과를 잠시 보여 주는 토스트. 호출부는 에이전트 동작에 토스트를 만들지 않아야 한다.
//! 입력·포커스를 받지 않으며 관리자는 중복 합치기·만료·범위·개수 제한을 처리한다.
//! 렌더링은 갤러리와 같은 공용 위젯을 사용한다. docs/design/systems/toast.md 참고.

use std::time::{Duration, Instant};

use crate::theme;

use super::layout_context::LayoutContext;

pub use crate::model::toast_kind::{ToastKind, ToastScope};

/// 단일 토스트 인스턴스 상태.
#[derive(Debug, Clone)]
pub struct ToastState {
    pub id: u64,
    pub message: String,
    pub kind: ToastKind,
    /// 알림을 낸 동작의 단축키 키캡. 메뉴·마우스로 실행했고 binding이 있을 때만 채운다.
    pub hint: Vec<String>,
    pub scope: ToastScope,
    pub spawned_at: Instant,
    pub lifetime: Duration,
}

const DEFAULT_LIFETIME: Duration = Duration::from_millis(2000);
/// 같은 범위·문구가 이 시간 안에 다시 오면 수명만 갱신한다.
const COALESCE_WINDOW: Duration = Duration::from_millis(500);
/// 스코프당 최대 동시 표시 개수.
const MAX_PER_SCOPE: usize = 5;
/// 문자 수가 이 상한을 넘으면 앞부분과 안내 접미만 표시한다.
/// 번역문을 만들 때와 같은 TOAST_MAX_CHARS를 사용한다.
const MAX_MESSAGE_CHARS: usize = crate::i18n::TOAST_MAX_CHARS;
use tasty_ui_widgets::{TOAST_FADE_OUT_MS as FADE_OUT_MS, toast_fade_alpha};
pub use tasty_ui_widgets::{ToastEntryView, ToastScopeView, ToastStackBottom, ToastViewProps};

/// 공용 위젯에 Tooltip 레이어 painter를 전달해 다른 UI 위에 토스트를 그린다.
/// 그린 카드의 egui 사각형을 돌려준다.
pub fn draw_toast_view(ctx: &egui::Context, props: &ToastViewProps<'_>) -> Vec<egui::Rect> {
    if props.scopes.is_empty() {
        return Vec::new();
    }

    let layer_id = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("toast_layer"));
    let painter = ctx.layer_painter(layer_id);
    tasty_ui_widgets::draw_toast_scopes(&painter, props)
}

/// 토스트 hint에 넣을 `binding_id` 동작의 첫 단축키 키캡. binding이 비었으면 빈 목록이다.
/// 메뉴·마우스로 실행한 동작에만 쓴다. 키를 눌러 실행했다면 사용자가 이미 키를 안다.
pub fn binding_hint(settings: &tasty_settings::Settings, binding_id: &str) -> Vec<String> {
    settings
        .keybindings
        .get_bindings(binding_id)
        .and_then(|b| b.first())
        .map(|b| tasty_settings::KeybindingSettings::format_display_parts(b, &settings.general))
        .unwrap_or_default()
}

pub struct ToastManager {
    toasts: Vec<ToastState>,
    next_id: u64,
    /// 새 토스트/coalesce 갱신에 부여할 수명. `Settings.overlay.toast_duration_ms`
    /// 에서 매 프레임 동기화된다(설정 미로드 시 [`DEFAULT_LIFETIME`]).
    lifetime: Duration,
    /// 마지막 draw에서 그린 카드 영역(egui 논리 좌표). 토스트가 없으면 비어 있다.
    /// WebView 동기화가 이 영역과 겹치는 native 화면만 숨긴다.
    card_rects: Vec<crate::model::LogicalRect>,
}

impl ToastManager {
    pub fn new() -> Self {
        Self {
            toasts: Vec::new(),
            next_id: 1,
            lifetime: DEFAULT_LIFETIME,
            card_rects: Vec::new(),
        }
    }

    /// 마지막 draw에서 그린 카드 영역. 카드가 떠 있는 동안만 비어 있지 않다.
    pub fn card_rects(&self) -> &[crate::model::LogicalRect] {
        &self.card_rects
    }

    /// 토스트 수명을 설정값(ms)으로 동기화한다. 이미 떠 있는 토스트에는 소급하지
    /// 않고, 이후 push/coalesce 되는 토스트부터 적용된다. draw 직전에 호출한다.
    pub fn set_lifetime_ms(&mut self, ms: u64) {
        self.lifetime = Duration::from_millis(ms);
    }

    /// 사용자 행동의 결과로 토스트를 추가한다.
    pub fn push(&mut self, message: impl Into<String>, kind: ToastKind, scope: ToastScope) {
        self.push_with_hint(message, kind, Vec::new(), scope);
    }

    /// `hint`(동작의 단축키 키캡)를 붙여 토스트를 추가한다. 키보드로 실행한 동작이나
    /// binding이 빈 동작에는 빈 목록을 넘긴다. docs/design/systems/toast.md 참고.
    pub fn push_with_hint(
        &mut self,
        message: impl Into<String>,
        kind: ToastKind,
        hint: Vec<String>,
        scope: ToastScope,
    ) {
        let message = truncate_message(message.into());
        let now = Instant::now();

        if let Some(existing) = self.toasts.iter_mut().rev().find(|t| {
            t.scope == scope
                && t.message == message
                && now.duration_since(t.spawned_at) < COALESCE_WINDOW
        }) {
            existing.spawned_at = now;
            existing.kind = kind;
            existing.hint = hint;
            existing.lifetime = self.lifetime;
            return;
        }

        let id = self.next_id;
        self.next_id += 1;

        self.toasts.push(ToastState {
            id,
            message,
            kind,
            hint,
            scope: scope.clone(),
            spawned_at: now,
            lifetime: self.lifetime,
        });

        let count_in_scope = self.toasts.iter().filter(|t| t.scope == scope).count();
        if count_in_scope > MAX_PER_SCOPE
            && let Some(idx) = self.toasts.iter().position(|t| t.scope == scope)
        {
            self.toasts.remove(idx);
        }
    }

    /// 테스트에서 현재 토스트 수를 확인한다.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.toasts.len()
    }

    /// 테스트에서 생성 순서의 문구를 확인한다.
    #[cfg(test)]
    pub(crate) fn messages(&self) -> Vec<&str> {
        self.toasts.iter().map(|t| t.message.as_str()).collect()
    }

    /// 편의 헬퍼: Info 토스트.
    pub fn push_info(&mut self, message: impl Into<String>, scope: ToastScope) {
        self.push(message, ToastKind::Info, scope);
    }

    /// 만료·표시 범위를 정리하고 공용 위젯으로 그린다. 모션 감소이면 페이드를 생략한다.
    pub fn draw(&mut self, ctx: &egui::Context, draw_ctx: &LayoutContext, reduced_motion: bool) {
        let now = Instant::now();

        self.toasts.retain(|t| {
            let age = now.duration_since(t.spawned_at);
            age < t.lifetime + Duration::from_millis(FADE_OUT_MS as u64)
        });

        self.toasts
            .retain(|t| Self::scope_rect(&t.scope, draw_ctx, ctx).is_some());

        if self.toasts.is_empty() {
            self.card_rects.clear();
            return;
        }

        // 살아있는 동안에는 매 프레임 다시 그려야 페이드가 보인다.
        ctx.request_repaint();

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
                    hint: t.hint.clone(),
                    alpha: compute_alpha(t, now, reduced_motion),
                })
                .collect();
            scopes.push(ToastScopeView {
                scope_rect,
                bottom: stack_bottom(scope),
                entries,
            });
        }

        let th = theme::theme();
        let props = ToastViewProps {
            theme: &th,
            scopes: &scopes,
        };
        self.card_rects = draw_toast_view(ctx, &props)
            .into_iter()
            .map(|r| crate::model::LogicalRect {
                x: crate::model::LogicalPx(r.min.x),
                y: crate::model::LogicalPx(r.min.y),
                width: crate::model::LogicalPx(r.width()),
                height: crate::model::LogicalPx(r.height()),
            })
            .collect();
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

/// 스택 맨 아래 카드의 기준. 창 전체 rect에 그리는 Window·Workspace는 상태바를 피해
/// 창 하단의 스택 거리 위에 쌓고, 화면 일부를 덮는 Pane·Surface는 가장자리 여백만 둔다.
fn stack_bottom(scope: &ToastScope) -> ToastStackBottom {
    match scope {
        ToastScope::Window | ToastScope::Workspace(_) => ToastStackBottom::Window,
        ToastScope::Pane(_) | ToastScope::Surface(_) => ToastStackBottom::ScopeMargin,
    }
}

/// 상태의 시간 값을 공용 위젯의 fade_alpha에 전달한다.
pub fn compute_alpha(t: &ToastState, now: Instant, reduced_motion: bool) -> f32 {
    toast_fade_alpha(now.duration_since(t.spawned_at), t.lifetime, reduced_motion)
}

/// 문자 경계를 지켜 본문을 상한까지 줄이고, 상한 밖에 번역한 안내 접미를 붙인다.
/// 상한과 같으면 자르지 않는다. 중복 비교 전에 적용해 같은 긴 문구도 합칠 수 있게 한다.
fn truncate_message(message: String) -> String {
    if message.chars().count() <= MAX_MESSAGE_CHARS {
        return message;
    }
    let truncated: String = message.chars().take(MAX_MESSAGE_CHARS).collect();
    let notice = crate::i18n::t_fmt("toast.char_limit_notice", &MAX_MESSAGE_CHARS.to_string());
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
    use crate::theme::Theme;

    fn test_theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    #[test]
    fn truncate_over_limit_keeps_200_chars_and_appends_notice() {
        let out = truncate_message("あ".repeat(250));
        let first_line = out.lines().next().unwrap();
        assert_eq!(first_line.chars().count(), MAX_MESSAGE_CHARS);
        assert!(out.contains('\n'));
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
            hint: Vec::new(),
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
    fn window_wide_scopes_stack_above_the_window_offset_and_regions_keep_the_margin() {
        assert_eq!(stack_bottom(&ToastScope::Window), ToastStackBottom::Window);
        assert_eq!(
            stack_bottom(&ToastScope::Workspace(0)),
            ToastStackBottom::Window
        );
        assert_eq!(
            stack_bottom(&ToastScope::Pane(1)),
            ToastStackBottom::ScopeMargin
        );
        assert_eq!(
            stack_bottom(&ToastScope::Surface(1)),
            ToastStackBottom::ScopeMargin
        );
    }

    #[test]
    fn view_empty_scopes_is_noop() {
        run_view(vec![]);
    }

    #[test]
    fn view_with_entries_does_not_panic() {
        let scopes = vec![ToastScopeView {
            scope_rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)),
            bottom: ToastStackBottom::ScopeMargin,
            entries: vec![
                ToastEntryView {
                    kind: ToastKind::Info,
                    message: "info".into(),
                    hint: Vec::new(),
                    alpha: 1.0,
                },
                ToastEntryView {
                    kind: ToastKind::Error,
                    message: "long error message that may wrap into multiple lines".into(),
                    hint: Vec::new(),
                    alpha: 0.5,
                },
            ],
        }];
        run_view(scopes);
    }

    #[test]
    fn view_skips_zero_alpha_entries() {
        let scopes = vec![ToastScopeView {
            scope_rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(800.0, 600.0)),
            bottom: ToastStackBottom::ScopeMargin,
            entries: vec![ToastEntryView {
                kind: ToastKind::Warning,
                message: "invisible".into(),
                hint: Vec::new(),
                alpha: 0.0,
            }],
        }];
        run_view(scopes);
    }
}
