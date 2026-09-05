//! `clipboard_viewer` specimen — clipboard-viewer plugin 의 header/type-bar/body/
//! footer popup (egui-mesh popup 전사, Overlays).
//!
//! 본체 렌더 경로: plugin `crates/tasty-plugin-clipboard-viewer/src/view.rs` 가
//! **egui-mesh popup**(ADR-0028 / B4)으로 popup 콘텐츠를 자기 프로세스에서 egui 로
//! 그린다 — rail(세로 타입 목록)은 폐기됐다. header(아이콘+타이틀+snapshot 뱃지+
//! close) → type-bar(1개면 아이콘+뱃지, 2개 이상이면 가로 세그먼트 스위치) →
//! body(well: border+radius+bg-app 스크롤) → footer(mime+Close) 4단 수직 스택.
//! host 는 셸(scrim/border)만 그리고 plugin mesh 를 content 영역에 합성한다. 갤러리는
//! plugin/host crate 에 의존할 수 없어 그 *구성* 을 Theme 토큰 painter mock 으로
//! 전사한다 — 픽셀 동일성 비목표, 토큰·구조 정합 목표.
//!
//! 9 상태를 나란히 노출:
//! - **data (text only)** — 정상 4단, type-bar 는 배지로 표시(타입 1개).
//! - **data (files, segmented)** — type-bar 가 Text/Files 2개 세그먼트로 표시되고
//!   body 는 아이콘+경로 한 줄씩.
//! - **image** — Image 타입 body(아이콘 + 치수·크기 메타 + "인라인 미리보기 없음"
//!   안내, 실제 픽셀 렌더링 없음 — design 결정).
//! - **html — raw source** — HTML 타입, Pretty print 체크박스 미체크(원본 그대로).
//! - **html — pretty print** — 같은 데이터, 체크박스 체크(인덴트 적용).
//! - **other** — text/files/image/html 가 아닌 raw 포맷을 이름+크기+미리보기 블록으로
//!   나열, 블록 사이 separator, 긴 미리보기는 `+N more lines`로 절삭.
//! - **empty** — 가용 타입 0개(아이콘 + 굵은 타이틀 + 옅은 부제 2줄).
//! - **read failed** — 클립보드 핸들 실패(danger 톤).
//! - **already open** — 단일 인스턴스 가드.
//!
//! `SEG_COMPACT_AT`(5) 이상의 압축 세그먼트는 이 TODO 시점에도 실 데이터가 5종
//! (Text/Files/Image/Html/Other)뿐이라 실제로 재현되지 않는다 — plugin `view.rs` 와
//! 동일한 한계다(`spec::note` 참고).

use std::cell::RefCell;
use tasty_type_geometry::length::LogicalPx;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{TagVariant, checkbox, tag};

use crate::catalog::icons::{self, MockGlyph};
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// popup 본문 치수(디자인 480×360 고정 — size_hint). Theme 에 대응 토큰이 없는
/// 화면 전용 고정값.
const POPUP_W: LogicalPx = LogicalPx(480.0);
const POPUP_H: f32 = 360.0;

// CenterState 아이콘 크기는 plugin 본체와 **같은 상수**를 읽는다(`tasty-ui-widgets::tokens`).
use tasty_ui_widgets::tokens::CLIPBOARD_CENTER_ICON_SIZE as CENTER_ICON_SIZE;

/// body well 안 mono 미리보기 샘플 — 현재 클립보드 text 표현.
const PREVIEW: &[&str] = &[
    "cargo build -p tasty-gallery",
    "git switch wt-5/T8-code",
    "tasty read screen --surface 3",
];

/// files 상태 body 미리보기 샘플 — 파일 탐색기에서 복사한 경로 목록.
const FILE_PREVIEW: &[&str] = &[
    "/home/user/Documents/report.pdf",
    "/home/user/Pictures/screenshot-2026-07-30.png",
    "/home/user/workspace/tasty/Cargo.toml",
];

/// HTML specimen 원본 소스(raw). `HTML_PRETTY` 는 같은 내용을 plugin
/// `html_format::prettify()` 와 동형 규칙(태그 깊이 인덴트)으로 손으로 정리해둔
/// 짝(design 의 `html`/`htmlPretty` specimen 과 동형) — 갤러리는 plugin crate 를
/// 의존할 수 없어 알고리즘을 다시 부르는 대신 결과물을 그대로 박아둔다.
const HTML_RAW: &str = "<div class=\"card\"><p>Hello <b>world</b></p></div>";
const HTML_PRETTY: &str =
    "<div class=\"card\">\n  <p>\n    Hello\n    <b>\n      world\n    </b>\n  </p>\n</div>";

/// "기타" specimen 한 포맷 블록 — 실 데이터는 plugin
/// `clipboard::OtherFormatEntry`(이름/바이트 길이/미리보기)지만 갤러리는 plugin
/// crate 를 의존할 수 없어 (이름, 크기 문자열, 미리보기, 절삭 줄 수) 를 직접 박아
/// 둔다.
struct OtherSample {
    name: &'static str,
    size: &'static str,
    preview: &'static str,
    /// `Some(n)` 이면 그 블록에 `+n more lines` 절삭 문구를 함께 그린다(design
    /// 확정 결과 — 내용이 길면 이탤릭 텍스트로 절삭 표시).
    more_lines: Option<usize>,
}

/// text/files/image/html 어디에도 안 걸린 raw 포맷 예시 2종 — 하나는 짧은 순수 텍스트
/// (Windows 드래그앤드롭 힌트류), 하나는 절삭이 필요한 긴 JSON(커스텀 앱 전용 포맷).
const OTHER_SAMPLES: &[OtherSample] = &[
    OtherSample {
        name: "Files Drop Effect",
        size: "4 B",
        preview: "DROPEFFECT_COPY",
        more_lines: None,
    },
    OtherSample {
        name: "Custom App Format",
        size: "1.2 KB",
        preview: "{\"id\":\"note-8842\",\"kind\":\"reference\"}\n{\"id\":\"note-8843\",\"kind\":\"reference\"}\n{\"id\":\"note-8844\",\"kind\":\"reference\"}",
        more_lines: Some(6),
    },
];

thread_local! {
    static HTML_RAW_PRETTY_ON: RefCell<bool> = const { RefCell::new(false) };
    static HTML_PRETTY_PRETTY_ON: RefCell<bool> = const { RefCell::new(true) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(
            ui,
            theme,
            "text only — header / type-bar(badge) / body / footer",
            |ui| data_popup(ui, theme),
        );
        spec::cluster(
            ui,
            theme,
            "files — type-bar(segmented Text/Files) / body(icon+path rows)",
            |ui| files_popup(ui, theme),
        );
        spec::cluster(
            ui,
            theme,
            "image — icon + meta + \"no inline preview\"",
            |ui| image_popup(ui, theme),
        );
        spec::cluster(
            ui,
            theme,
            "html — raw source (Pretty print unchecked)",
            |ui| data_popup_html(ui, theme, &HTML_RAW_PRETTY_ON, HTML_RAW),
        );
        spec::cluster(
            ui,
            theme,
            "html — pretty print (Pretty print checked)",
            |ui| data_popup_html(ui, theme, &HTML_PRETTY_PRETTY_ON, HTML_PRETTY),
        );
        spec::cluster(
            ui,
            theme,
            "other — raw format bucket (unrecognized formats)",
            |ui| other_popup(ui, theme),
        );
        spec::cluster(ui, theme, "empty clipboard", |ui| {
            center_popup(
                ui,
                theme,
                icons::CLIPBOARD,
                "Clipboard is empty",
                "Copy some text, an image, or files and reopen to see a snapshot here.",
                false,
            );
        });
        spec::cluster(ui, theme, "read failed", |ui| {
            center_popup(
                ui,
                theme,
                icons::ALERT_TRIANGLE,
                "Couldn't read the clipboard",
                "The system clipboard handle could not be opened. Close another app that may be holding it and reopen.",
                true,
            );
        });
        spec::cluster(ui, theme, "already open", |ui| {
            center_popup(
                ui,
                theme,
                icons::LOCK,
                "Clipboard viewer is already open",
                "Only one snapshot window runs at a time — the existing one was brought to the front.",
                false,
            );
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "480×360 popup · bg-panel"),
            ("header", "icon + title(14/600) + snapshot tag + close"),
            (
                "type-bar",
                "≤1: icon+tag(accent) · ≥2: segmented(border-default)",
            ),
            ("body", "well(border+radius+bg-app) · mono scroll"),
            (
                "footer",
                "mime(mono caption)[+meta for html] + Close(secondary)",
            ),
            (
                "html type-bar right slot",
                "Pretty print checkbox swaps in for meta text",
            ),
            (
                "states",
                "data(text) · data(files) · image · html(raw/pretty) · other · empty · read-failed · already-open",
            ),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new("bg-app", "body well fill", theme.bg_app().to_egui()),
            TokenChip::new(
                "bg-sidebar",
                "type-bar row fill",
                theme.bg_sidebar().to_egui(),
            ),
            TokenChip::new(
                "accent-primary",
                "active segment / badge",
                theme.accent_primary().to_egui(),
            ),
            TokenChip::new("separator", "row divider", theme.separator.to_egui()),
            TokenChip::new(
                "accent-danger",
                "read-failed tone",
                theme.accent_danger().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "구조 전사 — 좌측 rail(세로 타입 목록)을 폐기하고 header/type-bar/\
         body/footer 4단 수직 스택으로 교체했다. 타입이 1개(Text)면 type-bar 를 배지 \
         하나로만, 2개 이상(Text/Files)이면 가로 세그먼트로 보여준다 — \
         `SEG_COMPACT_AT`(5) 이상의 압축 세그먼트(비활성 세그먼트가 아이콘 전용으로 \
         축소)는 골격만 갖춰뒀고 [[50]]이 타입을 늘리면 실제로 재현된다(그때 이 \
         specimen 도 압축 세그먼트 상태를 추가한다). files body 는 아이콘+mono 경로 \
         한 줄씩, 긴 경로는 말줄임 처리한다(design ellipsis 전사). image body 는 실제 \
         픽셀을 렌더링하지 않고 아이콘+치수·크기 메타+안내 문구만 중앙 정렬로 보여준다\
         (design 결정). HTML 타입은 렌더링하지 않고 원본 소스를 text \
         타입과 동일한 mono well 로 보여준다. type-bar 우측의 메타 슬롯이 HTML 타입일 \
         때만 Pretty print 체크박스로 스왑되고, 밀려난 메타(문자수·줄수)는 푸터로 \
         이동해 mime 과 `·` 로 결합 표시된다(`text/html · N chars · N line(s)`). \
         체크박스 on 상태의 인덴트 결과는 plugin `html_format::prettify()`(새 의존성 \
         없는 태그 깊이 인덴터, script/style/pre 는 verbatim 보존)와 동일 규칙으로 \
         수기 정리한 샘플이다 — 갤러리는 plugin crate 를 의존할 수 없어 결과 문자열을 \
         직접 박아둔다. 헤더/푸터의 Close 버튼은 host 의 outside-click/Esc 와 기능 \
         중복이지만 디자인이 명시적으로 요구해 그대로 반영했다. text/files/\
         image/html 어디에도 속하지 않는 raw 포맷은 \"Other\" 타입 하나로 묶여 \
         type-bar 에 나타난다. body 는 발견된 포맷마다 이름(mono, text-secondary, \
         굵게)+크기(mono, text-muted)를 같은 줄에, 그 아래 텍스트화된 미리보기를 \
         보여주는 블록을 세로로 나열하고 블록 사이는 1px separator 로 구분한다 — \
         목록 자체(포맷 개수)는 절대 접지 않는다(design §6.5 확정). 미리보기가 길면 \
         `+N more lines`로 절삭 표시한다. footer 는 mime 자리에 \"{n} unrecognized \
         formats\" 문구가 대신 들어간다(여러 이종 포맷을 묶은 버킷이라 단일 mime 이 \
         없음). 실제 raw 열거는 plugin `raw_formats`(Windows `clipboard-win`/macOS \
         `objc2-app-kit`/Linux `x11rb` TARGETS)가 text/files/image/html 로 이미 소비된 \
         변형을 플랫폼별 매핑 테이블로 제외한 나머지를 읽는다 — 갤러리는 그 결과 \
         문자열만 손으로 정리해 박아둔다.",
    );
}

/// image body 메타 샘플 — design mock(`clipboard_viewer.html` `multi.types` image 항목)
/// 과 동일한 예시 수치. 실제 값은 arboard `ImageData::width/height` + `bytes.len()`
/// 근사(`crates/tasty-plugin-clipboard-viewer/src/clipboard.rs::format_bytes`).
const IMAGE_META: &str = "1920×1080 · 7.9 MB";

/// 정상 데이터 상태 — header + type-bar(배지) + body(well) + footer 4행.
fn data_popup(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        header_row(ui, theme);
        type_bar_row(ui, theme);
        body_row(ui, theme);
        footer_row(ui, theme, "text/plain");
    });
}

/// files 상태 — header + type-bar(Text/Files 세그먼트) + body(경로 행) + footer 4행.
fn files_popup(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        header_row(ui, theme);
        type_bar_segmented_row(ui, theme);
        files_body_row(ui, theme);
        footer_row(ui, theme, "text/uri-list");
    });
}

/// image 타입 상태 — header + type-bar(Image 뱃지+meta) + body(아이콘+메타+안내) +
/// footer 4행(실제 렌더링 없음).
fn image_popup(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        header_row(ui, theme);
        image_type_bar_row(ui, theme);
        image_body_row(ui, theme);
        footer_row(ui, theme, "image/rgba8");
    });
}

/// type-bar — 타입 2개(Text/Files) 가로 세그먼트, active=Files(accent 채움).
fn type_bar_segmented_row(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
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

    egui::Frame::new()
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::ZERO)
        .show(&mut lui, |ui| {
            ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
            ui.horizontal(|ui| {
                seg(ui, theme, icons::TEXT_LEFT, "Text", false);
                seg(ui, theme, icons::FILE, "Files", true);
            });
        });

    hline(ui, theme, rect.bottom());
}

/// type-bar — Image 뱃지(좌) + meta 텍스트(우, design `t.meta` 슬롯).
fn image_type_bar_row(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
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
    lui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    kit::icon(
        &mut lui,
        icons::IMAGE,
        theme.icon_glyph_size_sm.value(),
        theme.text_muted().to_egui(),
    );
    tag(&mut lui, theme, "Image", TagVariant::Accent, false);

    let mut rui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    rui.label(
        egui::RichText::new(IMAGE_META)
            .monospace()
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );

    hline(ui, theme, rect.bottom());
}

/// 세그먼트 한 칸 — active 면 accent 채움 + on-accent 텍스트.
fn seg(ui: &mut egui::Ui, theme: &Theme, glyph: MockGlyph, label: &str, active: bool) {
    let h = theme.item_height_tab.value();
    let icon_sz = theme.icon_glyph_size_xs.value();
    let pad_x = theme.spacing_sm.value();
    let gap = theme.spacing_xs.value();
    let font = egui::FontId::proportional(theme.font_size_term_sm.value());
    let label_w = ui
        .fonts(|f| f.layout_no_wrap(label.to_owned(), font.clone(), egui::Color32::PLACEHOLDER))
        .size()
        .x;
    let w = pad_x * 2.0 + icon_sz + gap + label_w;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    if active {
        ui.painter()
            .rect_filled(rect, 0.0, theme.accent_primary().to_egui());
    }
    let fg = if active {
        theme.text_on_accent()
    } else {
        theme.text_secondary()
    }
    .to_egui();
    let icon_center = egui::pos2(rect.left() + pad_x + icon_sz * 0.5, rect.center().y);
    let icon_rect = egui::Rect::from_center_size(icon_center, egui::vec2(icon_sz, icon_sz));
    glyph.image(icon_sz, fg).paint_at(ui, icon_rect);
    ui.painter().text(
        egui::pos2(icon_center.x + icon_sz * 0.5 + gap, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        fg,
    );
}

/// body(files) — well 안에 아이콘 + mono 경로 한 줄씩(design ellipsis 전사).
fn files_body_row(ui: &mut egui::Ui, theme: &Theme) {
    let footer_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let header_h = theme.spacing_md.value() * 2.0 + theme.item_height_tab.value();
    let type_bar_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let h = POPUP_H - header_h - type_bar_h - footer_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let margin = theme.spacing_md.value();
    let well = rect.shrink(margin);
    let p = ui.painter();
    p.rect(
        well,
        theme.corner_radius.value(),
        theme.bg_app().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        egui::StrokeKind::Inside,
    );

    let icon_sz = theme.icon_glyph_size_sm.value();
    let gap = theme.spacing_sm.value();
    let tx = well.left() + theme.spacing_sm.value();
    let mut ty = well.top() + theme.spacing_sm.value();
    let line_h = icon_sz.max(theme.font_size_term_sm.value()) + theme.spacing_xs.value();
    for path in FILE_PREVIEW {
        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(tx + icon_sz * 0.5, ty + line_h * 0.5),
            egui::vec2(icon_sz, icon_sz),
        );
        icons::FILE
            .image(icon_sz, theme.text_muted().to_egui())
            .paint_at(ui, icon_rect);
        p.text(
            egui::pos2(tx + icon_sz + gap, ty + line_h * 0.5),
            egui::Align2::LEFT_CENTER,
            path,
            egui::FontId::monospace(theme.font_size_term_sm.value()),
            theme.text_primary().to_egui(),
        );
        ty += line_h;
    }
}

/// body — well 안에 아이콘(30px 고정) + 메타 + "인라인 미리보기 없음" 안내를 상하좌우
/// 중앙 정렬(design jsx image 분기의 `cbWell` + `alignItems/justifyContent: center`).
fn image_body_row(ui: &mut egui::Ui, theme: &Theme) {
    let footer_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let header_h = theme.spacing_md.value() * 2.0 + theme.item_height_tab.value();
    let type_bar_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let h = POPUP_H - header_h - type_bar_h - footer_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let margin = theme.spacing_md.value();
    let well = rect.shrink(margin);
    let p = ui.painter();
    p.rect(
        well,
        theme.corner_radius.value(),
        theme.bg_app().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        egui::StrokeKind::Inside,
    );

    const IMAGE_BODY_ICON_SIZE: LogicalPx = LogicalPx(30.0);
    let gap = theme.spacing_sm.value();
    let icon_h = IMAGE_BODY_ICON_SIZE.value();
    let meta_h = theme.font_size_caption.value();
    let sub_h = theme.font_size_caption.value();
    let block_h = icon_h + gap + meta_h + theme.spacing_xs.value() + sub_h;
    let mut y = well.center().y - block_h / 2.0;

    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(well.center().x, y + icon_h / 2.0),
        egui::vec2(icon_h, icon_h),
    );
    icons::IMAGE
        .image(IMAGE_BODY_ICON_SIZE.value(), theme.text_muted().to_egui())
        .paint_at(ui, icon_rect);
    y += icon_h + gap;

    ui.painter().text(
        egui::pos2(well.center().x, y),
        egui::Align2::CENTER_TOP,
        IMAGE_META,
        egui::FontId::monospace(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );
    y += meta_h + theme.spacing_xs.value();

    ui.painter().text(
        egui::pos2(well.center().x, y),
        egui::Align2::CENTER_TOP,
        "No inline image preview",
        egui::FontId::proportional(theme.font_size_caption.value()),
        theme.text_disabled().to_egui(),
    );
}

/// "기타" 버킷 상태 — header + type-bar(Other 뱃지) + body(포맷 블록 나열) + footer
/// 4행(design 확정 결과).
fn other_popup(ui: &mut egui::Ui, theme: &Theme) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        header_row(ui, theme);
        other_type_bar_row(ui, theme);
        other_body_row(ui, theme);
        footer_row(
            ui,
            theme,
            format!("{} unrecognized formats", OTHER_SAMPLES.len()),
        );
    });
}

/// type-bar — 타입이 Other 하나뿐인 상태 — 아이콘(layers)+accent 뱃지(design 확정
/// 결과). 다른 단일 타입 뱃지([`type_bar_row`])와 동일 구조, 라벨만 다르다.
fn other_type_bar_row(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
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
    lui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    kit::icon(
        &mut lui,
        icons::LAYERS,
        theme.icon_glyph_size_sm.value(),
        theme.text_muted().to_egui(),
    );
    tag(&mut lui, theme, "Other", TagVariant::Accent, false);

    hline(ui, theme, rect.bottom());
}

/// body(기타) — well 안에 [`OTHER_SAMPLES`] 포맷 블록을 세로로 나열, 블록 사이 1px
/// separator(design `TypeBody` `other` 분기 1:1 전사). 목록 자체는 접지
/// 않는다(design §6.5 확정) — well 이 이미 스크롤 컨테이너다.
fn other_body_row(ui: &mut egui::Ui, theme: &Theme) {
    let footer_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let header_h = theme.spacing_md.value() * 2.0 + theme.item_height_tab.value();
    let type_bar_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let h = POPUP_H - header_h - type_bar_h - footer_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let margin = theme.spacing_md.value();
    let well = rect.shrink(margin);
    let p = ui.painter();
    p.rect(
        well,
        theme.corner_radius.value(),
        theme.bg_app().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        egui::StrokeKind::Inside,
    );

    let tx = well.left() + theme.spacing_sm.value();
    let mut ty = well.top() + theme.spacing_sm.value();
    let name_font = egui::FontId::monospace(theme.font_size_caption.value());
    let name_h = theme.font_size_caption.value();
    let line_h = theme.font_size_term_sm.value() + theme.spacing_xs.value();

    for (i, sample) in OTHER_SAMPLES.iter().enumerate() {
        if i > 0 {
            ty += theme.spacing_sm.value();
            p.hline(
                egui::Rangef::new(well.left(), well.right()),
                ty,
                egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
            );
            ty += theme.spacing_sm.value();
        }

        let name_w = ui
            .fonts(|f| {
                f.layout_no_wrap(
                    sample.name.to_owned(),
                    name_font.clone(),
                    egui::Color32::PLACEHOLDER,
                )
            })
            .size()
            .x;
        p.text(
            egui::pos2(tx, ty),
            egui::Align2::LEFT_TOP,
            sample.name,
            name_font.clone(),
            theme.text_secondary().to_egui(),
        );
        p.text(
            egui::pos2(tx + name_w + theme.spacing_sm.value(), ty),
            egui::Align2::LEFT_TOP,
            sample.size,
            name_font.clone(),
            theme.text_muted().to_egui(),
        );
        ty += name_h + theme.spacing_xs.value();

        for line in sample.preview.lines() {
            p.text(
                egui::pos2(tx, ty),
                egui::Align2::LEFT_TOP,
                line,
                egui::FontId::monospace(theme.font_size_term_sm.value()),
                theme.text_primary().to_egui(),
            );
            ty += line_h;
        }
        if let Some(n) = sample.more_lines {
            p.text(
                egui::pos2(tx, ty),
                egui::Align2::LEFT_TOP,
                format!("+{n} more lines"),
                name_font.clone(),
                theme.text_muted().to_egui(),
            );
            ty += line_h;
        }
    }
}

/// 정상 데이터 상태(HTML) — header + type-bar(배지 + 우측 Pretty print 체크박스) +
/// body(well, 원본/포맷 텍스트) + footer(mime · meta + Close) 4행.
fn data_popup_html(
    ui: &mut egui::Ui,
    theme: &Theme,
    pretty_on: &'static std::thread::LocalKey<RefCell<bool>>,
    content: &str,
) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        header_row(ui, theme);
        type_bar_row_html(ui, theme, pretty_on);
        body_row_text(ui, theme, content);
        footer_row_html(ui, theme, content);
    });
}

/// header — 클립보드 아이콘 + "Clipboard" + snapshot 뱃지 + 우측 close.
fn header_row(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_md.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

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
    kit::icon(
        &mut lui,
        icons::CLIPBOARD,
        theme.icon_glyph_size_md.value(),
        theme.text_muted().to_egui(),
    );
    lui.label(
        egui::RichText::new("Clipboard")
            .size(theme.font_size_max.value())
            .strong()
            .color(theme.text_primary().to_egui()),
    );
    tag(&mut lui, theme, "snapshot", TagVariant::Default, false);

    let close_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + pad_y),
        egui::pos2(rect.right() - pad_x, rect.top() + pad_y + ctrl_h),
    );
    let mut rui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(close_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    kit::icon(
        &mut rui,
        icons::CLOSE,
        theme.icon_glyph_size_sm.value(),
        theme.text_secondary().to_egui(),
    );

    hline(ui, theme, rect.bottom());
}

/// type-bar — 타입이 1개(Text)뿐인 상태는 세그먼트가 아니라 아이콘+accent 뱃지.
fn type_bar_row(ui: &mut egui::Ui, theme: &Theme) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
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
    lui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    kit::icon(
        &mut lui,
        icons::TEXT_LEFT,
        theme.icon_glyph_size_sm.value(),
        theme.text_muted().to_egui(),
    );
    tag(&mut lui, theme, "Text", TagVariant::Accent, false);

    hline(ui, theme, rect.bottom());
}

/// type-bar(HTML) — 좌측 아이콘+accent 뱃지는 동일, 우측 슬롯이 Pretty print
/// 체크박스로 스왑된다(design 확정 결과). 다른 타입의 빈 우측 슬롯과 달리
/// 여기만 실제 상호작용 위젯을 그린다 — 갤러리 specimen 이라 클릭 시 로컬
/// `thread_local` 상태가 토글된다(다른 특수 checkbox specimen, `settings.rs` 의
/// Colors override 행과 동일 패턴).
fn type_bar_row_html(
    ui: &mut egui::Ui,
    theme: &Theme,
    pretty_on: &'static std::thread::LocalKey<RefCell<bool>>,
) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
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
    lui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    kit::icon(
        &mut lui,
        icons::HTML,
        theme.icon_glyph_size_sm.value(),
        theme.text_muted().to_egui(),
    );
    tag(&mut lui, theme, "HTML", TagVariant::Accent, false);

    let mut rui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    pretty_on.with_borrow_mut(|checked| {
        checkbox(&mut rui, theme, checked, "Pretty print", true);
    });

    hline(ui, theme, rect.bottom());
}

/// body — well(border+radius+bg-app) 안에 mono 미리보기.
fn body_row(ui: &mut egui::Ui, theme: &Theme) {
    let footer_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let header_h = theme.spacing_md.value() * 2.0 + theme.item_height_tab.value();
    let type_bar_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let h = POPUP_H - header_h - type_bar_h - footer_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let margin = theme.spacing_md.value();
    let well = rect.shrink(margin);
    let p = ui.painter();
    p.rect(
        well,
        theme.corner_radius.value(),
        theme.bg_app().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        egui::StrokeKind::Inside,
    );

    let tx = well.left() + theme.spacing_sm.value();
    let mut ty = well.top() + theme.spacing_sm.value();
    let line_h = theme.font_size_term_sm.value() + theme.spacing_xs.value();
    for line in PREVIEW {
        p.text(
            egui::pos2(tx, ty),
            egui::Align2::LEFT_TOP,
            line,
            egui::FontId::monospace(theme.font_size_term_sm.value()),
            theme.text_primary().to_egui(),
        );
        ty += line_h;
    }
}

/// body(HTML) — [`body_row`]와 동일 well, 임의 문자열(원본 또는 prettify 결과)을
/// 줄 단위로 그린다. text 타입과 완전히 동일한 스타일(design 확정 결과).
fn body_row_text(ui: &mut egui::Ui, theme: &Theme, content: &str) {
    let footer_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let header_h = theme.spacing_md.value() * 2.0 + theme.item_height_tab.value();
    let type_bar_h = theme.spacing_sm.value() * 2.0 + theme.item_height_tab.value();
    let h = POPUP_H - header_h - type_bar_h - footer_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());

    let margin = theme.spacing_md.value();
    let well = rect.shrink(margin);
    let p = ui.painter();
    p.rect(
        well,
        theme.corner_radius.value(),
        theme.bg_app().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        egui::StrokeKind::Inside,
    );

    let tx = well.left() + theme.spacing_sm.value();
    let mut ty = well.top() + theme.spacing_sm.value();
    let line_h = theme.font_size_term_sm.value() + theme.spacing_xs.value();
    for line in content.lines() {
        p.text(
            egui::pos2(tx, ty),
            egui::Align2::LEFT_TOP,
            line,
            egui::FontId::monospace(theme.font_size_term_sm.value()),
            theme.text_primary().to_egui(),
        );
        ty += line_h;
    }
}

/// footer(HTML) — mime 뒤에 `· {n} chars · {n} line(s)` 메타를 결합해
/// [`footer_row`] 에 넘긴다(design 확정 결과 예시 `text/html · 312 chars · 1 line`).
fn footer_row_html(ui: &mut egui::Ui, theme: &Theme, content: &str) {
    let chars = content.chars().count();
    let lines = content.lines().count().max(1);
    let word = if lines == 1 { "line" } else { "lines" };
    footer_row(
        ui,
        theme,
        format!("text/html · {chars} chars · {lines} {word}"),
    );
}

/// footer — mime(mono caption) + 우측 Close(secondary).
///
/// 다섯 상태(text/plain · files · image · other · html)가 이 레이아웃을 그대로 쓰고
/// **왼쪽 mime 라벨만 다르다.** 그래서 라벨을 인자로 받는다 — 상태마다 함수를 두면
/// 레이아웃이 다섯 벌이 되고, 한 벌만 고친 채 나머지가 남는 어긋남이 조용히 생긴다.
fn footer_row(ui: &mut egui::Ui, theme: &Theme, mime: impl ToString) {
    let pad_x = theme.spacing_md.value();
    let pad_y = theme.spacing_sm.value();
    let ctrl_h = theme.item_height_tab.value();
    let h = pad_y * 2.0 + ctrl_h;
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.top() + theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );

    ui.painter().text(
        egui::pos2(rect.left() + pad_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        mime,
        egui::FontId::monospace(theme.font_size_caption.value()),
        theme.text_muted().to_egui(),
    );

    let btn_w = 64.0;
    let btn_rect = egui::Rect::from_min_max(
        egui::pos2(rect.right() - pad_x - btn_w, rect.top() + pad_y),
        egui::pos2(rect.right() - pad_x, rect.top() + pad_y + ctrl_h),
    );
    ui.painter().rect(
        btn_rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        btn_rect.center(),
        egui::Align2::CENTER_CENTER,
        "Close",
        egui::FontId::proportional(theme.font_size_term_sm.value()),
        theme.text_secondary().to_egui(),
    );
}

/// CenterState — 아이콘(28px) + 굵은 타이틀 + 옅은 부제 2줄, popup 높이 절반 정도.
fn center_popup(
    ui: &mut egui::Ui,
    theme: &Theme,
    glyph: MockGlyph,
    title: &str,
    sub: &str,
    danger: bool,
) {
    kit::frame_card(ui, theme, POPUP_W, kit::panel_fill(theme), |ui| {
        header_row(ui, theme);
        let h = (POPUP_H / 2.0).round();
        let w = ui.available_width();
        ui.allocate_ui_with_layout(
            egui::vec2(w, h),
            egui::Layout::centered_and_justified(egui::Direction::TopDown),
            |ui| {
                ui.vertical_centered(|ui| {
                    let tint = if danger {
                        theme.accent_danger().to_egui().gamma_multiply(0.9)
                    } else {
                        theme.text_muted().to_egui().gamma_multiply(0.5)
                    };
                    kit::icon(ui, glyph, CENTER_ICON_SIZE, tint);
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
                    ui.add_space(theme.spacing_xs.value());
                    ui.label(
                        egui::RichText::new(sub)
                            .size(theme.font_size_term_sm.value())
                            .color(theme.text_muted().to_egui()),
                    );
                });
            },
        );
    });
}

fn hline(ui: &mut egui::Ui, theme: &Theme, y: f32) {
    let rect = ui.max_rect();
    ui.painter().hline(
        rect.x_range(),
        y - theme.border_width.value() * 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
}
