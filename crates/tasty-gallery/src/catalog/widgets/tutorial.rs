//! Tutorial — the 6th overlay family (Marker overlay · Callout bubble · Topic
//! popup). 디자인(4) `gallery/overlays-tutorial.jsx` 의 구조 전사(structural
//! transcription): jsx 의 `App`/`Marker`/`Scrim`/`Callout`/`Topic`/`TopicPopup`
//! 함수를 egui 함수로 1:1 대응한다.
//!
//! - **Marker** — 대상 rect 위에 그리는 독립 링/글로우. `pointer-events:none`,
//!   최상위 z. 메시지가 없는 순수 기하 마커(6번째 오버레이).
//! - **Callout** — 244px 고정폭 안내 말풍선(step/total·dot rail·Skip/Back/Next·
//!   4방 tail). 버튼은 DS `Button` 재사용.
//! - **Topic popup** — 360px CenteredFocused 팝업(스크롤 리스트 + 진행).
//!
//! 색·폰트·선굵기·간격·반경은 전부 `Theme` 토큰. 고정폭(244/360) 등 컴포넌트 박스
//! 치수는 구조적 레이아웃 값으로 `dialog::frame_card` 의 `240.0` 관례를 따라 리터럴.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, margin_all, vspace};

use crate::catalog::spec::{self, StageVariant, TokenChip};

// ── 고정 컴포넌트 치수 (구조 값 — dialog::frame_card 240.0 와 동일 관례) ──────
const CALLOUT_W: f32 = 244.0;
const POPUP_W: f32 = 360.0;
const TAIL: f32 = 12.0; // 12px diamond → 삼각 tail

// ── faux app 셸 (jsx `App`) ────────────────────────────────────────────────
/// 마커가 그 위에 뜨는 가짜 앱 무대. jsx `App` 의 사이드바(116) + 탭바(24) +
/// 터미널 본문 + 상태바(20) 를 painter 로 절대 배치 전사한다.
fn paint_faux_app(p: &egui::Painter, r: egui::Rect, theme: &Theme) {
    let sep = egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui());
    let sidebar_w = 116.0_f32.min(r.width() * 0.5);

    // ── 사이드바 (bg-sidebar, border-right) ──
    let side = egui::Rect::from_min_max(r.min, egui::pos2(r.min.x + sidebar_w, r.max.y));
    p.rect_filled(side, 0.0, theme.bg_sidebar().to_egui());
    p.vline(side.max.x, side.y_range(), sep);
    let rows = [("web", true), ("api", false), ("db", false)];
    let mut ry = side.min.y + 10.0;
    for (label, active) in rows {
        let row = egui::Rect::from_min_size(
            egui::pos2(side.min.x + 8.0, ry),
            egui::vec2(sidebar_w - 16.0, 22.0),
        );
        if active {
            p.rect_filled(
                row,
                theme.corner_radius_sm.value(),
                theme.surface_active().to_egui(),
            );
        }
        // workspace 상태 dot (accent-success).
        p.circle_filled(
            egui::pos2(row.min.x + 6.0 + 4.0, row.center().y),
            4.0,
            theme.accent_success().to_egui(),
        );
        p.text(
            egui::pos2(row.min.x + 6.0 + 14.0, row.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(11.0),
            if active {
                theme.text_primary().to_egui()
            } else {
                theme.text_muted().to_egui()
            },
        );
        ry += 22.0 + 3.0;
    }

    // ── 메인 컬럼 ──
    let main = egui::Rect::from_min_max(egui::pos2(side.max.x, r.min.y), r.max);
    // 탭바 (24, bg-panel, border-bottom).
    let tabbar = egui::Rect::from_min_size(main.min, egui::vec2(main.width(), 24.0));
    p.rect_filled(tabbar, 0.0, theme.bg_panel().to_egui());
    p.hline(tabbar.x_range(), tabbar.max.y, sep);
    let term_bg = theme.surface("terminal").focused_bg.to_egui();
    let mut tx = tabbar.min.x;
    for (i, t) in ["zsh", "logs", "vim"].iter().enumerate() {
        let tw = 12.0
            + p.layout_no_wrap(
                t.to_string(),
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            )
            .size()
            .x
            + 12.0;
        let tab = egui::Rect::from_min_size(egui::pos2(tx, tabbar.min.y), egui::vec2(tw, 24.0));
        if i == 0 {
            p.rect_filled(tab, 0.0, term_bg);
        }
        p.vline(tab.max.x, tab.y_range(), sep);
        p.text(
            tab.center(),
            egui::Align2::CENTER_CENTER,
            t,
            egui::FontId::proportional(11.0),
            if i == 0 {
                theme.text_primary().to_egui()
            } else {
                theme.text_muted().to_egui()
            },
        );
        tx += tw;
    }

    // 터미널 본문 (surface-terminal-focused-bg).
    let body = egui::Rect::from_min_max(
        egui::pos2(main.min.x, tabbar.max.y),
        egui::pos2(main.max.x, main.max.y - 20.0),
    );
    p.rect_filled(body, 0.0, term_bg);
    p.text(
        egui::pos2(body.min.x + 12.0, body.min.y + 10.0),
        egui::Align2::LEFT_TOP,
        "$ kubectl get pods -n prod",
        egui::FontId::monospace(11.0),
        theme.text_muted().to_egui(),
    );

    // 상태바 (20, bg-sidebar, border-top).
    let status = egui::Rect::from_min_max(egui::pos2(main.min.x, body.max.y), main.max);
    p.rect_filled(status, 0.0, theme.bg_sidebar().to_egui());
    p.hline(status.x_range(), status.min.y, sep);
}

/// 스포트라이트 scrim — 전체를 scrim-bg 로 덮는다(마커는 그 위에 밝게).
fn paint_scrim(p: &egui::Painter, r: egui::Rect, theme: &Theme) {
    p.rect_filled(r, 0.0, theme.scrim().to_egui());
}

/// 마커 링 (jsx `Marker`) — 2px accent-primary 링 + (glow 시) 정적 halo.
fn paint_marker(p: &egui::Painter, rect: egui::Rect, theme: &Theme, glow: bool) {
    let accent = theme.accent_primary();
    let radius = theme.corner_radius.value();
    if glow {
        // 정적 halo — accent 저알파 확장 링 2겹 (일회성 오버레이 이펙트, sanctioned).
        for (grow, alpha) in [(5.0_f32, 60u8), (2.5, 110)] {
            p.rect_stroke(
                rect.expand(grow),
                radius + grow,
                egui::Stroke::new(
                    theme.focus_ring_width.value() + grow,
                    accent.with_alpha(alpha).to_egui(),
                ),
                egui::StrokeKind::Outside,
            );
        }
    }
    p.rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.focus_ring_width.value(), accent.to_egui()),
        egui::StrokeKind::Inside,
    );
}

// ── Callout (jsx `Callout`) ────────────────────────────────────────────────

/// tail 방향 — 마커가 말풍선의 어느 쪽에 있는지.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tail {
    Up,
    Down,
    Left,
    Right,
}

/// 244px 고정폭 안내 말풍선. 제목·본문·step/total·dot rail·Skip/Back/Next·4방
/// tail. Frame 으로 본체를 그린 뒤 tail 삼각형을 painter 로 얹는다.
#[allow(clippy::too_many_arguments)]
fn callout(
    ui: &mut egui::Ui,
    theme: &Theme,
    tail: Tail,
    step: usize,
    total: usize,
    title: &str,
    body: &str,
    first: bool,
) {
    let resp = egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_strong().to_egui(),
        ))
        .corner_radius(theme.corner_radius_lg.value())
        .shadow(theme.shadow_popover().to_egui())
        .inner_margin(egui::Margin {
            left: theme.spacing_lg.value() as i8,
            right: theme.spacing_lg.value() as i8,
            top: theme.spacing_md.value() as i8,
            bottom: theme.spacing_md.value() as i8,
        })
        .show(ui, |ui| {
            ui.set_width(CALLOUT_W - 2.0 * theme.spacing_lg.value());
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            // step / total (mono, accent-primary, 600).
            ui.label(
                egui::RichText::new(format!("{step} / {total}"))
                    .monospace()
                    .size(theme.font_size_micro.value())
                    .strong()
                    .color(theme.accent_primary().to_egui()),
            );
            vspace(ui, theme.spacing_xs);
            // 제목 (13, semibold, text-primary).
            ui.label(
                egui::RichText::new(title)
                    .size(theme.font_size_body.value())
                    .strong()
                    .color(theme.text_primary().to_egui()),
            );
            // 디자인 전사값 6px — 토큰 산술(4×1.5)로 표현.
            vspace(ui, theme.spacing_xs * 1.5);
            // 본문 (11, text-secondary).
            ui.label(
                egui::RichText::new(body)
                    .size(theme.font_size_caption.value())
                    .color(theme.text_secondary().to_egui()),
            );
            ui.add_space(theme.spacing_md.value());
            // 버튼 행: dot rail(좌) + Skip · Back · Next(우).
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                for i in 0..total {
                    let c = if i == step - 1 {
                        theme.accent_primary().to_egui()
                    } else {
                        theme.surface_active().to_egui()
                    };
                    let (r, _) = ui.allocate_exact_size(egui::vec2(5.0, 5.0), egui::Sense::hover());
                    ui.painter().circle_filled(r.center(), 2.5, c);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    Button::new("Next")
                        .variant(ButtonVariant::Primary)
                        .size(ControlSize::Sm)
                        .show(ui, theme);
                    if !first {
                        Button::new("Back")
                            .variant(ButtonVariant::Secondary)
                            .size(ControlSize::Sm)
                            .show(ui, theme);
                    }
                    // Skip — 저강조 링크.
                    ui.label(
                        egui::RichText::new("Skip")
                            .size(theme.font_size_caption.value())
                            .color(theme.text_muted().to_egui()),
                    );
                });
            });
        });

    paint_tail(ui.painter(), resp.response.rect, theme, tail);
}

/// tail 삼각형 — bubble 모서리에서 마커 방향으로 튀어나온다. jsx 의 12px 회전
/// diamond(2변 border) 를 삼각형으로 전사(외곽 2변만 border-strong).
fn paint_tail(p: &egui::Painter, bubble: egui::Rect, theme: &Theme, tail: Tail) {
    let fill = theme.surface_raised().to_egui();
    let stroke = egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui());
    let h = TAIL / 2.0; // 삼각 높이 (돌출 길이).
    // jsx tail offset: up/down left:28, left/right top:24 (bubble 모서리에서의 위치).
    let (a, b, apex) = match tail {
        Tail::Up => {
            let cx = bubble.min.x + 28.0;
            (
                egui::pos2(cx - h, bubble.min.y),
                egui::pos2(cx + h, bubble.min.y),
                egui::pos2(cx, bubble.min.y - h),
            )
        }
        Tail::Down => {
            let cx = bubble.min.x + 28.0;
            (
                egui::pos2(cx - h, bubble.max.y),
                egui::pos2(cx + h, bubble.max.y),
                egui::pos2(cx, bubble.max.y + h),
            )
        }
        Tail::Left => {
            let cy = bubble.min.y + 24.0;
            (
                egui::pos2(bubble.min.x, cy - h),
                egui::pos2(bubble.min.x, cy + h),
                egui::pos2(bubble.min.x - h, cy),
            )
        }
        Tail::Right => {
            let cy = bubble.min.y + 24.0;
            (
                egui::pos2(bubble.max.x, cy - h),
                egui::pos2(bubble.max.x, cy + h),
                egui::pos2(bubble.max.x + h, cy),
            )
        }
    };
    p.add(egui::Shape::convex_polygon(
        vec![a, apex, b],
        fill,
        egui::Stroke::NONE,
    ));
    // 외곽 2변만 stroke (base 는 bubble 이 덮음).
    p.line_segment([a, apex], stroke);
    p.line_segment([apex, b], stroke);
}

// ── Topic row + popup (jsx `Topic` / `TopicPopup`) ─────────────────────────

fn topic_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    n: usize,
    title: &str,
    desc: &str,
    sel: bool,
    done: bool,
) {
    let border = if sel {
        theme.accent_primary().with_alpha(102).to_egui() // 40%
    } else {
        egui::Color32::TRANSPARENT
    };
    let fill = if sel {
        theme.surface_active().to_egui()
    } else {
        egui::Color32::TRANSPARENT
    };
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(theme.border_width.value(), border))
        .corner_radius(theme.corner_radius.value())
        // 디자인 전사값 10px — 토큰 산술(4×2.5)로 표현.
        .inner_margin(margin_all(theme.spacing_xs * 2.5))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                // 인덱스 캡 (20x20, radius-sm).
                let (cap, _) = ui.allocate_exact_size(egui::vec2(20.0, 20.0), egui::Sense::hover());
                let (cap_bg, cap_fg) = if sel {
                    (
                        theme.accent_primary().to_egui(),
                        theme.text_on_accent().to_egui(),
                    )
                } else {
                    (
                        theme.surface_raised().to_egui(),
                        theme.text_muted().to_egui(),
                    )
                };
                ui.painter()
                    .rect_filled(cap, theme.corner_radius_sm.value(), cap_bg);
                ui.painter().text(
                    cap.center(),
                    egui::Align2::CENTER_CENTER,
                    n.to_string(),
                    egui::FontId::monospace(theme.font_size_micro.value()),
                    cap_fg,
                );
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(
                        egui::RichText::new(title)
                            .size(theme.font_size_body.value())
                            .color(theme.text_primary().to_egui()),
                    );
                    ui.label(
                        egui::RichText::new(desc)
                            .size(theme.font_size_caption.value())
                            .color(theme.text_muted().to_egui()),
                    );
                });
                if done {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        ui.label(
                            egui::RichText::new("✓")
                                .size(theme.font_size_caption.value())
                                .color(theme.accent_success().to_egui()),
                        );
                    });
                }
            });
        });
}

/// 주제 목록 팝업 (360px, bg-panel, radius-8). `scaled` 시 4개 주제 + 완료 표시.
fn topic_popup(ui: &mut egui::Ui, theme: &Theme, scaled: bool) {
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_strong().to_egui(),
        ))
        .corner_radius(theme.corner_radius_lg.value())
        .shadow(theme.shadow_popover().to_egui())
        .show(ui, |ui| {
            ui.set_width(POPUP_W);
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            // 헤더.
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: theme.spacing_lg.value() as i8,
                    right: theme.spacing_lg.value() as i8,
                    top: theme.spacing_md.value() as i8,
                    bottom: theme.spacing_md.value() as i8,
                })
                .show(ui, |ui| {
                    ui.set_width(POPUP_W);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("튜토리얼")
                                .size(theme.font_size_body.value())
                                .strong()
                                .color(theme.text_primary().to_egui()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(egui::RichText::new("✕").color(theme.text_muted().to_egui()));
                        });
                    });
                });
            hsep(ui, theme);
            // 리스트 (max-height 200 → 내부 스크롤).
            egui::Frame::new()
                .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
                .show(ui, |ui| {
                    ui.set_width(POPUP_W);
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                            topic_row(
                                ui,
                                theme,
                                1,
                                "워크스페이스 · 패인 · 탭 · 서피스",
                                "화면 구조 4개 기본 개념.",
                                true,
                                scaled,
                            );
                            if scaled {
                                topic_row(
                                    ui,
                                    theme,
                                    2,
                                    "커맨드 팔레트 & 단축키",
                                    "모든 명령을 키보드로.",
                                    false,
                                    false,
                                );
                                topic_row(
                                    ui,
                                    theme,
                                    3,
                                    "포트 스캐너 · 리모트",
                                    "로컬 포트와 원격 세션 연결.",
                                    false,
                                    false,
                                );
                                topic_row(
                                    ui,
                                    theme,
                                    4,
                                    "프리셋 & 워크스페이스 레이아웃",
                                    "패인 배치를 저장·복원.",
                                    false,
                                    false,
                                );
                            }
                        });
                });
            hsep(ui, theme);
            // 푸터 (Esc 힌트 + 진행).
            egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: theme.spacing_lg.value() as i8,
                    right: theme.spacing_lg.value() as i8,
                    top: theme.spacing_md.value() as i8,
                    bottom: theme.spacing_md.value() as i8,
                })
                .show(ui, |ui| {
                    ui.set_width(POPUP_W);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new("Esc 닫기")
                                .monospace()
                                .size(theme.font_size_micro.value())
                                .color(theme.text_muted().to_egui()),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            Button::new("진행")
                                .variant(ButtonVariant::Primary)
                                .size(ControlSize::Sm)
                                .show(ui, theme);
                        });
                    });
                });
        });
}

/// 전체 폭 1px separator.
fn hsep(ui: &mut egui::Ui, theme: &Theme) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(w, theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}

// ── faux-app 데모 박스 헬퍼 ─────────────────────────────────────────────────
/// 무대 박스(bg-app + border + radius) 를 할당하고 그 안에 painter 로 그린다.
fn demo_box(
    ui: &mut egui::Ui,
    theme: &Theme,
    w: f32,
    h: f32,
    add: impl FnOnce(&egui::Painter, egui::Rect),
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, theme.corner_radius.value(), theme.bg_app().to_egui());
    add(&p, rect);
    p.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        egui::StrokeKind::Inside,
    );
}

// ── Spec: Marker overlay ────────────────────────────────────────────────────
pub fn draw_marker(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "ring — solid 2px", |ui| {
            demo_box(ui, theme, 260.0, 150.0, |p, r| {
                paint_faux_app(p, r, theme);
                let m = egui::Rect::from_min_max(
                    egui::pos2(r.min.x + 34.0, r.min.y + 38.0),
                    egui::pos2(r.max.x - 16.0, r.max.y - 22.0),
                );
                paint_marker(p, m, theme, false);
            });
        });
        spec::cluster(ui, theme, "glow + spotlight — default", |ui| {
            demo_box(ui, theme, 260.0, 150.0, |p, r| {
                paint_faux_app(p, r, theme);
                paint_scrim(p, r, theme);
                let m = egui::Rect::from_min_max(
                    egui::pos2(r.min.x + 34.0, r.min.y + 38.0),
                    egui::pos2(r.max.x - 16.0, r.max.y - 22.0),
                );
                paint_marker(p, m, theme, true);
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "family",
                "Modal · Popup · Toast · Banner · Modifier-hint · Marker",
            ),
            ("z-layer", "top — above all content"),
            ("pointer", "none (clicks pass through)"),
            ("ring width", "2px (focus-ring-width)"),
            ("radius", "4px (radius)"),
            ("highlight", "static glow default · pulse opt-in"),
            ("fires on", "Tools → Tutorial only"),
        ],
        &[
            TokenChip::new(
                "accent-primary",
                "ring + halo",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new("scrim-bg", "spotlight dim", theme.scrim().to_egui()),
            TokenChip::new(
                "border-strong",
                "callout edge",
                theme.border_strong().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Marker is the 6th overlay concept — the only one with no message. A pure \
         geometric ring drawn at a rect on the top z-layer, never touching the target \
         widget's own border, pointer-events:none so clicks pass through. Spotlight scrim \
         is ON by default (user-disableable); pulse is opt-in and falls back to a static \
         ring under reduced-motion.",
    );
}

// ── Spec: Callout bubble ────────────────────────────────────────────────────
pub fn draw_callout(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "tail up", |ui| {
            callout(
                ui,
                theme,
                Tail::Up,
                2,
                5,
                "탭 헤더",
                "패인 상단의 이 띠에서 탭을 전환·추가·닫습니다.",
                false,
            );
        });
        spec::cluster(ui, theme, "tail down", |ui| {
            callout(
                ui,
                theme,
                Tail::Down,
                3,
                5,
                "패인",
                "탭 하나가 열리는 이 사각 영역이 패인입니다.",
                false,
            );
        });
        spec::cluster(ui, theme, "tail left", |ui| {
            callout(
                ui,
                theme,
                Tail::Left,
                4,
                5,
                "서피스",
                "패인 안에서 실제 터미널·마크다운이 그려지는 면.",
                false,
            );
        });
        spec::cluster(ui, theme, "tail right · first step", |ui| {
            callout(
                ui,
                theme,
                Tail::Right,
                1,
                5,
                "워크스페이스",
                "이 전체 영역이 하나의 워크스페이스입니다.",
                true,
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("width", "244px fixed"),
            ("tail", "up · down · left · right (12px)"),
            ("progress", "2/5 label + dot rail"),
            ("buttons", "Skip (link) · Back (secondary) · Next (primary)"),
            ("first step", "Back hidden"),
            ("last step", "Next reopens topic popup"),
            (
                "placement",
                "below → above → right → left; flip + clamp 8px",
            ),
        ],
        &[
            TokenChip::new(
                "surface-raised",
                "bubble fill",
                theme.surface_raised().to_egui(),
            ),
            TokenChip::new(
                "border-strong",
                "edge + tail",
                theme.border_strong().to_egui(),
            ),
            TokenChip::new(
                "accent-primary",
                "step · dot · Next",
                theme.accent_primary().to_egui(),
            ),
        ],
    );

    spec::do_(
        ui,
        theme,
        "Reuse the DS Button (primary Next / secondary Back) and keep Skip a low-emphasis \
         link — the callout is guidance, not a decision surface.",
    );
}

/// scrim 무대 위에 중앙 정렬된 topic 팝업을 얹는 데모 박스 (jsx 의 grid cell).
fn topic_stage(ui: &mut egui::Ui, theme: &Theme, scaled: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(392.0, 300.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, theme.corner_radius.value(), theme.bg_app().to_egui());
    paint_scrim(&p, rect, theme);
    // scrim 위 팝업 — 중앙 정렬 child Ui.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
    ));
    child.set_clip_rect(rect);
    child.vertical_centered(|ui| {
        topic_popup(ui, theme, scaled);
    });
    ui.painter().rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        egui::StrokeKind::Inside,
    );
}

// ── Spec: Topic-list popup ──────────────────────────────────────────────────
pub fn draw_topics(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "default — one topic", |ui| {
            topic_stage(ui, theme, false);
        });
        spec::cluster(ui, theme, "scaled — scrollable + done ✓", |ui| {
            topic_stage(ui, theme, true);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("type", "CenteredFocused popup + scrim"),
            ("width", "360px"),
            ("list", "max-height 200px → internal scroll"),
            (
                "row states",
                "hover · selected = surface-active + accent cap · done = ✓",
            ),
            ("footer", "Esc hint + 진행 (primary)"),
        ],
        &[
            TokenChip::new("bg-panel", "popup fill", theme.bg_panel().to_egui()),
            TokenChip::new("scrim-bg", "dim behind", theme.scrim().to_egui()),
            TokenChip::new(
                "surface-active",
                "selected row",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new("accent-success", "done ✓", theme.accent_success().to_egui()),
        ],
    );
}

// ── Spec: Composite step in place ───────────────────────────────────────────
pub fn draw_composite(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        let w = ui.available_width().min(520.0);
        demo_box(ui, theme, w, 340.0, |p, r| {
            paint_faux_app(p, r, theme);
            paint_scrim(p, r, theme);
            // step 1 마커 = 콘텐츠 전체영역(사이드바 제외).
            let m = egui::Rect::from_min_max(
                egui::pos2(r.min.x + 124.0, r.min.y + 8.0),
                egui::pos2(r.max.x - 10.0, r.max.y - 8.0),
            );
            paint_marker(p, m, theme, true);
        });
    });
    spec::note(
        ui,
        theme,
        "Popup closed: marker (glow ring) + spotlight dim + callout only. Step 1 wraps the \
         whole content area (tabs + surface + status, sidebar excluded); later steps shrink \
         the marker tab → pane → surface, revealing containment. Next → step 2 · Back → prev \
         · Skip/Esc → topic popup · last-step Next → reopen popup. Marker + dim are \
         pointer-events:none.",
    );
}
