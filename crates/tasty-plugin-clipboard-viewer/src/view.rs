//! egui-mesh popup 콘텐츠 렌더 — header → type-bar → body → footer 4단 구조
//! (design-system `overlays/clipboard_viewer.jsx` 구조 전사, TODO51).
//!
//! rail(세로 타입 목록)은 폐기됐다 — 디자인 원칙: 단일 타입(현재의 유일한 실제
//! 케이스, Text)은 헤더 아래 뱃지로, 복수 타입은 가로 세그먼트 스위치([`type_switch`])로
//! 표현한다. `SEG_COMPACT_AT`(5) 이상이면 비활성 세그먼트를 아이콘 전용으로
//! 압축한다. 이 TODO는 `ClipboardType::Text` 하나만 실제로 채운다 — Image/Html/
//! Other/Files 는 자매 TODO(48/49/50/52)가 `ClipboardType`에 arm 을 추가하며
//! [`type_icon`]/[`type_body`]/[`footer_mime_text`] 에도 갈래를 보탠다(51 은 골격만).
//!
//! chrome(scrim/border/outside-click/Esc)은 host 소유 — plugin 은 content 영역만
//! 그린다. 색·폰트·간격은 전부 host 가 보낸 `Theme` 토큰에서 가져온다(from_rgb/raw
//! px 금지). 헤더/푸터의 Close 버튼은 host chrome 의 outside-click/Esc 와 기능이
//! 중복되지만 디자인이 명시적으로 요구해 그대로 반영한다 — 클릭 시 `draw`/
//! `draw_already_open` 이 `true` 를 반환하고, 호출부(`main.rs`)가 `popup.close` IPC 로
//! host 에 닫기를 요청한다(host 가 chrome 생애주기를 계속 소유).

// IMAGE/FILE/HTML/LAYERS 는 이 TODO(51)가 아직 안 쓴다 — 자매 TODO(48/49/50/52)가
// ClipboardType 에 arm 을 추가하며 소비한다. build.rs 가 9개를 한꺼번에 베이크해
// 그 TODO들이 build.rs 를 다시 건드릴 필요가 없게 해둔 의도된 상태.
#[allow(dead_code)]
mod baked_icons {
    include!(concat!(env!("OUT_DIR"), "/plugin_icons.rs"));
}

use tasty_plugin_sdk::{Translator, baked_icon};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, IconButton, TagVariant, tag};

use crate::ViewerState;
use crate::clipboard::{ClipboardType, ContentRepr};

/// 세그먼트가 5개 이상이면 비활성 세그먼트를 아이콘 전용으로 압축한다(design
/// `SEG_COMPACT_AT`).
const SEG_COMPACT_AT: usize = 5;

/// 헤더/타입바/푸터 공통 좌우 인셋. design `var(--tasty-size-14)` 근사 — Theme 에
/// 14px 전용 토큰이 없어 4px 그리드의 `spacing_md`(12)로 매핑한다.
fn row_pad_x(theme: &Theme) -> f32 {
    theme.spacing_md.value()
}

/// 아이콘 버튼(IconButton/Button leading_icon) 안에서 아이콘이 버튼 높이 대비 차지할
/// 비율(`tasty-plugin-image` 정본 튜닝값 재사용).
const ICON_DRAW_RATIO: f32 = 0.7;

/// CenterState 아이콘 크기(design 고정값 28 — Theme 아이콘 글리프 토큰은 16 상한이라
/// 화면 전용 고정값으로 둔다, 기존 gallery specimen 의 480×360 선례와 동일 정책).
const CENTER_ICON_SIZE: f32 = 28.0;

/// 주 인스턴스 popup 본문. 헤더는 항상 그리고, 그 아래는 read_error / empty / data
/// 3분기(design `dataState`/`snap.status` 동형). 헤더·푸터의 Close 클릭 시 `true`.
pub fn draw(ctx: &egui::Context, theme: &Theme, state: &mut ViewerState, tr: &Translator) -> bool {
    let mut close = false;
    panel(ctx, theme, |ui| {
        header(ui, theme, tr, &mut close);
        if state.read_error.is_some() {
            center_state(
                ui,
                theme,
                baked_icons::ALERT_TRIANGLE,
                tr.t("clipboard_viewer.popup.read_failed_title"),
                Some(tr.t("clipboard_viewer.popup.read_failed_sub")),
                true,
            );
        } else if state.available.is_empty() {
            center_state(
                ui,
                theme,
                baked_icons::CLIPBOARD,
                tr.t("clipboard_viewer.popup.empty_title"),
                Some(tr.t("clipboard_viewer.popup.empty_sub")),
                false,
            );
        } else {
            data_state(ui, theme, state, tr, &mut close);
        }
    });
    close
}

/// 단일 인스턴스 가드 placeholder — 헤더 + "이미 열림" CenterState(기존
/// `already_open_tree` 동형).
pub fn draw_already_open(ctx: &egui::Context, theme: &Theme, tr: &Translator) -> bool {
    let mut close = false;
    panel(ctx, theme, |ui| {
        header(ui, theme, tr, &mut close);
        center_state(
            ui,
            theme,
            baked_icons::LOCK,
            tr.t("clipboard_viewer.popup.already_open_title"),
            Some(tr.t("clipboard_viewer.popup.already_open_sub")),
            false,
        );
    });
    close
}

/// popup content 영역을 채우는 CentralPanel. host 셸이 그린 `bg_panel` 과 이음매 없게
/// 동일 토큰으로 채운다. 4행(header/type-bar/body/footer)이 여백 없이 맞닿으므로
/// item_spacing 을 0 으로 죽인다(각 행이 자기 padding 을 직접 계산).
fn panel(ctx: &egui::Context, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    let frame = egui::Frame::new()
        .fill(theme.bg_panel().to_egui())
        .inner_margin(egui::Margin::ZERO);
    egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
        add(ui);
    });
}

// ── header: 클립보드 아이콘 + "Clipboard" + snapshot 뱃지 + 우측 close ──
fn header(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, close: &mut bool) {
    let full_w = ui.available_width();
    let pad_x = row_pad_x(theme);
    let pad_y = theme.spacing_md.value();
    let ctrl_h = ControlSize::Sm.height(theme);
    let h = pad_y * 2.0 + ctrl_h;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(full_w, h), egui::Sense::hover());

    let content = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad_x, rect.top()),
        egui::pos2(rect.right() - pad_x, rect.bottom()),
    );
    let mut lui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    lui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    icon_glyph(
        &mut lui,
        baked_icons::CLIPBOARD,
        theme.icon_glyph_size_md.value(),
        theme.text_muted().to_egui(),
    );
    lui.label(
        egui::RichText::new(tr.t("clipboard_viewer.popup.header"))
            .size(theme.font_size_max.value())
            .strong()
            .color(theme.text_primary().to_egui()),
    );
    tag(
        &mut lui,
        theme,
        tr.t("clipboard_viewer.popup.snapshot_badge"),
        TagVariant::Default,
        false,
    );

    let close_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + pad_y),
        egui::pos2(rect.right() - pad_x, rect.top() + pad_y + ctrl_h),
    );
    let mut rui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(close_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let resp = IconButton::new()
        .size(ControlSize::Sm)
        .show(&mut rui, theme, &|ui, r, c| {
            icon_in_button(ui, baked_icons::CLOSE, r, c);
        })
        .on_hover_text(tr.t("clipboard_viewer.popup.close_tooltip"));
    if resp.clicked() {
        *close = true;
    }

    bottom_separator(ui, theme, rect);
}

/// 타입 있음 — type-bar + body + footer 3행(design `dataState`).
fn data_state(
    ui: &mut egui::Ui,
    theme: &Theme,
    state: &mut ViewerState,
    tr: &Translator,
    close: &mut bool,
) {
    let types: Vec<ClipboardType> = state.available.iter().map(|(t, _)| *t).collect();
    let active = state
        .selected
        .filter(|s| types.contains(s))
        .unwrap_or(types[0]);

    // 우측 슬롯 — 이 TODO는 Text 뿐이라 메타가 없다(빈 클로저). TODO49가 html 타입일
    // 때 이 자리에 Pretty print 체크박스를 그리는 다른 클로저를 넘기면 된다 —
    // `type_bar` 시그니처는 바뀌지 않는다.
    let picked = type_bar(ui, theme, tr, &types, active, |_ui, _theme| {});
    if let Some(picked) = picked {
        state.selected = Some(picked);
    }
    let active = picked.unwrap_or(active);

    let cur = state
        .available
        .iter()
        .find(|(t, _)| *t == active)
        .map(|(t, c)| (*t, c.clone()));

    let footer_h = theme.spacing_sm.value() * 2.0 + ControlSize::Sm.height(theme);
    let body_h = (ui.available_height() - footer_h).max(0.0);
    let full_w = ui.available_width();
    let (body_rect, _) = ui.allocate_exact_size(egui::vec2(full_w, body_h), egui::Sense::hover());
    let mut bui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(body_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    if let Some((ty, content)) = &cur {
        type_body(&mut bui, theme, *ty, content);
    }

    footer(ui, theme, tr, cur.as_ref().map(|(t, _)| *t), close);
}

/// design `TypeSwitch` — 1개면 아이콘+뱃지(읽기전용), 2개 이상이면 가로 세그먼트
/// 버튼 그룹(rail 재도입 금지). `SEG_COMPACT_AT` 이상이면 비활성 세그먼트를 아이콘
/// 전용으로 압축(active 만 라벨 유지) + `.on_hover_text()`로 전체 타입명 노출.
fn type_switch(
    ui: &mut egui::Ui,
    theme: &Theme,
    tr: &Translator,
    types: &[ClipboardType],
    active: ClipboardType,
) -> Option<ClipboardType> {
    if types.len() <= 1 {
        let ty = types.first().copied().unwrap_or(active);
        icon_glyph(
            ui,
            type_icon(ty),
            theme.icon_glyph_size_sm.value(),
            theme.text_muted().to_egui(),
        );
        tag(
            ui,
            theme,
            tr.t(ty.label_i18n_key()),
            TagVariant::Accent,
            false,
        );
        return None;
    }

    let compact = types.len() >= SEG_COMPACT_AT;
    let h = ControlSize::Sm.height(theme);
    let icon_sz = theme.icon_glyph_size_xs.value();
    let font = egui::FontId::proportional(theme.font_size_term_sm.value());
    let pad_x = theme.spacing_sm.value();
    let gap = theme.spacing_xs.value();
    let mut picked = None;

    egui::Frame::new()
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::ZERO)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.horizontal(|ui| {
                for (i, ty) in types.iter().copied().enumerate() {
                    let on = ty == active;
                    let show_label = seg_shows_label(compact, on);
                    let label = tr.t(ty.label_i18n_key());
                    let label_w = if show_label {
                        ui.fonts(|f| {
                            f.layout_no_wrap(
                                label.to_owned(),
                                font.clone(),
                                egui::Color32::PLACEHOLDER,
                            )
                        })
                        .size()
                        .x
                    } else {
                        0.0
                    };
                    let row_gap = if show_label { gap } else { 0.0 };
                    let seg_w = pad_x * 2.0 + icon_sz + row_gap + label_w;
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(seg_w, h), egui::Sense::click());

                    if i > 0 {
                        ui.painter().vline(
                            rect.left(),
                            rect.y_range(),
                            egui::Stroke::new(
                                theme.border_width.value(),
                                theme.border_default().to_egui(),
                            ),
                        );
                    }
                    if on {
                        ui.painter()
                            .rect_filled(rect, 0.0, theme.accent_primary().to_egui());
                    }
                    let fg = if on {
                        theme.text_on_accent()
                    } else {
                        theme.text_secondary()
                    }
                    .to_egui();
                    let icon_center =
                        egui::pos2(rect.left() + pad_x + icon_sz * 0.5, rect.center().y);
                    baked_icon::draw(ui.painter(), type_icon(ty), icon_center, icon_sz, fg);
                    if show_label {
                        ui.painter().text(
                            egui::pos2(icon_center.x + icon_sz * 0.5 + row_gap, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            label,
                            font.clone(),
                            fg,
                        );
                    }
                    if resp.on_hover_text(label).clicked() && !on {
                        picked = Some(ty);
                    }
                }
            });
        });

    picked
}

/// 세그먼트가 라벨을 보여줄지 — compact 압축 모드는 active 세그먼트만 라벨 유지.
/// 순수 함수라 렌더 없이 단위 테스트 가능(`SEG_COMPACT_AT` 문턱값 회귀 방지) — 이
/// TODO는 `ClipboardType`이 Text 하나뿐이라 실 데이터로 segmented 분기를 직접
/// 재현할 수 없으므로, 이 로직 자체의 정확성은 테스트로 담보한다.
fn seg_shows_label(compact: bool, active: bool) -> bool {
    !compact || active
}

/// 타입바 행 — 좌측 [`type_switch`] + 우측 슬롯(메타 텍스트 또는 커스텀 위젯,
/// design "type-bar 우측 슬롯"). 우측 슬롯을 클로저로 받아 텍스트 고정을 피한다 —
/// TODO49가 html 타입일 때 이 자리에 Pretty print 체크박스를 그리도록 다른 클로저를
/// 넘기면 된다(구조 변경 없음).
fn type_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    tr: &Translator,
    types: &[ClipboardType],
    active: ClipboardType,
    right_slot: impl FnOnce(&mut egui::Ui, &Theme),
) -> Option<ClipboardType> {
    let full_w = ui.available_width();
    let pad_x = row_pad_x(theme);
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = ControlSize::Sm.height(theme);
    let h = pad_y * 2.0 + ctrl_h;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(full_w, h), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());

    let content = egui::Rect::from_min_max(
        egui::pos2(rect.left() + pad_x, rect.top()),
        egui::pos2(rect.right() - pad_x, rect.bottom()),
    );
    let mut lui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    lui.spacing_mut().item_spacing.x = theme.spacing_md.value();
    let picked = type_switch(&mut lui, theme, tr, types, active);

    let mut rui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    right_slot(&mut rui, theme);

    bottom_separator(ui, theme, rect);
    picked
}

/// design `TypeBody` — 이 TODO는 Text arm만 채운다. Image/Html/Other/Files 는
/// 자매 TODO(48/49/50/52)가 이 match 에 arm 을 보탠다.
fn type_body(ui: &mut egui::Ui, theme: &Theme, ty: ClipboardType, content: &ContentRepr) {
    match (ty, content) {
        (ClipboardType::Text, ContentRepr::Text(text)) => text_body(ui, theme, text),
    }
}

/// mono pre 텍스트 — well(border+radius+bg-app fill) 안에 스크롤(design `cbWell` +
/// `cbMono`).
fn text_body(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    well(ui, theme, |ui| {
        ui.style_mut().interaction.selectable_labels = true;
        ui.add(
            egui::Label::new(
                egui::RichText::new(text)
                    .monospace()
                    .size(theme.font_size_term_sm.value())
                    .color(theme.text_primary().to_egui()),
            )
            .wrap(),
        );
    });
}

/// design `cbWell` — border+radius+bg-app fill 스크롤 컨테이너. 토큰 매핑:
/// fill=`bg-app`, border=`separator`+`border-width`, radius=`corner_radius`.
fn well(ui: &mut egui::Ui, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    let margin = theme.spacing_md.value();
    egui::Frame::new()
        .fill(theme.bg_app().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::symmetric(
            margin.round() as i8,
            theme.spacing_sm.value().round() as i8,
        ))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .id_salt("clip_body")
                .show(ui, add);
        });
}

/// 푸터 — mime 텍스트([`footer_mime_text`], 타입별 조건분기 수용 지점: 51 은
/// `{mime}` 기본 경로만, 49가 html 의 `{mime} · {meta}` 조건을 보탠다) + Close 버튼.
/// host 가 이미 outside-click/Esc close 경로를 제공하지만, 디자인이 footer Close 를
/// 명시적으로 요구해 중복이라도 그대로 반영한다.
fn footer(
    ui: &mut egui::Ui,
    theme: &Theme,
    tr: &Translator,
    ty: Option<ClipboardType>,
    close: &mut bool,
) {
    let full_w = ui.available_width();
    let pad_x = row_pad_x(theme);
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = ControlSize::Sm.height(theme);
    let h = pad_y * 2.0 + ctrl_h;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(full_w, h), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top() + theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );

    if let Some(ty) = ty {
        ui.painter().text(
            egui::pos2(rect.left() + pad_x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            footer_mime_text(ty, None),
            egui::FontId::monospace(theme.font_size_caption.value()),
            theme.text_muted().to_egui(),
        );
    }

    let btn_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + pad_y),
        egui::pos2(rect.right() - pad_x, rect.top() + pad_y + ctrl_h),
    );
    let mut bui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(btn_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    if Button::new(tr.t("clipboard_viewer.popup.close"))
        .variant(ButtonVariant::Secondary)
        .size(ControlSize::Sm)
        .show(&mut bui, theme)
        .clicked()
    {
        *close = true;
    }
}

/// design 조건: 일반 타입은 `{mime}`, HTML 타입([[49-...]])은 `{mime} · {meta}`.
/// 이 TODO는 Text(유일한 실제 타입) 기본 경로만 채운다 — `meta` 는 지금 미사용이지만
/// 시그니처는 49가 html arm 을 보탤 때 바꾸지 않아도 되게 남겨둔다.
fn footer_mime_text(ty: ClipboardType, _meta: Option<&str>) -> String {
    match ty {
        ClipboardType::Text => ty.mime_str().to_string(),
    }
}

/// 타입별 세그먼트/뱃지 아이콘(design `TYPE_ICON`). 이 TODO는 Text 하나만 실제
/// arm 을 채운다 — 자매 TODO가 `ClipboardType`에 arm 을 추가하며 이 match 에도
/// 갈래를 보탠다.
fn type_icon(ty: ClipboardType) -> &'static [&'static [[f32; 2]]] {
    match ty {
        ClipboardType::Text => baked_icons::TEXT_LEFT,
    }
}

/// 빈/읽기실패/이미열림 — 아이콘 + 굵은 타이틀 + 옅은 부제 2줄, content 영역 중앙
/// (design `CenterState`). `danger` 는 읽기실패 톤(`accent-danger`), 아니면 muted.
fn center_state(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: &'static [&'static [[f32; 2]]],
    title: &str,
    sub: Option<&str>,
    danger: bool,
) {
    let h = ui.available_height().max(1.0);
    ui.allocate_ui_with_layout(
        egui::vec2(ui.available_width(), h),
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.vertical_centered(|ui| {
                // design opacity 0.9(danger)/0.5(muted) 근사 — gamma_multiply(경고
                // callout 의 color-mix 근사 idiom 재사용).
                let icon_tint = if danger {
                    theme.accent_danger().to_egui().gamma_multiply(0.9)
                } else {
                    theme.text_muted().to_egui().gamma_multiply(0.5)
                };
                icon_glyph(ui, icon, CENTER_ICON_SIZE, icon_tint);
                ui.add_space(theme.spacing_sm.value());
                let title_color = if danger {
                    theme.accent_danger().to_egui()
                } else {
                    theme.text_secondary().to_egui()
                };
                ui.label(
                    egui::RichText::new(title)
                        .size(theme.font_size_body.value())
                        .strong()
                        .color(title_color),
                );
                if let Some(sub) = sub {
                    ui.add_space(theme.spacing_xs.value());
                    ui.label(
                        egui::RichText::new(sub)
                            .size(theme.font_size_term_sm.value())
                            .color(theme.text_muted().to_egui()),
                    );
                }
            });
        },
    );
}

/// 단독 글리프 — `size` 정사각 영역을 할당해 `color` tint 로 벡터 stroke 를 그린다
/// (베이크된 폴리라인, `tasty_plugin_sdk::baked_icon::draw`).
fn icon_glyph(ui: &mut egui::Ui, icon: &[&[[f32; 2]]], size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    baked_icon::draw(ui.painter(), icon, rect.center(), size, color);
}

/// 버튼 chrome 안에 그리는 글리프 — 버튼이 계산한 `rect`(정사각)에 `ICON_DRAW_RATIO`
/// 비율로 축소해 그린다(`tasty-plugin-image` 정본 튜닝값).
fn icon_in_button(ui: &mut egui::Ui, icon: &[&[[f32; 2]]], rect: egui::Rect, color: egui::Color32) {
    baked_icon::draw(
        ui.painter(),
        icon,
        rect.center(),
        rect.height() * ICON_DRAW_RATIO,
        color,
    );
}

fn bottom_separator(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect) {
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_mode_starts_at_seg_compact_at() {
        // compact 가 아니면(types.len() < SEG_COMPACT_AT) active 여부와 무관하게 항상
        // 라벨을 보여준다.
        assert!(seg_shows_label(false, false));
        assert!(seg_shows_label(false, true));
        // compact(true, types.len() >= SEG_COMPACT_AT) 는 active 세그먼트만 라벨 유지.
        assert!(!seg_shows_label(true, false));
        assert!(seg_shows_label(true, true));
    }

    #[test]
    fn seg_compact_at_matches_design() {
        assert_eq!(SEG_COMPACT_AT, 5);
    }

    #[test]
    fn footer_mime_text_text_is_plain_mime_only() {
        assert_eq!(footer_mime_text(ClipboardType::Text, None), "text/plain");
    }

    #[test]
    fn type_icon_text_uses_text_left_glyph() {
        // 값 비교 — `const` 는 참조마다 다른 주소로 프로모트될 수 있어(ptr::eq 로는
        // 신뢰 불가) 폴리라인 내용 자체를 비교한다.
        assert_eq!(type_icon(ClipboardType::Text), baked_icons::TEXT_LEFT);
    }
}
