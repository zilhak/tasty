//! Banner — 디자인(4) Overlays `banner` Section (3 Spec). 네 번째 overlay 패밀리
//! (Modal / Popup / Toast / **Banner**).
//!
//! 스코프 콘텐츠 영역 최상단(탭바 바로 아래 8px)에 떠 있는 focus-less 공지.
//! Toast 와 달리 **자기 마우스를 소비**하고 **action 을 실을 수 있다**. 선택적 TTL
//! 카운트다운(우상단)이 hover 시 × 로 전환된다. 스코프당 1개만 표시, 나머지는 큐(≤5).
//!
//! 본 specimen 은 디자인 `gallery/overlays-banners.jsx` 의 Spec 구조를 전사한다:
//! 1. **shell + 예시** — 캐노니컬 마우스 캡쳐 배너(mouse glyph + 제목 + Shift 우회 힌트).
//!    persistent(버튼 없음); 우상단 ×는 hover 시에만. 옛 Pause/Don't-show 버튼 단은 폐기.
//! 2. **dismiss & TTL** — plain(hover × 노출) / TTL(우상단 카운트다운) 두 상태.
//! 3. **queue & stacking** — 상위 스코프 배너(전면) + 하위 스코프 배너(40% 디밍, 후면).
//! 4. **position & hit-zone** — 카드 rect 만 마우스 소비, 그 아래 surface 본문은 pass-through.
//! 5. **capture blacklist** — Settings › Terminal 의 행 리스트 에디터(filled / empty).
//!
//! 색·치수·폰트는 모두 `Theme` 토큰 경유(`from_rgb`/hex 리터럴 금지). 배너 전용
//! Tier-3 토큰은 banner-03 에서 본체 Theme 에 도입되어, 이 specimen 도 근사 없이
//! 토큰 접근자를 직접 쓴다: 디밍 = `opacity_recessed()`(0.4), 라운드 = `corner_radius_lg`
//! (radius-8), 그림자 = `shadow_popover()`. semantic 매핑(`banner_bg` → surface-raised 등)도
//! 전용 접근자(`banner_bg()`/`banner_border()`/`banner_icon_fg()`/`banner_countdown_fg()`)로
//! 노출된다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Input, kbd, switch,
};

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

/// mouse-capture 배너 "더보기"(⋯) 트리거 상태 — Spec 6 (더보기 컨텍스트 메뉴).
#[derive(Clone, Copy, PartialEq, Eq)]
enum MoreTriggerState {
    /// hover 전 — ⋯/× 둘 다 숨김(폭은 예약된 채 비어 있음).
    Hidden,
    /// hover 중 — ⋯ + × 둘 다 노출(⋯가 왼쪽, 4px gap).
    Hovered,
    /// 메뉴가 열려 있음 — ⋯ 는 hover 여부와 무관하게 계속 표시 + active 강조.
    Open,
}

/// 우상단 "더보기"(⋯) 트리거. sm IconButton, `active` 면 icon-button-bg-active 강조.
fn more_trigger(ui: &mut egui::Ui, theme: &Theme, active: bool) {
    IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .active(active)
        .show(ui, theme, &|ui, rect, c| {
            icons::MORE.image(rect.height(), c).paint_at(ui, rect)
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

/// faux surface chrome — #000 배경 + 탭 스트립(마지막 탭 active) + 선택적 상단
/// 디밍 콘텐츠 줄 / 하단 pass-through 주석. 반환값은 배너 존 rect(탭바 아래 8px,
/// 양옆 8px margin). 셸/배너는 호출측이 이 rect 위에 그린다. (specimen 전사 치수:
/// 탭 높이 28 은 디자인 tab strip 고정치.)
fn faux_chrome(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    height: f32,
    tabs: [&str; 3],
    top_line: Option<&str>,
    bottom_note: Option<&str>,
) -> egui::Rect {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let radius = theme.corner_radius.value();
    let painter = ui.painter_at(rect);

    // 터미널 surface 배경(#000 = ansi_black).
    painter.rect_filled(rect, radius, theme.ansi_black.to_egui());

    // 탭 스트립(28px, bg-sidebar) — 마지막 탭 active.
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

    let content_pad = theme.spacing_md.value();
    // 상단 터미널 콘텐츠(디밍된 mono 텍스트) — 배너 뒤 컨텍스트.
    if let Some(line) = top_line {
        painter.text(
            egui::pos2(rect.left() + content_pad, tab_rect.bottom() + content_pad),
            egui::Align2::LEFT_TOP,
            line,
            egui::FontId::monospace(theme.font_size_term_sm.value()),
            dim(theme.text_muted().to_egui(), theme.opacity_recessed()),
        );
    }
    // 하단 pass-through 주석(hit-zone) — 카드 아래는 앱으로 전달됨을 표기.
    if let Some(note) = bottom_note {
        painter.text(
            egui::pos2(rect.left() + content_pad, rect.bottom() - content_pad),
            egui::Align2::LEFT_BOTTOM,
            note,
            egui::FontId::monospace(theme.font_size_caption.value()),
            theme.text_disabled().to_egui(),
        );
    }

    // 배너 존 — 탭바 아래 8px, 양옆 8px margin, 하단 margin 없음.
    let margin = theme.spacing_sm.value();
    egui::Rect::from_min_max(
        egui::pos2(rect.left() + margin, tab_rect.bottom() + margin),
        egui::pos2(rect.right() - margin, rect.bottom() - margin),
    )
}

/// faux scope + 배너를 존에 그린다(Spec 1 용 기본 vim 스코프).
fn faux_scope(
    ui: &mut egui::Ui,
    theme: &Theme,
    width: f32,
    height: f32,
    banner: impl FnOnce(&mut egui::Ui),
) {
    let zone = faux_chrome(
        ui,
        theme,
        width,
        height,
        ["server", "dev", "vim"],
        Some("~/tasty $ vim src/main.rs"),
        None,
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(zone));
    banner(&mut child);
}

/// 캐노니컬 마우스 캡쳐 배너 본문 — leading mouse 글리프 + 제목 + Shift 우회 힌트.
/// persistent(action 버튼 없음); 우상단 ×는 hover 시에만. `more` 는 "더보기" ⋯
/// 트리거의 상태(닫힘/hover/열림) — mouse-capture 배너는 ⋯ 몫까지 항상 2 슬롯을
/// 예약한다(hover 전환으로 본문 폭이 흔들리지 않도록). 디자인
/// `MouseCaptureBannerG`(overlays-shared.jsx) 전사 + 더보기 확장(design-spec-more-menu).
fn mouse_capture_banner(ui: &mut egui::Ui, theme: &Theme, more: MoreTriggerState) {
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
        glyph(
            ui,
            icons::MOUSE,
            theme.icon_glyph_size_md.value(),
            theme.banner_icon_fg().to_egui(),
        );
        // 본문 컬럼 — 우상단 ⋯/× 자리(2×item_height)를 비워두고 남는 폭을 채운다.
        let body_w = (ui.available_width()
            - theme.item_height_interactive.value() * 2.0
            - theme.spacing_md.value())
        .max(0.0);
        ui.vertical(|ui| {
            ui.set_width(body_w);
            ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
            title_line(ui, theme, "Mouse input captured");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
                body_line(ui, theme, "This app is capturing the mouse. Hold");
                kbd(ui, theme, "Shift");
                body_line(ui, theme, "+drag to select text,");
                kbd(ui, theme, "Shift");
                body_line(ui, theme, "+Right-click for the tasty menu.");
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
            if more != MoreTriggerState::Hidden {
                dismiss_x(ui, theme);
                more_trigger(ui, theme, more == MoreTriggerState::Open);
            }
        });
    });
}

// ── Spec 1: shell + 예시(캐노니컬 마우스 캡쳐 배너) ──────────────────────────
pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        faux_scope(ui, theme, theme.measure_lg.value(), 200.0, |ui| {
            banner_shell(ui, theme, 1.0, |ui| {
                mouse_capture_banner(ui, theme, MoreTriggerState::Hovered);
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
            ("persistent", "no TTL · × on hover only"),
            ("fires on", "user click only (never IPC)"),
        ],
        &[
            TokenChip::new("banner-bg", "surface0 fill", theme.banner_bg().to_egui()),
            TokenChip::new("banner-border", "1px edge", theme.banner_border().to_egui()),
            TokenChip::new(
                "banner-icon-fg",
                "mouse glyph",
                theme.banner_icon_fg().to_egui(),
            ),
        ],
    );

    spec::do_(
        ui,
        theme,
        "Reach for a Banner when an action could attach (Popup = independent feature, \
         Banner = info ± action, Toast = info only). This first instance is action-less: \
         the bypass keys (Shift+drag / Shift+Right-click) are the message.",
    );

    spec::note(
        ui,
        theme,
        "There is no Info/Warning/Error kind — each banner's id is its kind and styles \
         its own leading glyph. The mouse-capture banner is persistent (no TTL / countdown); \
         its interactive elements are the × and the ⋯ \"more\" trigger, both hover-revealed \
         (see the banner-more-menu spec for the ⋯ context menu).",
    );
}

// ── Spec 4: position & hit-zone — 카드만 소비, 본문은 pass-through ─────────────
pub fn draw_hit_zone(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        let zone = faux_chrome(
            ui,
            theme,
            theme.measure_lg.value(),
            220.0,
            ["server", "dev", "htop"],
            None,
            Some("surface body below the card — pass-through; clicks/drag reach the app"),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(zone));
        banner_shell(&mut child, theme, 1.0, |ui| {
            mouse_capture_banner(ui, theme, MoreTriggerState::Hovered);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("anchor", "surface content top, under tab bar"),
            ("margins", "8 top / 8 sides · 0 bottom"),
            ("consume zone", "the drawn card rect only"),
            ("below card", "pass-through to the app"),
            ("focus", "never steals keyboard focus"),
            ("inactive surface", "first click only focuses it"),
        ],
        &[
            TokenChip::new("banner-margin", "8px gap", theme.banner_bg().to_egui()),
            TokenChip::new(
                "accent-info",
                "consume-zone label",
                theme.accent_info().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "The card rect — not the whole scope — is the mouse-consume + hover zone. Clicks on \
         the surface body below the card pass through to the capturing app, so mouse reporting \
         keeps working everywhere except the card itself.",
    );
}

// ── Spec 5: capture blacklist — Settings › Terminal 행 리스트 에디터 ──────────
pub fn draw_blacklist(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            caption_label(ui, theme, "filled · one row hovered");
            blacklist_editor(ui, theme, false);
        });
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            caption_label(ui, theme, "empty (default) · Add disabled");
            blacklist_editor(ui, theme, true);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("lives in", "Settings › Terminal"),
            ("group", "hint switch + blacklist"),
            ("row", "pattern (mono) + × remove"),
            ("add", "Input + Add (disabled when empty)"),
            ("match", "case-insensitive substring or *"),
            ("default", "empty list"),
        ],
        &[
            TokenChip::new(
                "overlay-hover",
                "row hover",
                theme.overlay_hover().to_egui(),
            ),
            TokenChip::new(
                "accent-warning",
                "match-rule notice",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new("input-bg", "add field", theme.surface_raised().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "Promoted from the old newline textarea to a list editor. The wheel is always \
         forwarded to the program even for blacklisted apps — only clicks/drags are \
         intercepted. A blacklisted foreground app also suppresses the capture banner there.",
    );
}

/// 블랙리스트 한 행 — 패턴(mono) + 우측 × 제거. hover 행은 overlay-hover 배경.
fn blacklist_row(ui: &mut egui::Ui, theme: &Theme, pattern: &str, hover: bool) {
    let fill = if hover {
        theme.overlay_hover().to_egui()
    } else {
        egui::Color32::TRANSPARENT
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(theme.corner_radius_sm.value())
        .inner_margin(egui::Margin {
            left: theme.spacing_sm.value() as i8,
            right: theme.spacing_xs.value() as i8,
            top: 0,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(pattern)
                        .monospace()
                        .size(theme.font_size_caption.value())
                        .color(theme.text_primary().to_egui()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    dismiss_x(ui, theme);
                });
            });
        });
}

/// 블랙리스트 에디터 카드 — hint 스위치 + 행 리스트(또는 빈 상태) + Add 행 + notice.
/// 디자인 `BlacklistEditorG`(overlays-shared.jsx) 전사. `empty` 면 빈 상태(neutral
/// 톤) + Add 버튼 disabled.
fn blacklist_editor(ui: &mut egui::Ui, theme: &Theme, empty: bool) {
    egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.set_width(theme.measure_sm.value());
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();

            // 섹션 라벨 — mono micro uppercase muted.
            ui.label(
                egui::RichText::new("MOUSE CAPTURE")
                    .monospace()
                    .size(theme.font_size_micro.value())
                    .color(theme.text_muted().to_egui()),
            );

            // hint 스위치 행.
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Show mouse-capture hint")
                        .size(theme.font_size_body.value())
                        .color(theme.text_secondary().to_egui()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut on = true;
                    switch(ui, theme, &mut on, None, true);
                });
            });

            // separator (1px).
            let (sep, _) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), theme.border_width.value()),
                egui::Sense::hover(),
            );
            ui.painter()
                .rect_filled(sep, 0.0, theme.separator.to_egui());

            ui.label(
                egui::RichText::new("Disable capture for these programs")
                    .size(theme.font_size_body.value())
                    .color(theme.text_secondary().to_egui()),
            );

            if empty {
                // 빈 상태 — neutral 톤.
                ui.label(
                    egui::RichText::new(
                        "No programs excluded — clicks are sent to capturing apps.",
                    )
                    .size(theme.font_size_caption.value())
                    .color(theme.text_muted().to_egui()),
                );
            } else {
                ui.scope(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    blacklist_row(ui, theme, "htop", false);
                    blacklist_row(ui, theme, "vim", true);
                    blacklist_row(ui, theme, "ht*", false);
                });
            }

            // Add 행 — 우측 Add 버튼(empty 면 disabled) + 남는 폭 Input.
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    Button::new("Add")
                        .variant(ButtonVariant::Secondary)
                        .size(ControlSize::Sm)
                        .enabled(!empty)
                        .show(ui, theme);
                    let mut buf = String::new();
                    Input::new()
                        .placeholder("process name or pattern, e.g. htop or ht*")
                        .mono(true)
                        .show(ui, theme, &mut buf);
                });
            });

            // match-rule notice — accent-warning(빈 상태 neutral 과 톤 구분).
            ui.label(
                egui::RichText::new(
                    "Case-insensitive substring or * wildcard on the process name. \
                     When a listed program is foreground, clicks/drags are handled locally; \
                     the wheel is still sent.",
                )
                .size(theme.font_size_caption.value())
                .color(theme.accent_warning().to_egui()),
            );
        });
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

// ── Spec 6: "더보기"(⋯) 컨텍스트 메뉴 — trigger 3상태 + 메뉴 ──────────────────
pub fn draw_more_menu(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();

        // 트리거 상태 3종 — 닫힘(hover 전) / hover(⋯+× 둘 다 노출) / 메뉴 열림(active).
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
            for (state, caption) in [
                (MoreTriggerState::Hidden, "closed — hover to reveal"),
                (MoreTriggerState::Hovered, "hover — ⋯ left of ×, 4px gap"),
                (MoreTriggerState::Open, "menu open — ⋯ stays + active tint"),
            ] {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                    caption_label(ui, theme, caption);
                    ui.scope(|ui| {
                        ui.set_max_width(theme.measure_md.value());
                        banner_shell(ui, theme, 1.0, |ui| {
                            mouse_capture_banner(ui, theme, state);
                        });
                    });
                });
            }
        });

        // 인터랙티브 컨텍스트 메뉴 — 실제 앵커(트리거 아래 4px, 우측 정렬)를 근사.
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            caption_label(
                ui,
                theme,
                "menu — anchored 4px below the trigger, right-aligned",
            );
            mouse_capture_menu(ui, theme, "vim", 240.0);
        });

        // 가변폭 예시 — min-width(200)에서 긴 프로그램 이름이 ellipsis 되는지.
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            caption_label(
                ui,
                theme,
                "min-width (200px) — the app segment ellipsizes, the fixed label text never does",
            );
            mouse_capture_menu(ui, theme, "a-very-long-tui-program-name", 200.0);
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("trigger", "24px Ghost IconButton · 3-dot `more` icon"),
            ("order", "⋯ left of × · 4px gap between"),
            ("reserve", "56px (2×24 + gap4 + gap4) — always reserved"),
            (
                "anchor",
                "trigger bottom +4px · right-aligned · flips up if clipped",
            ),
            ("menu size", "200–288px wide · 4px inner padding"),
            (
                "items",
                "suppress banner (bell) · disable capture (mouse) — both neutral",
            ),
        ],
        &[
            TokenChip::new(
                "menu-item-bg-hover",
                "row hover",
                theme.menu_item_bg_hover().to_egui(),
            ),
            TokenChip::new(
                "icon-button-bg-active",
                "⋯ active tint",
                theme.icon_button_bg_active().to_egui(),
            ),
            TokenChip::new(
                "border-strong",
                "menu edge",
                theme.border_strong().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "The ⋯ trigger shows on hover (same condition as ×) and stays + active-tints while \
         its menu is open — re-click ⋯ to reuse. Both menu items run immediately and close \
         the menu; neither is danger-toned (both are reversible from Settings › Terminal). \
         Suppressing the banner also closes it immediately; disabling capture leaves it open \
         so the user can read the confirmation. The program name renders as its own mono \
         segment so only it ellipsizes — the fixed label text never wraps or truncates.",
    );
}

/// 마우스 캡처 배너 "더보기" 컨텍스트 메뉴 카드 — surface-raised + border-strong +
/// popover shadow (Tools menu 와 동일 셸 토큰). 두 항목 고정(순서 고정): 배너
/// 억제(bell) / 캡처 비활성화(mouse). `width` 로 min/max 폭 케이스를 시연한다.
fn mouse_capture_menu(ui: &mut egui::Ui, theme: &Theme, app: &str, width: f32) {
    egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_strong().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .shadow(theme.shadow_popover().to_egui())
        .inner_margin(egui::Margin::same(theme.spacing_xs.value() as i8))
        .show(ui, |ui| {
            ui.set_width(width);
            ui.spacing_mut().item_spacing.y = 0.0;
            mouse_capture_menu_row(
                ui,
                theme,
                icons::BELL,
                "Turn off this notification for ",
                app,
                "",
                true,
            );
            mouse_capture_menu_row(
                ui,
                theme,
                icons::MOUSE,
                "Disable mouse capture for ",
                app,
                "",
                false,
            );
        });
}

/// 메뉴 한 줄 — icon + [prefix][app(mono, 강조 + ellipsis)][suffix]. 본체 구현
/// (`mouse_capture_menu.rs`)과 동일 원칙 — 고정 라벨 텍스트는 줄바꿈/truncate
/// 없이, 프로그램 이름 세그먼트만 독립적으로 축소+ellipsis 된다.
fn mouse_capture_menu_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: MockGlyph,
    prefix: &str,
    app: &str,
    suffix: &str,
    hovered: bool,
) {
    let height = theme.menu_item_height().value();
    let pad_x = theme.menu_item_padding_x().value();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::hover(),
    );
    if hovered {
        ui.painter().rect_filled(
            rect,
            theme.menu_item_radius().value(),
            theme.menu_item_bg_hover().to_egui_premultiplied(),
        );
    }
    let icon_glyph = theme.icon_glyph_size_md.value();
    let gap = theme.spacing_sm.value();
    let mut x = rect.left() + pad_x;
    let irect = egui::Rect::from_center_size(
        egui::pos2(x + icon_glyph * 0.5, rect.center().y),
        egui::vec2(icon_glyph, icon_glyph),
    );
    icon.image(icon_glyph, theme.text_muted().to_egui())
        .paint_at(ui, irect);
    x += icon_glyph + gap;

    let label_rect = egui::Rect::from_min_max(
        egui::pos2(x, rect.top()),
        egui::pos2(rect.right() - pad_x, rect.bottom()),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(label_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 0.0;
    let fg = theme.text_primary().to_egui();
    if !prefix.is_empty() {
        child.label(
            egui::RichText::new(prefix)
                .size(theme.font_size_body.value())
                .color(fg),
        );
    }
    child.add(
        egui::Label::new(
            egui::RichText::new(app)
                .monospace()
                .size(theme.font_size_body.value())
                .color(fg),
        )
        .truncate(),
    );
    if !suffix.is_empty() {
        child.label(
            egui::RichText::new(suffix)
                .size(theme.font_size_body.value())
                .color(fg),
        );
    }
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
