//! Banner — 디자인(4) Overlays `banner` Section (3 Spec). 네 번째 overlay 패밀리
//! (Modal / Popup / Toast / **Banner**).
//!
//! 스코프 콘텐츠 영역 최상단(탭바 바로 아래 8px)에 떠 있는 focus-less 공지.
//! Toast 와 달리 **자기 마우스를 소비**하고 **action 을 실을 수 있다**. 선택적 TTL
//! 카운트다운(우상단)이 hover 시 × 로 전환된다. 스코프당 1개만 표시, 나머지는 큐(≤5).
//!
//! 본 specimen 은 디자인 `gallery/overlays.jsx` 의 3 Spec 구조를 전사한다:
//! 1. **shell + 예시** — 마우스 캡쳐 안내 배너(icon + 제목 + 본문 + Shift hint + action).
//! 2. **dismiss & TTL** — plain(hover × 노출) / TTL(우상단 카운트다운) 두 상태.
//! 3. **queue & stacking** — 상위 스코프 배너(전면) + 하위 스코프 배너(40% 디밍, 후면).
//!
//! 색·치수·폰트는 모두 `Theme` 토큰 경유(`from_rgb`/hex 리터럴 금지). 배너 전용
//! Tier-3 토큰은 banner-03 에서 본체 Theme 에 도입되어, 이 specimen 도 근사 없이
//! 토큰 접근자를 직접 쓴다: 디밍 = `opacity_recessed()`(0.4), 라운드 = `corner_radius_lg`
//! (radius-8), 그림자 = `shadow_popover()`. semantic 매핑(`banner_bg` → surface-raised 등)도
//! 전용 접근자(`banner_bg()`/`banner_border()`/`banner_icon_fg()`/`banner_countdown_fg()`)로
//! 노출된다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, kbd};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 색을 opacity 로 곱한다(하위 스코프 배너 디밍 — toast 스택 fade 와 같은 관습).
fn dim(color: egui::Color32, opacity: f32) -> egui::Color32 {
    color.gamma_multiply(opacity)
}

/// 배너 shell chrome — surface-raised fill + 1px border-strong + radius-8 + popover shadow.
/// padding 은 12(x)/8(y). `opacity` < 1 이면 모든 색을 곱해 디밍(recessed). `content` 는
/// 패딩 안의 child Ui 를 받아 본문(icon/제목/본문/action/dismiss)을 그린다.
fn banner_shell(
    ui: &mut egui::Ui,
    theme: &Theme,
    opacity: f32,
    content: impl FnOnce(&mut egui::Ui),
) {
    let bg = dim(theme.banner_bg().to_egui(), opacity);
    let border = dim(theme.banner_border().to_egui(), opacity);
    let mut shadow = theme.shadow_popover().to_egui();
    shadow.color = shadow.color.gamma_multiply(opacity);
    egui::Frame::new()
        .fill(bg)
        .stroke(egui::Stroke::new(theme.border_width.value(), border))
        .corner_radius(theme.corner_radius_lg.value())
        .shadow(shadow)
        .inner_margin(egui::Margin::symmetric(
            theme.spacing_md.value() as i8,
            theme.spacing_sm.value() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            content(ui);
        });
}

/// 인라인 leading 글리프 — `size` 정사각, `color` tint.
fn glyph(ui: &mut egui::Ui, g: MockGlyph, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    g.image(size, color).paint_at(ui, rect);
}

/// 제목 줄 — body(13) semibold, text-primary.
fn title_line(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_body.value())
            .strong()
            .color(theme.text_primary().to_egui()),
    );
}

/// 본문 줄 — caption(11), text-muted.
fn body_line(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// 우상단 dismiss × (hover 노출 상태). sm IconButton.
fn dismiss_x(ui: &mut egui::Ui, theme: &Theme) {
    IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .show(ui, theme, &|ui, rect, c| {
            icons::CLOSE.image(rect.height(), c).paint_at(ui, rect)
        });
}

/// 우상단 TTL 카운트다운 숫자 — mono micro(10), text-muted, tabular.
fn countdown(ui: &mut egui::Ui, theme: &Theme, seconds: u32) {
    ui.label(
        egui::RichText::new(seconds.to_string())
            .monospace()
            .size(theme.font_size_micro.value())
            .color(theme.banner_countdown_fg().to_egui()),
    );
}

// ── faux scope: 탭 스트립(반드시 비워둠) + 콘텐츠 + 배너 존 ────────────────────
// 배너가 "탭바 바로 아래(content-top + 8px), 양옆 8px margin" 에 뜨는 위치 관계를
// 전사한다. 탭바를 절대 덮지 않는다(탭 전환 차단 방지).
fn faux_scope(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    height: f32,
    banner: impl FnOnce(&mut egui::Ui),
) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let radius = theme.corner_radius.value();
    let painter = ui.painter_at(rect);

    // 터미널 surface 배경(#000 = ansi_black).
    painter.rect_filled(rect, radius, theme.ansi_black.to_egui());

    // 탭 스트립(28px, bg-sidebar) — server / dev / vim, vim active.
    let tab_h = 28.0;
    let tab_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), tab_h));
    painter.rect_filled(
        tab_rect,
        egui::CornerRadius {
            nw: radius as u8,
            ne: radius as u8,
            sw: 0,
            se: 0,
        },
        theme.bg_sidebar().to_egui(),
    );
    painter.hline(
        tab_rect.x_range(),
        tab_rect.bottom(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    let tabs = ["server", "dev", "vim"];
    let mut x = tab_rect.left();
    for (i, label) in tabs.iter().enumerate() {
        let active = i == 2;
        let pad = theme.spacing_md.value();
        let galley = painter.layout_no_wrap(
            label.to_string(),
            egui::FontId::proportional(theme.font_size_body.value()),
            if active {
                theme.text_primary().to_egui()
            } else {
                theme.text_muted().to_egui()
            },
        );
        let tab_w = galley.size().x + pad * 2.0;
        let this =
            egui::Rect::from_min_size(egui::pos2(x, tab_rect.top()), egui::vec2(tab_w, tab_h));
        if active {
            painter.rect_filled(this, 0.0, theme.bg_panel().to_egui());
            // active underline (accent, 2px).
            let bar = egui::Rect::from_min_size(
                egui::pos2(this.left(), this.bottom() - theme.focus_ring_width.value()),
                egui::vec2(this.width(), theme.focus_ring_width.value()),
            );
            painter.rect_filled(bar, 0.0, theme.accent_primary().to_egui());
        }
        painter.galley(
            egui::pos2(x + pad, tab_rect.center().y - galley.size().y / 2.0),
            galley,
            theme.text_primary().to_egui(),
        );
        x += tab_w;
        painter.vline(
            x,
            tab_rect.y_range(),
            egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        );
    }

    // 터미널 콘텐츠(디밍된 mono 텍스트) — 배너 뒤 컨텍스트.
    let content_pad = theme.spacing_md.value();
    painter.text(
        egui::pos2(rect.left() + content_pad, tab_rect.bottom() + content_pad),
        egui::Align2::LEFT_TOP,
        "~/tasty $ vim src/main.rs",
        egui::FontId::monospace(theme.font_size_term_sm.value()),
        dim(theme.text_muted().to_egui(), theme.opacity_recessed()),
    );

    // 배너 존 — 탭바 아래 8px, 양옆 8px margin, 하단 margin 없음.
    let margin = theme.spacing_sm.value();
    let zone = egui::Rect::from_min_max(
        egui::pos2(rect.left() + margin, tab_rect.bottom() + margin),
        egui::pos2(rect.right() - margin, rect.bottom() - margin),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(zone));
    banner(&mut child);
}

// ── Spec 1: shell + 예시(마우스 캡쳐 안내) ──────────────────────────────────
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        faux_scope(ui, theme, theme.measure_lg.value(), 240.0, |ui| {
            banner_shell(ui, theme, 1.0, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                    glyph(
                        ui,
                        icons::MOUSE,
                        theme.icon_glyph_size_md.value(),
                        theme.banner_icon_fg().to_egui(),
                    );
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                        title_line(ui, theme, "Mouse reporting captured your drag");
                        body_line(
                            ui,
                            theme,
                            "vim has mouse tracking on, so drag-to-select is disabled.",
                        );
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                            body_line(ui, theme, "Hold");
                            kbd(ui, theme, "Shift");
                            body_line(ui, theme, "to select anyway.");
                        });
                        ui.add_space(theme.spacing_xs.value());
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                            Button::new("Pause reporting")
                                .variant(ButtonVariant::Secondary)
                                .size(ControlSize::Sm)
                                .show(ui, theme);
                            Button::new("Don't show again")
                                .variant(ButtonVariant::Ghost)
                                .size(ControlSize::Sm)
                                .show(ui, theme);
                        });
                    });
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("family", "Modal · Popup · Toast · Banner"),
            ("position", "content-top, below the tab bar"),
            ("width", "100% − 16 (8 each side)"),
            ("margin", "8 top/sides · 0 bottom"),
            ("radius", "8 (banner-radius)"),
            ("fires on", "user action only (never IPC)"),
        ],
        &[
            TokenChip::new("banner-bg", "surface0 fill", theme.banner_bg().to_egui()),
            TokenChip::new("banner-border", "1px edge", theme.banner_border().to_egui()),
            TokenChip::new(
                "banner-icon-fg",
                "leading glyph",
                theme.banner_icon_fg().to_egui(),
            ),
        ],
    );

    spec::do_(
        ui,
        theme,
        "Reach for a Banner when there's an action to attach (Popup = independent \
         feature, Banner = info + action, Toast = info only). Short and action-less? Use a Toast.",
    );

    spec::note(
        ui,
        theme,
        "There is no Info/Warning/Error kind — each banner's id is its kind and styles \
         its own leading glyph. These tokens are only the shared shell. The × is hidden \
         until the banner is hovered.",
    );
}

// ── Spec 2: dismiss & TTL — plain(hover ×) / TTL(카운트다운) ──────────────────
pub fn draw_dismiss(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();

        // plain — hover 시 × 노출(여기선 노출 상태로 표시).
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            caption_label(ui, theme, "no TTL — hover the banner to reveal ×");
            ui.scope(|ui| {
                ui.set_max_width(theme.measure_md.value());
                banner_shell(ui, theme, 1.0, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                        body_secondary(ui, theme, "Plain banner — × appears on hover (top-right)");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            dismiss_x(ui, theme);
                        });
                    });
                });
            });
        });

        // TTL — 우상단 카운트다운 숫자(6초).
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            caption_label(
                ui,
                theme,
                "TTL 6s — live countdown · hover to pause + show ×",
            );
            ui.scope(|ui| {
                ui.set_max_width(theme.measure_md.value());
                banner_shell(ui, theme, 1.0, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                        glyph(
                            ui,
                            icons::CHECK,
                            theme.icon_glyph_size_md.value(),
                            theme.accent_success().to_egui(),
                        );
                        body_secondary(ui, theme, "Preset Dev split applied to this tab.");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            countdown(ui, theme, 6);
                        });
                    });
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("slot", "top-right · one affordance"),
            ("plain", "× hidden → reveal on hover"),
            ("TTL", "seconds count → × on hover"),
            ("expiry", "0 → auto-dismiss"),
            ("pause", "hover · backgrounded scope"),
            ("motion", "120ms alpha fade, no move"),
        ],
        &[
            TokenChip::new(
                "banner-countdown-fg",
                "seconds",
                theme.banner_countdown_fg().to_egui(),
            ),
            TokenChip::new(
                "accent-success",
                "per-banner glyph",
                theme.accent_success().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "The top-right corner holds one affordance. The countdown pauses while the banner \
         is hovered or its scope is backgrounded. Appear/dismiss is a 120ms alpha fade — \
         the banner never moves. (egui immediate-mode renders the end state.)",
    );
}

// ── Spec 3: queue & stacking — 상위(전면) + 하위(40% 디밍, 후면) ──────────────
pub fn draw_stack(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        let width = theme.measure_md.value();
        let height = 116.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

        // 하위 스코프(Pane) 배너 — 더 크고, 40% 로 디밍되어 뒤에. overhang 만 보인다.
        let lower = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + 14.0),
            egui::pos2(rect.right(), rect.bottom()),
        );
        let mut lower_ui = ui.new_child(egui::UiBuilder::new().max_rect(lower));
        banner_shell(&mut lower_ui, theme, theme.opacity_recessed(), |ui| {
            ui.set_min_height(84.0);
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                body_line(
                    ui,
                    theme,
                    "Pane banner — recessed to 40% opacity behind the higher banner; only its overhang shows",
                );
            });
        });

        // 상위 스코프(Workspace) 배너 — 전면, full opacity, 높은 z(나중에 그림).
        let upper = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.top() + 56.0));
        let mut upper_ui = ui.new_child(egui::UiBuilder::new().max_rect(upper));
        banner_shell(&mut upper_ui, theme, 1.0, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                glyph(
                    ui,
                    icons::ALERT_TRIANGLE,
                    theme.icon_glyph_size_md.value(),
                    theme.accent_warning().to_egui(),
                );
                body_secondary(
                    ui,
                    theme,
                    "Workspace banner — in front, full opacity, higher z-index",
                );
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("per scope", "1 shown + up to 5 queued"),
            ("dup (shown)", "resets the countdown"),
            ("dup (queued)", "ignored"),
            ("queue full", "new banner dropped"),
            ("z-order", "View > Workspace > Pane > Tab > Surface"),
            ("recessed", "lower scope → 40% opacity"),
        ],
        &[
            TokenChip::new(
                "banner-recessed-opacity",
                "dimmed lower",
                dim(theme.banner_bg().to_egui(), theme.opacity_recessed()),
            ),
            TokenChip::new("banner-bg", "both shells", theme.banner_bg().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "A scope shows one banner; others queue (max 5). Across scopes the higher one \
         (View > Workspace > Pane > Tab > Surface) sits in front at a higher z-index, and \
         the lower one is dimmed to ~40% opacity behind it. The interactive manager lives \
         in the kit specimen ui_kits/terminal/overlays/banner.html.",
    );
}

/// 데모 라벨 — caption(11), text-muted (스테이지 내 상태 주석).
fn caption_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// 단일 줄 본문 — body(13), text-secondary, 남는 폭을 채운다.
fn body_secondary(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_body.value())
            .color(theme.text_secondary().to_egui()),
    );
}
