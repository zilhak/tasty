//! Remote connections popup 의 **순수 시각** — 디자인
//! `ui_kits/terminal/overlays/remote_tool.jsx` 와 그 갤러리 미러
//! `gallery/overlays-shared.jsx` `RemoteFrame` / `LocalSshSection` 대응.
//!
//! ## 왜 이 crate 인가
//!
//! 갤러리(`tasty-gallery`)는 `tasty` bin 크레이트를 의존할 수 없다. 그래서 본체에만
//! 있는 그리기는 갤러리에서 **부를 수가 없고**, 전사 사본이 되면 두 자리가 말없이
//! 갈린다. 공용 crate 로 내리면 둘이 같은 함수를 부른다 —
//! [`crate::status_bar`] 의 `draw_status_bar_view` 가 같은 이유로 여기 있다.
//!
//! ## 이 crate 가 소유하지 않는 것 (=본체 wrapper 잔류)
//!
//! - **egui ctx memory** — 탭/폼 상태(`UiState`), 적용된 프로토콜 필터, 드롭다운
//!   열림 상태. 이 view 는 전부 인자로 받고 클릭만 돌려준다.
//! - **파일 IO** — `RemoteProfiles::load()` / `Passkeys::load()` / `~/.ssh/config`.
//! - **i18n** — 이 crate 는 `tasty-i18n` 을 의존하지 않는다(`status_bar`·`multi_select`
//!   와 동일 정책). 라벨·tooltip 문자열은 전부 props 로 주입받는다.
//! - **popup 배치·outside-click rect 보고** — 본체 popup 매니저 정책이다.

use std::collections::HashSet;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::tokens::{STRUCT_GAP_1, STRUCT_GAP_2};
use crate::vspace;

/// 프로토콜 필터 드롭다운의 최소 폭 — 체크박스 라벨 + unknown 배지가 한 줄에 들어가는
/// 폭. 디자인 `ProtocolFilter` 패널 236 에서 좌우 패딩을 뺀 값이다.
pub const FILTER_DROPDOWN_MIN_WIDTH: LogicalPx = LogicalPx(216.0);
/// 프로토콜 목록이 길어질 때의 스크롤 상한. 드롭다운이 팝업 밖으로 자라지 않게 한다.
pub const FILTER_DROPDOWN_MAX_HEIGHT: LogicalPx = LogicalPx(168.0);

// ── read-only selectable 텍스트 ──────────────────────────────────────────
//
// egui 의 `Label` 은 선택 복사가 안 되고 `TextEdit` 은 편집이 된다. 이 팝업의 값
// (호스트·포트·경로)은 **복사해서 쓰는 것**이 용건이라 선택은 필요하고 편집은 곤란하다.
// 그래서 매 프레임 지역 버퍼를 clone 해 넘긴다 — 타이핑해도 다음 프레임에 원래
// 텍스트로 돌아간다(편집 결과를 버린다).

/// [`selectable_text`] 의 줄바꿈 모드.
#[derive(Clone, Copy)]
pub enum TextWrap {
    /// 줄바꿈 없음, 콘텐츠 폭만큼만 차지(`egui::Label` 기본값과 동형).
    None,
    /// 가용 폭에서 여러 줄로 줄바꿈(`Label::wrap()` 과 동형).
    Wrap,
    /// 지정 폭에서 한 줄로 말줄임(`Label::truncate()` 과 동형) — `…` 로 elide.
    ///
    /// 폭이 `LogicalPx` 가 아닌 이유: 이 값은 곧장 egui `LayoutJob::wrap.max_width` 로
    /// 들어간다. 타입을 붙이면 만드는 자리 하나에서 벗기던 것을 쓰는 자리 하나에서
    /// 벗기게 될 뿐이라 총수가 그대로다.
    Truncate(f32),
}

/// selectable 텍스트 렌더 — 위 절 주석 참고.
pub fn selectable_text(
    ui: &mut egui::Ui,
    text: &str,
    color: impl Into<egui::Color32>,
    size: f32,
    monospace: bool,
    italic: bool,
    wrap: TextWrap,
) -> egui::Response {
    let color = color.into();
    let font_id = if monospace {
        egui::FontId::monospace(size)
    } else {
        egui::FontId::proportional(size)
    };
    let mut buffer = text.to_string();
    let mut layouter = move |ui: &egui::Ui, text: &str, wrap_width: f32| {
        let mut job = egui::text::LayoutJob::default();
        job.append(
            text,
            0.0,
            egui::TextFormat {
                font_id: font_id.clone(),
                color,
                italics: italic,
                ..Default::default()
            },
        );
        match wrap {
            TextWrap::None => job.wrap.max_width = f32::INFINITY,
            TextWrap::Wrap => {
                job.wrap.max_width = wrap_width;
                job.break_on_newline = true;
            }
            TextWrap::Truncate(w) => {
                job.wrap.max_width = w;
                job.wrap.max_rows = 1;
                job.wrap.break_anywhere = true;
            }
        }
        ui.fonts(|f| f.layout_job(job))
    };
    // Wrap 은 가용 폭까지 확장해야 실제로 줄바꿈된다. None/Truncate 는 콘텐츠(또는
    // truncate 결과) 폭만큼만 차지해야 `Label` 과 동일하게 부모 레이아웃(가로 나열,
    // 우측 정렬용 RTL 트릭 등)에 자연스럽게 맞물린다 — desired_width 를 고정폭으로
    // 주면 짧은 텍스트도 그 폭을 다 차지해 정렬이 깨진다.
    let desired_width = if matches!(wrap, TextWrap::Wrap) {
        f32::INFINITY
    } else {
        0.0
    };
    ui.add(
        egui::TextEdit::multiline(&mut buffer)
            .frame(false)
            .desired_rows(1)
            .desired_width(desired_width)
            .layouter(&mut layouter),
    )
}

/// [`selectable_text`] 축약형 — 줄바꿈 없음·italic 아님(가장 흔한 경우).
pub fn selectable_label(
    ui: &mut egui::Ui,
    text: &str,
    color: impl Into<egui::Color32>,
    size: f32,
    monospace: bool,
) -> egui::Response {
    selectable_text(ui, text, color, size, monospace, false, TextWrap::None)
}

// ── 구역 구분선 · 버튼 ───────────────────────────────────────────────────

/// 구역 구분선 — egui `Separator` 는 이 팝업의 패널 배경 위에서 사실상 비가시라
/// `border_strong` 색 hline 으로 직접 그린다.
pub fn hsep(ui: &mut egui::Ui, th: &Theme) {
    vspace(ui, STRUCT_GAP_2);
    let r = ui.max_rect();
    ui.painter().hline(
        r.x_range(),
        ui.cursor().top(),
        egui::Stroke::new(th.border_width.value(), th.border_strong()),
    );
    vspace(ui, STRUCT_GAP_2);
}

/// 디자인 secondary 버튼 (`Button.jsx --secondary`): surface-raised(surface0) 채움.
/// base(bg-panel) 패널 배경 위에서 한 단계 밝게 떠 보인다. (egui inactive 기본 버튼은
/// fill=base 라 base 패널 위에서 묻히므로 fill 을 명시한다.)
pub fn secondary_button(ui: &mut egui::Ui, th: &Theme, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .color(th.text_primary())
                .size(th.font_size_body.value()),
        )
        .fill(th.surface_raised())
        .stroke(egui::Stroke::new(
            th.border_width.value(),
            th.border_strong(),
        )),
    )
}

/// Primary 버튼 — accent 채움 + on-accent 텍스트. 디자인 `Button variant="primary"`.
pub fn primary_button(ui: &mut egui::Ui, th: &Theme, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .color(th.text_on_accent())
                .size(th.font_size_body.value()),
        )
        .fill(th.accent_primary()),
    )
}

/// Ghost 버튼 — 투명 배경 + secondary 텍스트(hover 시 overlay). 디자인
/// `Button variant="ghost"`.
pub fn ghost_button(ui: &mut egui::Ui, th: &Theme, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(
            egui::RichText::new(label)
                .color(th.text_secondary())
                .size(th.font_size_body.value()),
        )
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::NONE),
    )
}

/// 경고 배지 — 아이콘 없는 pill. `text`·`tooltip` 은 호출자가 번역해 넘긴다.
pub fn warn_badge(ui: &mut egui::Ui, th: &Theme, text: &str, tooltip: &str) {
    selectable_label(
        ui,
        &format!("⚠ {text}"),
        th.accent_warning(),
        th.font_size_caption.value(),
        false,
    )
    .on_hover_text(tooltip);
}

// ── 3탭 언더라인 탭 스트립 ───────────────────────────────────────────────

/// 언더라인 탭 스트립 한 프레임 분의 입력.
///
/// **밑줄이지 채움이 아니다** — 2026-09-17 디자인 결정(R1)이 둘을 갈랐다: 밑줄은
/// **view 를 바꾸고**(탭 스트립), 채움은 **값을 바꾼다**([`crate::segmented`]).
/// `surface-active` 는 행 선택 채움이라 어느 쪽도 아니다.
pub struct TabStripData<'a> {
    /// 탭 라벨(이미 번역된 것). 개수가 곧 탭 수다.
    pub labels: &'a [&'a str],
    /// 지금 켜진 탭의 인덱스.
    pub active: usize,
    /// 바 배경과 하단 separator 가 채울 가로 범위 — popup 전체폭이다. 탭 자체는
    /// 이 범위의 왼쪽에서 시작한다.
    pub x_range: egui::Rangef,
}

/// 언더라인 탭 스트립. 클릭된 탭 인덱스를 돌려준다(현재 탭 재클릭은 `None`).
///
/// 좌표를 직접 계산해 그린다(egui 자동 배치 우회) — 디자인 TabBtn 이 높이·패딩·gap 을
/// 고정값으로 정하고 있어 자동 배치로는 그 형상이 안 나온다.
pub fn draw_tab_strip(ui: &mut egui::Ui, th: &Theme, data: &TabStripData<'_>) -> Option<usize> {
    // 디자인 remote_tool.jsx TabBtn: 전체폭 bg-sidebar(mantle), height 35,
    // padding L8 / TabBtn padding 0 13 / gap 2.
    let tab_h = 36.0; // 디자인 TabBtn 35 + borderBottom 1 = 탭바 컨테이너 36
    let pad_l = 8.0; // 디자인 탭바 padding-left
    let pad_x = 13.0; // 디자인 TabBtn padding 0 13
    let gap = 2.0; // 디자인 탭바 gap
    let font = egui::FontId::proportional(th.font_size_body.value());

    let top = ui.cursor().top();
    let bar = egui::Rect::from_min_size(
        egui::pos2(data.x_range.min, top),
        egui::vec2(data.x_range.span(), tab_h),
    );
    // bg-sidebar 전체폭 + 하단 borderBottom separator (mantle 위 → surface1 근사).
    ui.painter().rect_filled(bar, 0.0, th.bg_sidebar());
    ui.painter().hline(
        data.x_range,
        bar.max.y,
        egui::Stroke::new(th.border_width.value(), th.border_strong()),
    );

    let mut clicked = None;
    let mut x = data.x_range.min + pad_l;
    for (i, label) in data.labels.iter().enumerate() {
        let on = i == data.active;
        let text_w = ui.fonts(|f| {
            f.layout_no_wrap((*label).to_string(), font.clone(), th.text_primary().into())
                .size()
                .x
        });
        let w = text_w + pad_x * 2.0;
        let rect = egui::Rect::from_min_size(egui::pos2(x, top), egui::vec2(w, tab_h));
        let resp = ui.interact(rect, ui.id().with((i, "rt_tab")), egui::Sense::click());
        if resp.hovered() && !on {
            ui.painter()
                .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
        }
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            font.clone(),
            if on {
                th.text_primary().into()
            } else {
                th.text_muted().into()
            },
        );
        // 활성 탭 하단 accent bar — separator 위에 그려 덮는다. 굵기는 다른 탭
        // 지표(explorer/preset)와 같은 `tab_indicator_width`, 덮을 separator 두께는
        // `border_width`. 둘 다 얇은 구조선이라 zoom 을 타지 않는다(값 불변).
        if on {
            ui.painter().hline(
                rect.x_range(),
                bar.max.y - th.border_width.value(),
                egui::Stroke::new(th.tab_indicator_width.value(), th.accent_primary()),
            );
        }
        if resp.clicked() && !on {
            clicked = Some(i);
        }
        x += w + gap;
    }
    // 탭바 영역만큼 커서 전진 → 다음 구역(콘텐츠)이 그 아래로.
    ui.allocate_rect(bar, egui::Sense::hover());
    clicked
}

// ── 로컬 ssh config 섹션 ─────────────────────────────────────────────────

/// 로컬 `~/.ssh/config` 행 하나 — tasty 레코드가 아니라 사용자 파일의 항목이다.
pub struct LocalSshHost<'a> {
    /// `Host` 별칭.
    pub alias: &'a str,
    /// 표시용 요약(`HostName[:Port]`). 표시 전용 — 가져오기에는 alias 만 쓴다.
    pub hint: &'a str,
    /// 이미 프로필로 가져왔으면 그 사실을 알리는 **번역된 캡션**. `None` 이면 미가져옴.
    pub imported_caption: Option<&'a str>,
}

/// 섹션 한 프레임 분의 입력. 문자열은 전부 호출자가 번역해 넘긴다.
pub struct LocalSshSectionData<'a> {
    /// 섹션 제목.
    pub heading: &'a str,
    /// 표시용 config 경로(`~/.ssh/config`).
    pub path: &'a str,
    /// 재로드 아이콘 tooltip.
    pub refresh_tooltip: &'a str,
    /// 가져오기 아이콘 tooltip.
    pub import_tooltip: &'a str,
    /// 호스트가 0 건일 때의 한 줄 — 원인(없음/못 읽음/정말 0 건)은 호출자가 가른다.
    pub empty_message: &'a str,
    pub hosts: &'a [LocalSshHost<'a>],
}

/// 섹션이 돌려주는 사용자 액션.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocalSshAction {
    /// 재로드 아이콘 클릭.
    Reload,
    /// `hosts[i]` 의 가져오기 클릭.
    Import(usize),
}

/// 로컬 ssh config 섹션 — 프로필 목록 **아래**에 구분선으로 갈라 붙는다.
pub fn draw_local_ssh_section(
    ui: &mut egui::Ui,
    th: &Theme,
    data: &LocalSshSectionData<'_>,
) -> Option<LocalSshAction> {
    let mut out = None;
    hsep(ui, th);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        selectable_label(
            ui,
            data.heading,
            th.text_secondary(),
            th.font_size_caption.value(),
            false,
        );
        selectable_label(
            ui,
            data.path,
            th.text_muted(),
            th.font_size_caption.value(),
            true,
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // 프로필 행의 재감지와 같은 글리프지만 하는 일이 다르다(원격 프로브가
            // 아니라 로컬 파일 재로드) — 툴팁으로 가른다.
            if ui
                .add(
                    egui::ImageButton::new(tasty_icons::REFRESH.image(
                        th.icon_glyph_size_row_action.value(),
                        th.text_muted().into(),
                    ))
                    .frame(false),
                )
                .on_hover_text(data.refresh_tooltip)
                .clicked()
            {
                out = Some(LocalSshAction::Reload);
            }
        });
    });
    ui.add_space(th.spacing_xs.value());
    if data.hosts.is_empty() {
        // 섹션을 통째로 숨기지 않는다 — 문구가 있어야 "가져올 게 없다" 와 "그런 기능이
        // 없다" 가 구분된다.
        selectable_text(
            ui,
            data.empty_message,
            th.text_muted(),
            th.font_size_caption.value(),
            false,
            true,
            TextWrap::None,
        );
        ui.add_space(th.spacing_xs.value());
        return out;
    }
    for (i, h) in data.hosts.iter().enumerate() {
        if draw_local_ssh_row(ui, th, h, data.import_tooltip) {
            out = Some(LocalSshAction::Import(i));
        }
    }
    out
}

/// alias 행 한 줄 — 이름 / hint caption / 우측 가져오기. 가져오기를 눌렀으면 `true`.
fn draw_local_ssh_row(
    ui: &mut egui::Ui,
    th: &Theme,
    h: &LocalSshHost<'_>,
    import_tooltip: &str,
) -> bool {
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = STRUCT_GAP_1.value();
            selectable_label(
                ui,
                h.alias,
                th.text_primary(),
                th.font_size_body.value(),
                false,
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
                selectable_label(
                    ui,
                    h.hint,
                    th.text_muted(),
                    th.font_size_caption.value(),
                    true,
                );
                if let Some(caption) = h.imported_caption {
                    selectable_label(
                        ui,
                        caption,
                        th.text_muted(),
                        th.font_size_caption.value(),
                        false,
                    );
                }
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_xs.value();
            // 이미 가져온 alias 는 비활성 — 같은 호스트를 두 번 등록하는 사고를 막는다.
            let btn = ui.add_enabled(
                h.imported_caption.is_none(),
                egui::ImageButton::new(tasty_icons::DOWNLOAD.image(
                    th.icon_glyph_size_row_action.value(),
                    th.text_muted().into(),
                ))
                .frame(false),
            );
            if h.imported_caption.is_none() && btn.on_hover_text(import_tooltip).clicked() {
                clicked = true;
            }
        });
    });
    ui.add_space(th.spacing_xs.value());
    clicked
}

// ── 프로토콜 필터 ────────────────────────────────────────────────────────

/// 필터 목록의 한 프로토콜.
pub struct ProtocolFilterItem<'a> {
    pub name: &'a str,
    /// tasty 가 전용 폼을 가진 kind 가 아님 — 경고 배지가 붙는다.
    pub unknown: bool,
}

/// 드롭다운 본문의 문자열(전부 번역된 것).
pub struct ProtocolFilterLabels<'a> {
    pub title: &'a str,
    pub select_all: &'a str,
    pub deselect_all: &'a str,
    pub reset: &'a str,
    pub apply: &'a str,
    pub unknown: &'a str,
    pub unknown_hint: &'a str,
}

/// 프로토콜 필터 버튼(funnel + 라벨). `filtered` 면 primary(accent), 아니면 secondary.
pub fn draw_protocol_filter_button(
    ui: &mut egui::Ui,
    th: &Theme,
    label: &str,
    filtered: bool,
) -> egui::Response {
    let text_col: egui::Color32 = if filtered {
        th.text_on_accent().into()
    } else {
        th.text_primary().into()
    };
    let fill: egui::Color32 = if filtered {
        th.accent_primary().into()
    } else {
        th.surface_raised().into()
    };
    let stroke = if filtered {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(th.border_width.value(), th.border_strong())
    };
    ui.add(
        egui::Button::image_and_text(
            tasty_icons::FUNNEL.image(th.icon_glyph_size_sm.value(), text_col),
            egui::RichText::new(label)
                .color(text_col)
                .size(th.font_size_body.value()),
        )
        .fill(fill)
        .stroke(stroke),
    )
}

/// 필터 드롭다운 **본문** — 체크박스 목록 + 모두선택/모두해제/초기화/적용.
///
/// 띄우는 것(popup 위젯·앵커·outside-click rect 보고)은 호출자 몫이다. 이 함수는
/// 넘겨받은 `ui` 안을 채우기만 한다 — 그래서 갤러리가 "열린 상태" 를 popup 없이
/// 카드 안에 그대로 그릴 수 있다.
///
/// `draft` 는 **제외 집합**이다(체크 = 미제외). 모두선택/모두해제/초기화는 여기서
/// 곧바로 `draft` 를 고치고, 적용만 `true` 로 보고한다 — Apply-on-confirm 이라
/// 적용 시점의 반영은 호출자가 한다.
pub fn draw_protocol_filter_body(
    ui: &mut egui::Ui,
    th: &Theme,
    items: &[ProtocolFilterItem<'_>],
    labels: &ProtocolFilterLabels<'_>,
    draft: &mut HashSet<String>,
) -> bool {
    let mut applied = false;
    ui.set_min_width(FILTER_DROPDOWN_MIN_WIDTH.value());
    selectable_label(
        ui,
        labels.title,
        th.text_muted(),
        th.font_size_caption.value(),
        true,
    );
    ui.add_space(th.spacing_xs.value());
    egui::ScrollArea::vertical()
        .max_height(FILTER_DROPDOWN_MAX_HEIGHT.value())
        .show(ui, |ui| {
            for item in items {
                ui.horizontal(|ui| {
                    // draft 는 제외 집합 → checked = 미제외.
                    let mut checked = !draft.contains(item.name);
                    if crate::checkbox(ui, th, &mut checked, item.name, true).changed() {
                        if checked {
                            draft.remove(item.name);
                        } else {
                            draft.insert(item.name.to_string());
                        }
                    }
                    if item.unknown {
                        warn_badge(ui, th, labels.unknown, labels.unknown_hint);
                    }
                });
            }
        });
    hsep(ui, th);
    // 일괄 조작: 모두 선택(=빈 제외) / 모두 해제(=전체 제외).
    ui.horizontal(|ui| {
        if ghost_button(ui, th, labels.select_all).clicked() {
            draft.clear();
        }
        if ghost_button(ui, th, labels.deselect_all).clicked() {
            *draft = items.iter().map(|i| i.name.to_string()).collect();
        }
    });
    // 초기화(=전체 선택) / 적용.
    ui.horizontal(|ui| {
        if ghost_button(ui, th, labels.reset).clicked() {
            draft.clear();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if primary_button(ui, th, labels.apply).clicked() {
                applied = true;
            }
        });
    });
    applied
}
