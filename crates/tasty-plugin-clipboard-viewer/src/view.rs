//! egui-mesh popup 콘텐츠 렌더 — header → type-bar → body → footer 4단 구조
//! (design-system `overlays/clipboard_viewer.jsx` 구조 전사).
//!
//! rail(세로 타입 목록)은 폐기됐다 — 디자인 원칙: 단일 타입은 헤더 아래 뱃지로,
//! 복수 타입은 가로 세그먼트 스위치([`type_switch`])로 표현한다. `SEG_COMPACT_AT`(5)
//! 이상이면 비활성 세그먼트를 아이콘 전용으로 압축한다. `ClipboardType::Text`/`Files`/
//! `Image`/`Html`(HTML 은 raw 소스 표시 + Pretty print 체크박스, [`crate::html_format`])/
//! `Other`(text/files/image/html 가 아닌 raw 포맷을 포맷별 블록으로 나열,
//! [`crate::raw_formats`])을 채운다.
//!
//! chrome(scrim/border/outside-click/Esc)은 host 소유 — plugin 은 content 영역만
//! 그린다. 색·폰트·간격은 전부 host 가 보낸 `Theme` 토큰에서 가져온다(from_rgb/raw
//! px 금지). 헤더/푸터의 Close 버튼은 host chrome 의 outside-click/Esc 와 기능이
//! 중복되지만 디자인이 명시적으로 요구해 그대로 반영한다 — 클릭 시 `draw`/
//! `draw_already_open` 이 `true` 를 반환하고, 호출부(`main.rs`)가 `popup.close` IPC 로
//! host 에 닫기를 요청한다(host 가 chrome 생애주기를 계속 소유).

mod baked_icons {
    include!(concat!(env!("OUT_DIR"), "/plugin_icons.rs"));
}

use tasty_plugin_sdk::{Translator, baked_icon};
use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, IconButton, TagVariant, checkbox, tag};

use crate::ViewerState;
use crate::clipboard::{ClipboardType, ContentRepr, OtherFormatEntry, format_bytes};
use crate::html_format::prettify;

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

// CenterState 아이콘 크기는 `tasty-ui-widgets::tokens` 가 단일 출처다 — 갤러리
// specimen(`components/clipboard_viewer.rs`)이 같은 상수를 읽는다.
use tasty_ui_widgets::tokens::CLIPBOARD_CENTER_ICON_SIZE as CENTER_ICON_SIZE;

/// image body 아이콘 크기(design 고정값 30 — `CENTER_ICON_SIZE` 와 동일 정책).
const IMAGE_BODY_ICON_SIZE: f32 = 30.0;

/// "기타" 버킷 한 블록의 미리보기 최대 줄 수 — 넘으면 `+N more lines`로 절삭(design은
/// 구체적 상한을 구현에 위임). 목록 자체(포맷 개수)는 절대 접지 않는다(design
/// §6.5 확정) — 이건 블록 "내부" 콘텐츠 줄 수 상한일 뿐이다.
const OTHER_PREVIEW_MAX_LINES: usize = 20;

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

    // 우측 슬롯 — design `t.meta`(html 이 아닌 타입 전체 공통 경로). Text/Files 는
    // 메타가 없다(빈 클로저), Image 는 치수·크기 메타를 채운다. HTML 타입일 때만
    // "Pretty print" 체크박스로 스왑된다(design 확정 결과) — 그 경우
    // meta_text()(Html 은 항상 None)는 쓰지 않는다. 클릭 반영 전(이번 프레임 진입
    // 시점) active 기준으로 그린다 — type_switch 의 active 하이라이트도 동일하게
    // 클릭 전 상태를 쓰므로 한 프레임 지연이 일관된다.
    let active_meta = state
        .available
        .iter()
        .find(|(t, _)| *t == active)
        .and_then(|(_, c)| c.meta_text());
    // "기타" 세그먼트 tooltip(design "{n} unrecognized formats")에 쓰는 포맷
    // 개수 — Other 가 available 에 없으면 None(다른 타입 뿐이면 tooltip 없이 기본
    // 라벨 유지).
    let other_count = state.available.iter().find_map(|(t, c)| match (t, c) {
        (ClipboardType::Other, ContentRepr::Other(entries)) => Some(entries.len()),
        _ => None,
    });
    let mut html_pretty = state.html_pretty;
    let picked = type_bar(ui, theme, tr, &types, active, other_count, |ui, theme| {
        if active == ClipboardType::Html {
            checkbox(
                ui,
                theme,
                &mut html_pretty,
                tr.t("clipboard_viewer.popup.pretty_print"),
                true,
            );
        } else if let Some(meta) = &active_meta {
            meta_label(ui, theme, meta);
        }
    });
    state.html_pretty = html_pretty;
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
        type_body(&mut bui, theme, *ty, content, tr, state.html_pretty);
    }

    // HTML 타입은 밀려난 메타(문자수/줄수)를 푸터에서 `{mime} · {meta}` 로 결합해
    // 노출한다(design 확정 결과) — Other 는 mime 자체가 없어 포맷 개수 문구가
    // mime 을 대체한다(`footer_mime_text`). 다른 타입은 기존처럼 mime만.
    let footer_meta = cur.as_ref().and_then(|(ty, content)| {
        html_footer_meta(tr, *ty, content).or_else(|| other_footer_meta(tr, *ty, content))
    });
    footer(
        ui,
        theme,
        tr,
        cur.as_ref().map(|(t, _)| *t),
        footer_meta,
        close,
    );
}

/// design `TypeSwitch` — 1개면 아이콘+뱃지(읽기전용), 2개 이상이면 가로 세그먼트
/// 버튼 그룹(rail 재도입 금지). `SEG_COMPACT_AT` 이상이면 비활성 세그먼트를 아이콘
/// 전용으로 압축(active 만 라벨 유지) + `.on_hover_text()`로 전체 타입명 노출.
///
/// `other_count` — Other 타입이 available 이면 그 포맷 개수(design "{n} unrecognized
/// formats"). Other 세그먼트/뱃지의 tooltip 이 기본 라벨("Other") 대신 이
/// 개수 문구를 쓴다 — 다른 타입은 영향 없음.
fn type_switch(
    ui: &mut egui::Ui,
    theme: &Theme,
    tr: &Translator,
    types: &[ClipboardType],
    active: ClipboardType,
    other_count: Option<usize>,
) -> Option<ClipboardType> {
    if types.len() <= 1 {
        let ty = types.first().copied().unwrap_or(active);
        icon_glyph(
            ui,
            type_icon(ty),
            theme.icon_glyph_size_sm.value(),
            theme.text_muted().to_egui(),
        );
        let resp = tag(
            ui,
            theme,
            tr.t(ty.label_i18n_key()),
            TagVariant::Accent,
            false,
        );
        if ty == ClipboardType::Other
            && let Some(n) = other_count
        {
            resp.on_hover_text(other_unrecognized_text(tr, n));
        }
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
                    let tooltip = match (ty, other_count) {
                        (ClipboardType::Other, Some(n)) => other_unrecognized_text(tr, n),
                        _ => label.to_string(),
                    };
                    if resp.on_hover_text(tooltip).clicked() && !on {
                        picked = Some(ty);
                    }
                }
            });
        });

    picked
}

/// 세그먼트가 라벨을 보여줄지 — compact 압축 모드는 active 세그먼트만 라벨 유지.
/// 순수 함수라 렌더 없이 단위 테스트 가능(`SEG_COMPACT_AT` 문턱값 회귀 방지) — 현재
/// `ClipboardType`이 2종(Text/Files)뿐이라 `SEG_COMPACT_AT`(5) 이상의 compact 분기를
/// 실 데이터로 직접 재현할 수 없으므로, 이 로직 자체의 정확성은 테스트로 담보한다.
fn seg_shows_label(compact: bool, active: bool) -> bool {
    !compact || active
}

/// 타입바 행 — 좌측 [`type_switch`] + 우측 슬롯(메타 텍스트 또는 커스텀 위젯,
/// design "type-bar 우측 슬롯"). 우측 슬롯을 클로저로 받아 텍스트 고정을 피한다 —
/// HTML 타입일 때 [`data_state`]가 이 자리에 Pretty print 체크박스를 그리는 클로저를
/// 넘긴다(구조 변경 없음).
fn type_bar(
    ui: &mut egui::Ui,
    theme: &Theme,
    tr: &Translator,
    types: &[ClipboardType],
    active: ClipboardType,
    other_count: Option<usize>,
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
    let picked = type_switch(&mut lui, theme, tr, types, active, other_count);

    let mut rui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    right_slot(&mut rui, theme);

    bottom_separator(ui, theme, rect);
    picked
}

/// design `TypeBody` — Text/Files/Image/Html/Other arm 을 채운다(51/52/48/49/50).
fn type_body(
    ui: &mut egui::Ui,
    theme: &Theme,
    ty: ClipboardType,
    content: &ContentRepr,
    tr: &Translator,
    html_pretty: bool,
) {
    match (ty, content) {
        (ClipboardType::Text, ContentRepr::Text(text)) => text_body(ui, theme, text),
        (ClipboardType::Files, ContentRepr::Files(files)) => files_body(ui, theme, files),
        (ClipboardType::Image, ContentRepr::Image { .. }) => {
            // meta_text() 는 Image 에 항상 Some — read_available() 이 항상 실제
            // width/height/byte_len 을 채워 push 하므로.
            let meta = content.meta_text().unwrap_or_default();
            image_body(ui, theme, tr, &meta);
        }
        (ClipboardType::Html, ContentRepr::Html(html)) => {
            // 렌더링 없이 원본 소스 그대로 — 체크 시에만 인덴터를 거친 결과로 교체.
            // 위젯 구조 자체는 text 타입과 동일(design 확정 결과).
            if html_pretty {
                text_body(ui, theme, &prettify(html));
            } else {
                text_body(ui, theme, html);
            }
        }
        (ClipboardType::Other, ContentRepr::Other(entries)) => other_body(ui, theme, tr, entries),
        // ClipboardType 과 ContentRepr 는 read_available() 이 항상 같은 종류끼리만
        // 짝지어 push 한다 — 다른 조합은 구조적으로 발생하지 않는다.
        _ => unreachable!("ClipboardType/ContentRepr mismatch"),
    }
}

/// design `TypeBody` 의 image 분기 — 아이콘 + 메타(치수·크기) + "인라인 미리보기
/// 없음" 안내 문구, well 안에 중앙 정렬(실제 픽셀 렌더링 없음 — design 결정).
fn image_body(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, meta: &str) {
    well_centered(ui, theme, |ui| {
        icon_glyph(
            ui,
            baked_icons::IMAGE,
            IMAGE_BODY_ICON_SIZE,
            theme.text_muted().to_egui(),
        );
        ui.add_space(theme.spacing_sm.value());
        meta_label(ui, theme, meta);
        ui.add_space(theme.spacing_xs.value());
        ui.label(
            egui::RichText::new(tr.t("clipboard_viewer.popup.image_no_preview"))
                .italics()
                .size(theme.font_size_caption.value())
                .color(theme.text_disabled().to_egui()),
        );
    });
}

/// design `cbMetaMono` — mono/caption 크기/text-muted 색의 메타 텍스트 한 줄(type-bar
/// 우측 슬롯 + image body 공용).
fn meta_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .monospace()
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// mono pre 텍스트 — well(border+radius+bg-app fill) 안에 스크롤(design `cbWell` +
/// `cbMono`).
///
/// `egui::Label` 대신 read-only 흉내를 낸 `egui::TextEdit` 를 쓴다 — `Label` 의 내장
/// 드래그 선택(`LabelSelectionState`)은 세로 이탈만 처리하고 가로 이탈은 처리하지
/// 않아(egui 의도적 설계 범위, upstream 미수정 확정) 포인터가 위젯을 가로로
/// 빠르게 벗어나면 선택이 멈춘다. `TextEdit` 의 커서 갱신은 이 게이팅이 없다.
/// `interactive(false)` 는 쓰지 않는다 — 편집뿐 아니라 선택 자체도 막아버린다
/// (egui 소스 확인). 대신 매 프레임 지역 `String` 버퍼를 넘겨 편집 결과를 버린다.
fn text_body(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    well(ui, theme, |ui| {
        // 캐럿(편집 커서)만 숨긴다 — 이 well 스코프의 자식 ui 한정이라 다른 위젯에
        // 새지 않는다. selection 하이라이트는 별도 스타일이라 영향 없음.
        ui.visuals_mut().text_cursor.stroke = egui::Stroke::NONE;
        let mut buf = text.to_owned();
        ui.add(
            egui::TextEdit::multiline(&mut buf)
                .font(egui::FontId::monospace(theme.font_size_term_sm.value()))
                .text_color(theme.text_primary().to_egui())
                .frame(false)
                .desired_width(ui.available_width())
                .desired_rows(1),
        );
    });
}

/// 파일 경로 목록 — 아이콘 + mono 경로 텍스트를 한 줄씩(design `TypeBody` `files`
/// 분기 1:1 전사, `well` 안에 스크롤). 긴 경로는 말줄임(ellipsis).
fn files_body(ui: &mut egui::Ui, theme: &Theme, files: &[std::path::PathBuf]) {
    well(ui, theme, |ui| {
        let icon_sz = theme.icon_glyph_size_sm.value();
        let gap = theme.spacing_sm.value();
        let row_h = icon_sz.max(theme.font_size_term_sm.value()) + theme.spacing_xs.value();
        let font = egui::FontId::monospace(theme.font_size_term_sm.value());
        for path in files {
            let full_w = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(full_w, row_h), egui::Sense::hover());
            let icon_center = egui::pos2(rect.left() + icon_sz * 0.5, rect.center().y);
            baked_icon::draw(
                ui.painter(),
                baked_icons::FILE,
                icon_center,
                icon_sz,
                theme.text_muted().to_egui(),
            );
            let text_x = rect.left() + icon_sz + gap;
            let galley = truncated_galley(
                ui,
                &path.display().to_string(),
                font.clone(),
                theme.text_primary().to_egui(),
                (rect.right() - text_x).max(0.0),
            );
            ui.painter().galley(
                egui::pos2(text_x, rect.center().y - galley.size().y * 0.5),
                galley,
                theme.text_primary().to_egui(),
            );
        }
    });
}

/// "기타" 버킷 본문 — 발견된 포맷마다 한 블록씩 세로 나열, 블록 사이 1px
/// separator(design `TypeBody` `other` 분기 1:1 전사). 목록 자체는 절대 접지
/// 않는다(design §6.5 확정) — well 이 이미 스크롤되므로 포맷이 몇 개든 전부 그린다.
fn other_body(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, entries: &[OtherFormatEntry]) {
    well(ui, theme, |ui| {
        for (i, entry) in entries.iter().enumerate() {
            if i > 0 {
                ui.add_space(theme.spacing_sm.value());
                let full_w = ui.available_width();
                let (rect, _) = ui.allocate_exact_size(
                    egui::vec2(full_w, theme.border_width.value()),
                    egui::Sense::hover(),
                );
                ui.painter().hline(
                    rect.x_range(),
                    rect.center().y,
                    egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
                );
                ui.add_space(theme.spacing_sm.value());
            }
            other_format_block(ui, theme, tr, entry);
        }
    });
}

/// "기타" 버킷 한 블록 — 첫 줄 이름+크기(같은 줄), 그 아래 텍스트화된 미리보기,
/// `OTHER_PREVIEW_MAX_LINES` 초과 시 이탤릭 `+N more lines`(design 확정 결과).
fn other_format_block(ui: &mut egui::Ui, theme: &Theme, tr: &Translator, entry: &OtherFormatEntry) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(&entry.name)
                .monospace()
                .strong()
                .size(theme.font_size_caption.value())
                .color(theme.text_secondary().to_egui()),
        );
        ui.label(
            egui::RichText::new(format_bytes(entry.byte_len))
                .monospace()
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    });
    ui.add_space(theme.spacing_xs.value());
    let (shown, truncated_lines) = truncate_lines(&entry.preview, OTHER_PREVIEW_MAX_LINES);
    // 바이너리 fallback(hex 요약)은 실제 클립보드 내용이 아니라 대체 표현이라는
    // 것을 이탤릭으로 구분한다 — 사용자가 hex 를 원본 텍스트로 오인하지 않게.
    let mut preview_text = egui::RichText::new(shown)
        .monospace()
        .size(theme.font_size_term_sm.value())
        .color(theme.text_primary().to_egui());
    if entry.is_binary {
        preview_text = preview_text.italics();
    }
    ui.add(egui::Label::new(preview_text).wrap());
    if truncated_lines > 0 {
        ui.add_space(theme.spacing_xs.value());
        ui.label(
            egui::RichText::new(other_more_lines_text(tr, truncated_lines))
                .italics()
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    }
}

/// 순수 함수 — `text`를 최대 `max_lines`줄로 절삭하고 잘려나간 줄 수를 반환한다.
/// 렌더 없이 단위 테스트 가능.
fn truncate_lines(text: &str, max_lines: usize) -> (String, usize) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        return (text.to_string(), 0);
    }
    (lines[..max_lines].join("\n"), lines.len() - max_lines)
}

/// design "+{n} more lines" — 블록 본문 절삭 문구.
fn other_more_lines_text(tr: &Translator, n: usize) -> String {
    tr.t("clipboard_viewer.popup.other_more_lines")
        .replace("{n}", &n.to_string())
}

/// design "{n} unrecognized formats" — Other 세그먼트/뱃지 tooltip 문구.
fn other_unrecognized_text(tr: &Translator, n: usize) -> String {
    tr.t("clipboard_viewer.popup.other_unrecognized_formats")
        .replace("{n}", &n.to_string())
}

/// Other 타입일 때 푸터 메타 — `{n} unrecognized formats`(mime 이 없어 이 문구가
/// mime 자리를 대체한다, `footer_mime_text`). 다른 타입은 `None`.
fn other_footer_meta(tr: &Translator, ty: ClipboardType, content: &ContentRepr) -> Option<String> {
    match (ty, content) {
        (ClipboardType::Other, ContentRepr::Other(entries)) => {
            Some(other_unrecognized_text(tr, entries.len()))
        }
        _ => None,
    }
}

/// 한 줄 말줄임 galley — 폭 초과 시 '…' 로 잘라낸다(`tasty-ui-widgets::listctrl` 정본
/// 이식, design ellipsis 전사).
fn truncated_galley(
    ui: &egui::Ui,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width);
    ui.fonts(|f| f.layout_job(job))
}

/// design `cbWell` frame — border+radius+bg-app fill. 토큰 매핑: fill=`bg-app`,
/// border=`separator`+`border-width`, radius=`corner_radius`. [`well`](스크롤)과
/// [`well_centered`](중앙 정렬, image body) 가 공유한다.
fn well_frame(theme: &Theme) -> egui::Frame {
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
}

/// design `cbWell` — 스크롤 컨테이너(text/html 같은 긴 콘텐츠).
fn well(ui: &mut egui::Ui, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    well_frame(theme).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .id_salt("clip_body")
            .show(ui, add);
    });
}

/// design `cbWell` — 콘텐츠를 상하좌우 중앙에 배치(image body 전용, design jsx의
/// image 분기가 `cbWell` 에 `display:flex; alignItems:center; justifyContent:center`
/// 를 덧씌운 것과 동형).
fn well_centered(ui: &mut egui::Ui, theme: &Theme, add: impl FnOnce(&mut egui::Ui)) {
    well_frame(theme).show(ui, |ui| {
        let h = ui.available_height().max(1.0);
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), h),
            egui::Layout::centered_and_justified(egui::Direction::TopDown),
            |ui| {
                ui.vertical_centered(add);
            },
        );
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
    meta: Option<String>,
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
            footer_mime_text(ty, meta.as_deref()),
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

/// design 조건: 일반 타입은 `{mime}`, HTML 타입은 `{mime} · {meta}`(확정 결과).
/// Other 는 mime 자체가 없어 meta(포맷 개수 문구)가 mime 을 통째로 대체한다.
/// Text/Files/Image 는 기본 경로만 채운다.
fn footer_mime_text(ty: ClipboardType, meta: Option<&str>) -> String {
    match (ty, meta) {
        (ClipboardType::Html, Some(meta)) => format!("{} · {meta}", ty.mime_str()),
        (ClipboardType::Other, Some(meta)) => meta.to_string(),
        _ => ty.mime_str().to_string(),
    }
}

/// HTML 타입일 때만 푸터 메타(`{n} chars · {n} line(s)`, design 확정 결과 예시
/// `312 chars · 1 line`)를 만든다. 다른 타입은 `None`(mime만 표시).
fn html_footer_meta(tr: &Translator, ty: ClipboardType, content: &ContentRepr) -> Option<String> {
    match (ty, content) {
        (ClipboardType::Html, ContentRepr::Html(html)) => Some(format_html_meta(tr, html)),
        _ => None,
    }
}

fn format_html_meta(tr: &Translator, html: &str) -> String {
    let chars = html.chars().count();
    let lines = html.lines().count().max(1);
    let key = if lines == 1 {
        "clipboard_viewer.popup.html_meta_line"
    } else {
        "clipboard_viewer.popup.html_meta_lines"
    };
    tr.t(key)
        .replace("{chars}", &chars.to_string())
        .replace("{lines}", &lines.to_string())
}

/// 타입별 세그먼트/뱃지 아이콘(design `TYPE_ICON`). Other 는 `layers`(design 확정
/// 결과 — "여러 겹" = 여러 포맷이 쌓여있다는 은유).
fn type_icon(ty: ClipboardType) -> &'static [&'static [[f32; 2]]] {
    match ty {
        ClipboardType::Text => baked_icons::TEXT_LEFT,
        ClipboardType::Files => baked_icons::FILE,
        ClipboardType::Image => baked_icons::IMAGE,
        ClipboardType::Html => baked_icons::HTML,
        ClipboardType::Other => baked_icons::LAYERS,
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
                // 빈 상태 아이콘 톤 — 디자인이 적은 opacity 0.9(danger)/0.5(muted).
                // 대응 토큰 없음(0.5 는 `opacity_disabled` 와 값만 같고 역할이 다르다).
                const EMPTY_ICON_DANGER_OPACITY: f32 = 0.9;
                const EMPTY_ICON_MUTED_OPACITY: f32 = 0.5;
                let icon_tint = if danger {
                    theme
                        .accent_danger()
                        .to_egui()
                        .gamma_multiply(EMPTY_ICON_DANGER_OPACITY)
                } else {
                    theme
                        .text_muted()
                        .to_egui()
                        .gamma_multiply(EMPTY_ICON_MUTED_OPACITY)
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
    fn footer_mime_text_image_is_rgba8_mime_only() {
        assert_eq!(footer_mime_text(ClipboardType::Image, None), "image/rgba8");
    }

    #[test]
    fn type_icon_text_uses_text_left_glyph() {
        // 값 비교 — `const` 는 참조마다 다른 주소로 프로모트될 수 있어(ptr::eq 로는
        // 신뢰 불가) 폴리라인 내용 자체를 비교한다.
        assert_eq!(type_icon(ClipboardType::Text), baked_icons::TEXT_LEFT);
    }

    #[test]
    fn footer_mime_text_files_is_uri_list_mime() {
        assert_eq!(
            footer_mime_text(ClipboardType::Files, None),
            "text/uri-list"
        );
    }

    #[test]
    fn type_icon_files_uses_file_glyph() {
        assert_eq!(type_icon(ClipboardType::Files), baked_icons::FILE);
    }

    #[test]
    fn type_icon_image_uses_image_glyph() {
        assert_eq!(type_icon(ClipboardType::Image), baked_icons::IMAGE);
    }

    #[test]
    fn footer_mime_text_html_without_meta_is_mime_only() {
        assert_eq!(footer_mime_text(ClipboardType::Html, None), "text/html");
    }

    #[test]
    fn footer_mime_text_html_with_meta_combines_mime_and_meta() {
        assert_eq!(
            footer_mime_text(ClipboardType::Html, Some("312 chars · 1 line")),
            "text/html · 312 chars · 1 line"
        );
    }

    #[test]
    fn type_icon_html_uses_html_glyph() {
        assert_eq!(type_icon(ClipboardType::Html), baked_icons::HTML);
    }

    #[test]
    fn html_footer_meta_is_none_for_non_html_types() {
        assert_eq!(
            html_footer_meta(
                &Translator::default(),
                ClipboardType::Text,
                &ContentRepr::Text("x".into())
            ),
            None
        );
    }

    fn test_translator() -> Translator {
        Translator::load(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("lang"),
            "en",
        )
    }

    #[test]
    fn format_html_meta_singular_line_uses_singular_key() {
        assert_eq!(
            format_html_meta(&test_translator(), "<p>x</p>"),
            "8 chars · 1 line"
        );
    }

    #[test]
    fn format_html_meta_plural_lines_uses_plural_key() {
        assert_eq!(
            format_html_meta(&test_translator(), "line one\nline two"),
            "17 chars · 2 lines"
        );
    }

    #[test]
    fn type_icon_other_uses_layers_glyph() {
        assert_eq!(type_icon(ClipboardType::Other), baked_icons::LAYERS);
    }

    #[test]
    fn footer_mime_text_other_without_meta_falls_back_to_mime_str() {
        // read_available() 은 entries 가 비어 있으면 Other 를 애초에 push 하지
        // 않으므로 실전에서는 발생하지 않지만, 함수 자체는 두 인자 조합 모두에
        // 대해 정의돼야 한다.
        assert_eq!(
            footer_mime_text(ClipboardType::Other, None),
            "application/octet-stream"
        );
    }

    #[test]
    fn footer_mime_text_other_with_meta_replaces_mime_entirely() {
        assert_eq!(
            footer_mime_text(ClipboardType::Other, Some("2 unrecognized formats")),
            "2 unrecognized formats"
        );
    }

    #[test]
    fn other_footer_meta_is_none_for_non_other_types() {
        assert_eq!(
            other_footer_meta(
                &test_translator(),
                ClipboardType::Text,
                &ContentRepr::Text("x".into())
            ),
            None
        );
    }

    #[test]
    fn other_footer_meta_counts_entries() {
        let entries = vec![
            OtherFormatEntry::from_bytes("A".into(), b"1", 1024),
            OtherFormatEntry::from_bytes("B".into(), b"2", 1024),
        ];
        assert_eq!(
            other_footer_meta(
                &test_translator(),
                ClipboardType::Other,
                &ContentRepr::Other(entries)
            )
            .as_deref(),
            Some("2 unrecognized formats")
        );
    }

    #[test]
    fn truncate_lines_keeps_short_text_untouched() {
        assert_eq!(truncate_lines("a\nb\nc", 20), ("a\nb\nc".to_string(), 0));
    }

    #[test]
    fn truncate_lines_cuts_and_counts_remainder() {
        let text = (0..25)
            .map(|i| format!("line{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let (shown, cut) = truncate_lines(&text, 20);
        assert_eq!(shown.lines().count(), 20);
        assert_eq!(cut, 5);
    }

    #[test]
    fn other_more_lines_text_interpolates_count() {
        assert_eq!(
            other_more_lines_text(&test_translator(), 7),
            "+7 more lines"
        );
    }

    #[test]
    fn other_unrecognized_text_interpolates_count() {
        assert_eq!(
            other_unrecognized_text(&test_translator(), 3),
            "3 unrecognized formats"
        );
    }
}
