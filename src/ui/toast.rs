//! 휘발성 인앱 알림(Toast) 시스템.
//!
//! 설계 문서: `docs/design/toast-system.md`.
//!
//! - **사용자 행동에서만 발사**된다. CLI/IPC를 통한 에이전트 동작은 토스트를 만들지 않는다.
//! - 포커스를 받지 않으며 입력 이벤트를 소비하지 않는다 (마우스가 그대로 통과).
//! - 자동 소멸한다 (기본 2초).
//! - `LayoutContext`를 받아 스코프별 rect를 얻는다.

use std::time::{Duration, Instant};

use crate::theme;

use super::layout_context::LayoutContext;

/// Toast의 종류. 좌측 컬러 바 색을 결정한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

/// 어느 영역에 떠오를지를 결정하는 위치 앵커.
#[derive(Debug, Clone, PartialEq)]
pub enum ToastScope {
    Window,
    Workspace(usize),
    Pane(u32),
    Surface(u32),
}

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
/// 토스트 사이 세로 간격 (px).
const TOAST_GAP: f32 = 6.0;
/// 스코프 가장자리에서의 안쪽 여백 (px).
const SCOPE_MARGIN: f32 = 12.0;
/// 본문 텍스트의 좌우/상하 여백 (px).
const PADDING_X: f32 = 12.0;
const PADDING_Y: f32 = 8.0;
/// 좌측 컬러 바 두께 (px).
const ACCENT_BAR_WIDTH: f32 = 4.0;

pub struct ToastManager {
    toasts: Vec<ToastState>,
    next_id: u64,
}

impl ToastManager {
    pub fn new() -> Self {
        Self {
            toasts: Vec::new(),
            next_id: 1,
        }
    }

    /// 토스트 발사. 사용자 행동에서만 호출되어야 한다.
    pub fn push(&mut self, message: impl Into<String>, kind: ToastKind, scope: ToastScope) {
        let message = message.into();
        let now = Instant::now();

        // Coalesce: 같은 스코프·같은 메시지가 짧은 시간 내에 또 오면 수명만 갱신한다.
        if let Some(existing) = self.toasts.iter_mut().rev().find(|t| {
            t.scope == scope
                && t.message == message
                && now.duration_since(t.spawned_at) < COALESCE_WINDOW
        }) {
            existing.spawned_at = now;
            existing.kind = kind;
            existing.lifetime = DEFAULT_LIFETIME;
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
            lifetime: DEFAULT_LIFETIME,
        });

        // 스코프당 최대 개수 초과 시 가장 오래된 것 제거.
        let count_in_scope = self.toasts.iter().filter(|t| t.scope == scope).count();
        if count_in_scope > MAX_PER_SCOPE {
            if let Some(idx) = self.toasts.iter().position(|t| t.scope == scope) {
                self.toasts.remove(idx);
            }
        }
    }

    /// 편의 헬퍼: Info 토스트.
    pub fn push_info(&mut self, message: impl Into<String>, scope: ToastScope) {
        self.push(message, ToastKind::Info, scope);
    }

    /// 매 프레임 호출. 만료된 토스트를 제거하고 살아있는 토스트를 그린다.
    /// `draw_ctx`는 PopupManager가 만든 것을 그대로 공유한다.
    ///
    /// `reduced_motion`이 true면 페이드 인/아웃을 0ms로 처리 (시각 자극 최소화).
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

        let th = theme::theme();
        let layer_id = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("toast_layer"));
        let painter = ctx.layer_painter(layer_id);

        // 3) 스코프별로 그루핑하여 우측 하단에서부터 위로 쌓는다.
        let mut by_scope: std::collections::HashMap<String, Vec<&ToastState>> =
            std::collections::HashMap::new();
        for t in &self.toasts {
            by_scope
                .entry(format!("{:?}", t.scope))
                .or_default()
                .push(t);
        }

        for (_, mut group) in by_scope {
            // 안정적인 순서: id 오름차순(= 발사 순서). 새것이 가장 아래.
            group.sort_by_key(|t| t.id);
            let scope = &group[0].scope;
            let Some(scope_rect) = Self::scope_rect(scope, draw_ctx, ctx) else {
                continue;
            };

            // 우측 하단 시작 좌표 (가장 새로운 토스트의 bottom).
            let mut cursor_y = scope_rect.max.y - SCOPE_MARGIN;

            // 새것부터 그리며 위로 올라간다.
            for t in group.iter().rev() {
                let alpha = if reduced_motion {
                    // 페이드 없음 — lifetime이 남았으면 1.0, 끝났으면 0.0.
                    let age_ms = now.duration_since(t.spawned_at).as_secs_f32() * 1000.0;
                    let life_ms = t.lifetime.as_secs_f32() * 1000.0;
                    if age_ms < life_ms { 1.0 } else { 0.0 }
                } else {
                    Self::compute_alpha(t, now)
                };
                if alpha <= 0.0 {
                    continue;
                }

                let body_text = t.message.as_str();
                let max_width = (scope_rect.width() * 0.8).max(80.0);
                let font = egui::FontId::proportional(th.font_size_body.value());

                // 텍스트 갤리(줄바꿈 포함) 측정.
                let galley = ctx.fonts(|f| {
                    f.layout(
                        body_text.to_string(),
                        font.clone(),
                        th.text.into(),
                        max_width - PADDING_X * 2.0 - ACCENT_BAR_WIDTH,
                    )
                });

                let toast_w = (galley.size().x + PADDING_X * 2.0 + ACCENT_BAR_WIDTH).min(max_width);
                let toast_h = galley.size().y + PADDING_Y * 2.0;

                let max_x = scope_rect.max.x - SCOPE_MARGIN;
                let bottom_y = cursor_y;
                let top_y = bottom_y - toast_h;
                let left_x = max_x - toast_w;

                let rect = egui::Rect::from_min_max(
                    egui::pos2(left_x, top_y),
                    egui::pos2(max_x, bottom_y),
                );

                let bg = th.surface0.gamma_multiply(alpha);
                let border = th.surface1.gamma_multiply(alpha);
                let accent = Self::accent_color(t.kind, &th).gamma_multiply(alpha);

                // 배경 + 보더.
                painter.rect_filled(rect, th.corner_radius.value(), bg);
                painter.rect_stroke(
                    rect,
                    th.corner_radius.value(),
                    egui::Stroke::new(th.border_width.value(), border),
                    egui::StrokeKind::Inside,
                );

                // 좌측 컬러 바.
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

                // 본문 텍스트. galley 색을 alpha 적용해 다시 만들지 않고 그대로 그린다 —
                // 알파는 배경/보더로 표현하고, 텍스트는 단단하게 둬도 가독성에 도움이 된다.
                let text_pos = egui::pos2(
                    rect.min.x + ACCENT_BAR_WIDTH + PADDING_X,
                    rect.min.y + PADDING_Y,
                );
                painter.galley(text_pos, galley, th.text.gamma_multiply(alpha).into());

                cursor_y = top_y - TOAST_GAP;
            }
        }
    }

    /// 페이드 인/아웃 알파 계산.
    fn compute_alpha(t: &ToastState, now: Instant) -> f32 {
        let age_ms = now.duration_since(t.spawned_at).as_secs_f32() * 1000.0;
        let life_ms = t.lifetime.as_secs_f32() * 1000.0;

        if age_ms < FADE_IN_MS {
            (age_ms / FADE_IN_MS).clamp(0.0, 1.0)
        } else if age_ms < life_ms {
            1.0
        } else {
            let fade_out = (age_ms - life_ms) / FADE_OUT_MS;
            (1.0 - fade_out).clamp(0.0, 1.0)
        }
    }

    fn accent_color(kind: ToastKind, th: &theme::Theme) -> egui::Color32 {
        match kind {
            ToastKind::Info => th.blue.into(),
            ToastKind::Success => th.green.into(),
            ToastKind::Warning => th.yellow.into(),
            ToastKind::Error => th.red.into(),
        }
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

impl Default for ToastManager {
    fn default() -> Self {
        Self::new()
    }
}
