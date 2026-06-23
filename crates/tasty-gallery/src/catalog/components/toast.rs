//! Toast 카드 스택 데모 (Tier 3 재분류).
//!
//! 본체 `src/adapters/ui/toast.rs::draw_toast_view` 의 *우측 하단 스택 시각* 을
//! mock props 로 재현. ToastManager 의 *상태 관리* (push / coalesce / lifetime /
//! fade) 는 그대로 유지된다 — 갤러리는 미리 계산된 alpha 만 주입.
//!
//! 갤러리가 본체 binary 에 의존할 수 없어 ToastKind / Entry / Scope 구조를 로컬
//! 미러. POC 패턴: `.claude-workspace/conductor/tier-3-props-extraction-pattern.md`.
//!
//! 대표 상태 (6 가지):
//! 1. Single Info (정상)
//! 2. Single Success
//! 3. Single Warning
//! 4. Single Error
//! 5. 긴 메시지 (줄바꿈 wrap)
//! 6. 스택 4 개 (Info → Success → Warning → Error, fade alpha 그라데이션)
//!
//! Note: Tooltip 레이어 위치 결정은 본체에서만 의미가 있으므로 데모는 카드 그룹을
//! 한 frame area 안에 우측 하단 앵커로 그려 *상대 위치* 만 시각화한다.

use tasty_type_appearance::theme::Theme;

const TOAST_GAP: f32 = 6.0;
const SCOPE_MARGIN: f32 = 12.0;
const PADDING_X: f32 = 12.0;
const PADDING_Y: f32 = 8.0;
const ACCENT_BAR_WIDTH: f32 = 4.0;
/// 본체 `toast.rs` 와 동일 — 좁은 surface 에서 max_width 클램프 하한.
const MIN_TOAST_INNER_WIDTH: f32 = 48.0;

#[derive(Clone, Copy, Debug)]
enum ToastKind {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
struct ToastEntryView {
    kind: ToastKind,
    message: String,
    alpha: f32,
}

#[derive(Clone, Debug)]
struct ToastScopeView {
    /// 데모용 — 카드를 그릴 영역 (frame 의 local rect).
    scope_rect: egui::Rect,
    /// id 오름차순 (= 발사 순서). view 가 reverse 해서 위로 쌓는다.
    entries: Vec<ToastEntryView>,
}

struct ToastViewProps<'a> {
    theme: &'a Theme,
    scopes: &'a [ToastScopeView],
}

fn accent_color(kind: ToastKind, theme: &Theme) -> egui::Color32 {
    match kind {
        ToastKind::Info => theme.accent_primary().into(),
        ToastKind::Success => theme.accent_success().into(),
        ToastKind::Warning => theme.accent_warning().into(),
        ToastKind::Error => theme.accent_danger().into(),
    }
}

/// 본체 `draw_toast_view` 의 카드 스택 시각 미러. Tooltip 레이어 대신 painter_at
/// 으로 frame 내부에 직접 그린다.
fn draw_toast_view_mock(ui: &mut egui::Ui, props: &ToastViewProps<'_>) {
    let th = props.theme;
    for scope in props.scopes {
        let scope_rect = scope.scope_rect;
        let painter = ui.painter_at(scope_rect);
        let mut cursor_y = scope_rect.max.y - SCOPE_MARGIN;

        for entry in scope.entries.iter().rev() {
            let alpha = entry.alpha;
            if alpha <= 0.0 {
                continue;
            }

            // 본체 toast.rs 와 동일: 좁은 surface 에서 좌측 누출 방지 클램프
            // (정상 폭에서는 0.8 폭 그대로 = 시각 무변경) + wrap_width 음수 가드.
            let inner_limit = (scope_rect.width() - SCOPE_MARGIN * 2.0).max(MIN_TOAST_INNER_WIDTH);
            let max_width = (scope_rect.width() * 0.8).max(80.0).min(inner_limit);
            let font = egui::FontId::proportional(th.font_size_body.value());
            let text_color = th.text.gamma_multiply(alpha);
            let wrap_width = (max_width - PADDING_X * 2.0 - ACCENT_BAR_WIDTH).max(1.0);

            let galley = ui.ctx().fonts(|f| {
                f.layout(
                    entry.message.clone(),
                    font.clone(),
                    text_color.into(),
                    wrap_width,
                )
            });

            let toast_w = (galley.size().x + PADDING_X * 2.0 + ACCENT_BAR_WIDTH).min(max_width);
            let toast_h = galley.size().y + PADDING_Y * 2.0;

            let max_x = scope_rect.max.x - SCOPE_MARGIN;
            let bottom_y = cursor_y;
            let top_y = bottom_y - toast_h;
            // 본체 toast.rs 와 동일: scope 상단 초과 시 옛 토스트 생략(클립 보완).
            if top_y < scope_rect.min.y {
                break;
            }
            let left_x = max_x - toast_w;

            let rect =
                egui::Rect::from_min_max(egui::pos2(left_x, top_y), egui::pos2(max_x, bottom_y));

            let bg = th.surface0.gamma_multiply(alpha);
            let border = th.surface1.gamma_multiply(alpha);
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
            painter.galley(text_pos, galley, text_color.into());

            cursor_y = top_y - TOAST_GAP;
        }
    }
}

/// 카드 그룹을 보여주기 위해 surface1 보더의 영역을 할당하고 그 안에 toast view 호출.
fn frame_case(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    height: f32,
    entries: Vec<ToastEntryView>,
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let painter = ui.painter_at(rect);

    // Frame 배경 — base 색으로 *어디에 떠 있는지* 가시화.
    painter.rect_filled(
        rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.base),
    );
    painter.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.surface1),
        ),
        egui::StrokeKind::Inside,
    );

    // 좌상단에 "scope" 라벨 — 데모임을 알림.
    painter.text(
        egui::pos2(rect.min.x + 8.0, rect.min.y + 6.0),
        egui::Align2::LEFT_TOP,
        "scope (frame)",
        egui::FontId::proportional(theme.font_size_micro.value()),
        egui::Color32::from(theme.overlay0),
    );

    let scopes = vec![ToastScopeView {
        scope_rect: rect,
        entries,
    }];
    let props = ToastViewProps {
        theme,
        scopes: &scopes,
    };
    draw_toast_view_mock(ui, &props);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "ToastViewProps + draw_toast_view — AppState/CoreState 비의존 view 함수.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "Wrapper: src/adapters/ui/toast.rs::ToastManager::draw (상태 관리 + view 호출)",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(12.0);

    egui::ScrollArea::vertical()
        .id_salt("toast_demo_scroll")
        .show(ui, |ui| {
            // Case 1 — Info
            ui.label(
                egui::RichText::new("Case 1 — Info (blue accent, alpha=1.0)")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            frame_case(
                ui,
                theme,
                480.0,
                120.0,
                vec![ToastEntryView {
                    kind: ToastKind::Info,
                    message: "Reloaded settings.json".into(),
                    alpha: 1.0,
                }],
            );
            ui.add_space(16.0);

            // Case 2 — Success
            ui.label(
                egui::RichText::new("Case 2 — Success (green accent)")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            frame_case(
                ui,
                theme,
                480.0,
                120.0,
                vec![ToastEntryView {
                    kind: ToastKind::Success,
                    message: "Workspace saved.".into(),
                    alpha: 1.0,
                }],
            );
            ui.add_space(16.0);

            // Case 3 — Warning
            ui.label(
                egui::RichText::new("Case 3 — Warning (yellow accent)")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            frame_case(
                ui,
                theme,
                480.0,
                120.0,
                vec![ToastEntryView {
                    kind: ToastKind::Warning,
                    message: "Low disk space — clean up downloads.".into(),
                    alpha: 1.0,
                }],
            );
            ui.add_space(16.0);

            // Case 4 — Error
            ui.label(
                egui::RichText::new("Case 4 — Error (red accent)")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            frame_case(
                ui,
                theme,
                480.0,
                120.0,
                vec![ToastEntryView {
                    kind: ToastKind::Error,
                    message: "Plugin crashed: tasty-plugin-foo. See logs.".into(),
                    alpha: 1.0,
                }],
            );
            ui.add_space(16.0);

            // Case 5 — Long body (wrap)
            ui.label(
                egui::RichText::new("Case 5 — 긴 본문 (max_width 80% 내 줄바꿈 wrap)")
                    .strong()
                    .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            frame_case(
                ui,
                theme,
                480.0,
                180.0,
                vec![ToastEntryView {
                    kind: ToastKind::Warning,
                    message:
                        "이것은 매우 긴 toast 메시지로, scope 의 가로 80% 폭을 초과하면 여러 줄에 \
                         걸쳐 wrap 된다. 본체 view 가 ctx.fonts(|f| f.layout(...)) 로 측정하고 \
                         toast 카드 크기를 동적으로 늘린다. 여기서는 mock 으로 같은 알고리즘을 \
                         시연한다."
                            .into(),
                    alpha: 1.0,
                }],
            );
            ui.add_space(16.0);

            // Case 6 — 스택 4 개 (fade 그라데이션)
            ui.label(
                egui::RichText::new(
                    "Case 6 — 4 toast 스택 (id 오름차순: Info → Success → Warning → Error). \
                     alpha 그라데이션으로 fade-in/out 단계 시각화.",
                )
                .strong()
                .color(egui::Color32::from(theme.text)),
            );
            ui.add_space(2.0);
            frame_case(
                ui,
                theme,
                480.0,
                280.0,
                vec![
                    ToastEntryView {
                        kind: ToastKind::Info,
                        message: "Connected to plugin host.".into(),
                        alpha: 0.4, // 가장 오래된 — fade-out 진행
                    },
                    ToastEntryView {
                        kind: ToastKind::Success,
                        message: "Loaded 3 plugins.".into(),
                        alpha: 0.7,
                    },
                    ToastEntryView {
                        kind: ToastKind::Warning,
                        message: "Plugin 'foo' missing signature.".into(),
                        alpha: 1.0,
                    },
                    ToastEntryView {
                        kind: ToastKind::Error,
                        message: "Failed to start 'bar': missing entrypoint.".into(),
                        alpha: 1.0, // 가장 최근 — full opacity
                    },
                ],
            );

            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(
                    "⚠ 본체는 Tooltip 레이어에 그려 모든 UI 위에 표시. lifetime (2s) + \
                     fade-in (80ms) / fade-out (160ms) 은 ToastManager 가 매 프레임 \
                     alpha 로 계산해 view 에 전달 — view 는 시간 의존 없음.",
                )
                .small()
                .color(egui::Color32::from(theme.subtext0)),
            );
        });
}
