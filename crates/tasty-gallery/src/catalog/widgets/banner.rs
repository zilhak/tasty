//! 탭바 아래에 표시하는 배너의 외형·닫기 버튼·메뉴·겹침 예제.
//! 본체는 스코프당 한 개를 표시하고 최대 다섯 개를 대기시킨다. 이 예제는 큐를 실행하지 않는다.
//! 카운트다운도 6초로 고정해 그리며 호버·만료·키보드 포커스 처리는 재현하지 않는다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{
    Button, ButtonVariant, ControlSize, IconButton, IconButtonVariant, Input, kbd, switch,
};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 후면 배너를 아래로 내려 겹침을 드러내는 전시용 오프셋. 텍스트 크기와 무관하다.
const STACK_REAR_OVERHANG_Y: LogicalPx = LogicalPx(14.0);
/// 전면 배너 child 영역의 높이. 뒤쪽 배너의 아래 부분이 남도록 정한 데모 기하다.
const STACK_FRONT_BANNER_H: LogicalPx = LogicalPx(56.0);

fn dim(color: egui::Color32, opacity: f32) -> egui::Color32 {
    color.gamma_multiply(opacity)
}

/// 배너 프레임과 안쪽 콘텐츠를 그린다. opacity는 배경·테두리·그림자에 곱한다.
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

/// 마우스 캡처 배너의 더보기 버튼 표시 상태.
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

/// 남은 시간을 고정폭 글꼴로 표시한다.
fn countdown(ui: &mut egui::Ui, theme: &Theme, seconds: u32) {
    ui.label(
        egui::RichText::new(seconds.to_string())
            .monospace()
            .size(theme.font_size_micro.value())
            .color(theme.banner_countdown_fg().to_egui()),
    );
}

/// 탭바를 덮지 않는 배너 영역을 반환한다. 탭 높이는 이 예제의 디자인 값이다.
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

    painter.rect_filled(rect, radius, theme.ansi_black.to_egui());

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
            let bar = egui::Rect::from_min_size(
                egui::pos2(
                    this.left(),
                    this.bottom() - theme.tab_indicator_width.value(),
                ),
                egui::vec2(this.width(), theme.tab_indicator_width.value()),
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
    if let Some(line) = top_line {
        painter.text(
            egui::pos2(rect.left() + content_pad, tab_rect.bottom() + content_pad),
            egui::Align2::LEFT_TOP,
            line,
            egui::FontId::monospace(theme.font_size_term_sm.value()),
            dim(theme.text_muted().to_egui(), theme.opacity_recessed()),
        );
    }
    if let Some(note) = bottom_note {
        painter.text(
            egui::pos2(rect.left() + content_pad, rect.bottom() - content_pad),
            egui::Align2::LEFT_BOTTOM,
            note,
            egui::FontId::monospace(theme.font_size_caption.value()),
            theme.text_disabled().to_egui(),
        );
    }

    let margin = theme.spacing_sm.value();
    egui::Rect::from_min_max(
        egui::pos2(rect.left() + margin, tab_rect.bottom() + margin),
        egui::pos2(rect.right() - margin, rect.bottom() - margin),
    )
}

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

/// 더보기와 닫기 버튼의 폭을 항상 확보해 호버 전후에 본문 폭이 바뀌지 않게 한다.
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
        "The exclusion list keeps wheel events forwarded to the program while handling clicks and drags locally. A matching foreground program also suppresses the mouse-capture banner.",
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

/// 마우스 캡처 제외 목록 설정의 채운 상태와 빈 상태를 비교한다.
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

            ui.label(
                egui::RichText::new("MOUSE CAPTURE")
                    .monospace()
                    .size(theme.font_size_micro.value())
                    .color(theme.text_muted().to_egui()),
            );

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
pub fn draw_dismiss(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            caption_label(ui, theme, "no TTL — close button shown");
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

        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            caption_label(ui, theme, "TTL — static 6s countdown example");
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
        "The host pauses the countdown while the banner is hovered or its scope is backgrounded and uses a 120ms alpha fade. This example only paints the close button and a fixed countdown; it does not run the timer or fade.",
    );
}
pub fn draw_stack(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Solo, |ui| {
        let width = theme.measure_md.value();
        let height = 116.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());

        let lower = egui::Rect::from_min_max(
            egui::pos2(rect.left(), rect.top() + STACK_REAR_OVERHANG_Y.value()),
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

        let upper = egui::Rect::from_min_max(
            rect.min,
            egui::pos2(rect.right(), rect.top() + STACK_FRONT_BANNER_H.value()),
        );
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
        "The host displays one banner per scope and queues up to five more. Higher scopes appear in front (View > Workspace > Pane > Tab > Surface); lower banners use 40% opacity. This example paints the two layers without running the queue.",
    );
}
pub fn draw_more_menu(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Column, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_lg.value();

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

        // 실제 메뉴의 위치를 정적으로 보여준다.
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
            caption_label(
                ui,
                theme,
                "menu — anchored 4px below the trigger, right-aligned",
            );
            mouse_capture_menu(ui, theme, "vim", 240.0);
        });

        // 고정 라벨을 유지하면서 긴 프로그램 이름만 줄이는지 비교한다.
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

/// 배너 알림 끄기와 마우스 캡처 끄기 메뉴의 고정 순서·가변 폭 예제.
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

/// 고정 라벨은 유지하고 프로그램 이름만 줄인다. 본체 mouse_capture_menu와 같은 배치 원칙이다.
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
