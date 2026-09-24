//! 본체와 갤러리가 공유하는 원격 연결 화면의 그리기 함수.
//! 폼·필터·팝업 상태, 파일 읽기, 번역, 팝업 배치는 호출자가 맡는다.

use std::collections::HashSet;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::tokens::{STRUCT_GAP_1, STRUCT_GAP_2};
use crate::vspace;
use crate::{TagVariant, tag};

/// 프로토콜 드롭다운의 본문 최소 폭. 테두리까지 포함한 디자인의 전체 폭과는 다르다.
/// 현재 값을 유지하며 프레임 여백은 별도로 더한다.
pub const FILTER_DROPDOWN_MIN_WIDTH: LogicalPx = LogicalPx(216.0);
/// 프로토콜 목록이 길어질 때의 스크롤 상한. 드롭다운이 팝업 밖으로 자라지 않게 한다.
pub const FILTER_DROPDOWN_MAX_HEIGHT: LogicalPx = LogicalPx(168.0);

// 복사할 수 있지만 변경 내용은 저장하지 않는 텍스트다. 매 프레임 원본으로 버퍼를 만든다.

/// [`selectable_text`] 의 줄바꿈 모드.
#[derive(Clone, Copy)]
pub enum TextWrap {
    /// 줄바꿈 없음, 콘텐츠 폭만큼만 차지(`egui::Label` 기본값과 동형).
    None,
    /// 가용 폭에서 여러 줄로 줄바꿈(`Label::wrap()` 과 동형).
    Wrap,
    /// 지정 폭에서 한 줄로 줄인다. 폭은 egui LayoutJob에 직접 전달한다.
    Truncate(f32),
}

/// 선택·복사 가능한 텍스트를 그린다. 편집 결과는 보존하지 않는다.
pub fn selectable_text(
    ui: &mut egui::Ui,
    text: &str,
    color: impl Into<egui::Color32>,
    size: f32,
    monospace: bool,
    italic: bool,
    wrap: TextWrap,
) -> egui::Response {
    selectable_text_tracked(ui, text, color, size, monospace, italic, wrap, 0.0)
}

/// 선택 가능한 텍스트에 논리 픽셀 단위 자간을 추가한다.
#[allow(clippy::too_many_arguments)] // reason: 한 줄 텍스트의 표현 축이 그만큼이다
pub fn selectable_text_tracked(
    ui: &mut egui::Ui,
    text: &str,
    color: impl Into<egui::Color32>,
    size: f32,
    monospace: bool,
    italic: bool,
    wrap: TextWrap,
    tracking: f32,
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
                extra_letter_spacing: tracking,
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
    // Wrap만 가용 폭을 채운다. 나머지는 내용 폭을 사용해 짧은 텍스트가 정렬 공간을 독점하지 않게 한다.
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

/// [`selectable_label`] 에 자간을 더한 것 — 대문자 섹션 헤딩용.
pub fn selectable_label_tracked(
    ui: &mut egui::Ui,
    text: &str,
    color: impl Into<egui::Color32>,
    size: f32,
    monospace: bool,
    tracking: f32,
) -> egui::Response {
    selectable_text_tracked(
        ui,
        text,
        color,
        size,
        monospace,
        false,
        TextWrap::None,
        tracking,
    )
}

// ── 구역 구분선 · 버튼 ───────────────────────────────────────────────────

/// 패널 위에서 보이도록 border_strong 색으로 구분선을 그린다.
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

/// 패널 배경과 구분되도록 surface-raised를 사용하는 보조 버튼.
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

/// 화면을 전환하는 밑줄 탭의 입력. 값을 고르는 segmented와 구분한다.
pub struct TabStripData<'a> {
    /// 탭 라벨(이미 번역된 것). 개수가 곧 탭 수다.
    pub labels: &'a [&'a str],
    /// 지금 켜진 탭의 인덱스.
    pub active: usize,
    /// 바 배경과 하단 separator 가 채울 가로 범위 — popup 전체폭이다. 탭 자체는
    /// 이 범위의 왼쪽에서 시작한다.
    pub x_range: egui::Rangef,
}

/// 클릭한 다른 탭의 인덱스를 반환한다. 현재 탭을 다시 누르면 None이다.
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
        // 활성 밑줄은 구분선 위에 그린다. 두 선의 굵기는 UI 배율과 무관하다.
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
    ui.allocate_rect(bar, egui::Sense::hover());
    clicked
}

// SSH 설정 항목은 프로필 목록 아래 별도 구역으로 표시한다.
// 이미 가져온 호스트는 버튼 대신 상태 태그를 표시하며, 설정 없음은 오류색으로 표시하지 않는다.

/// 대응하는 공용 토큰이 없는 디자인의 섹션 상단 여백.
const SSH_SECTION_MARGIN_TOP: LogicalPx = LogicalPx(10.0);
/// 헤더와 행의 간격. 같은 숫자의 상태 점 토큰과 역할이 달라 공유하지 않는다.
const SSH_GAP: LogicalPx = LogicalPx(6.0);

/// 대문자 헤딩의 자간 비율. 글꼴 크기에 곱해 논리 픽셀로 변환한다.
const SECTION_HEADING_TRACKING_EM: f32 = 0.06;

/// 로컬 `~/.ssh/config` 행 하나 — tasty 레코드가 아니라 사용자 파일의 항목이다.
pub struct LocalSshHost<'a> {
    /// `Host` 별칭.
    pub alias: &'a str,
    /// 표시용 요약(`user@host:port`). 표시 전용 — 가져오기에는 alias 만 쓴다.
    pub target: &'a str,
    /// 이미 프로필로 가져왔는가. 그러면 액션 대신 Tag 가 붙는다.
    pub in_profiles: bool,
}

/// 섹션 한 프레임 분의 입력. 문자열은 전부 호출자가 번역해 넘긴다.
pub struct LocalSshSectionData<'a> {
    /// 섹션 제목. 그리기 직전에 대문자로 바꾼다(canonical `textTransform: uppercase`).
    pub heading: &'a str,
    /// 표시용 config 경로(`~/.ssh/config`).
    pub path: &'a str,
    /// 이미 가져온 호스트에 붙는 Tag 라벨.
    pub in_profiles_tag: &'a str,
    /// 미가져온 호스트의 ghost 액션 라벨.
    pub add_label: &'a str,
    /// 호스트가 0 건일 때의 한 줄 — 원인(없음/못 읽음/정말 0 건)은 호출자가 가른다.
    pub empty_message: &'a str,
    pub hosts: &'a [LocalSshHost<'a>],
}

/// 본문만 안쪽으로 들여쓴다. 구분선에는 이 여백을 적용하지 않는다.
fn ssh_inset<R>(ui: &mut egui::Ui, th: &Theme, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(th.spacing_xs.value() as i8, 0))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// 프로필 아래에 SSH 설정 항목을 표시하고 가져오기를 누른 인덱스를 반환한다.
pub fn draw_local_ssh_section(
    ui: &mut egui::Ui,
    th: &Theme,
    data: &LocalSshSectionData<'_>,
) -> Option<usize> {
    let mut clicked = None;
    // 프로필과 별도 구역임을 구분선으로 표시한다.
    vspace(ui, SSH_SECTION_MARGIN_TOP);
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        egui::Stroke::new(th.border_width.value(), th.border_frame()),
    );
    vspace(ui, th.spacing_sm);

    ssh_inset(ui, th, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = SSH_GAP.value();
            selectable_label_tracked(
                ui,
                &data.heading.to_uppercase(),
                th.text_secondary(),
                th.font_size_caption.value(),
                false,
                th.font_size_caption.value() * SECTION_HEADING_TRACKING_EM,
            );
            selectable_label(
                ui,
                data.path,
                th.text_muted(),
                th.font_size_caption.value(),
                true,
            );
            if !data.hosts.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    selectable_label(
                        ui,
                        &data.hosts.len().to_string(),
                        th.text_muted(),
                        th.font_size_caption.value(),
                        true,
                    );
                });
            }
        });
    });
    vspace(ui, SSH_GAP);

    if data.hosts.is_empty() {
        // 비어 있는 이유를 표시하되 설정 부재를 경고색으로 표현하지 않는다.
        ssh_inset(ui, th, |ui| {
            selectable_text(
                ui,
                data.empty_message,
                th.text_muted(),
                th.font_size_term_sm.value(),
                false,
                false,
                TextWrap::Wrap,
            );
        });
        vspace(ui, SSH_GAP);
        return None;
    }
    for (i, h) in data.hosts.iter().enumerate() {
        if draw_local_ssh_row(ui, th, h, data) {
            clicked = Some(i);
        }
    }
    clicked
}

/// alias 행 한 줄 — 2 줄 본문 + 우측 Tag 또는 ghost 액션. 액션을 눌렀으면 `true`.
fn draw_local_ssh_row(
    ui: &mut egui::Ui,
    th: &Theme,
    h: &LocalSshHost<'_>,
    data: &LocalSshSectionData<'_>,
) -> bool {
    let mut clicked = false;
    vspace(ui, SSH_GAP);
    ssh_inset(ui, th, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
            // 긴 별칭이 오른쪽 버튼을 밀지 않도록 버튼 영역을 먼저 확보한다.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if h.in_profiles {
                    // 이미 가져온 항목은 재등록 버튼 대신 상태를 표시한다.
                    tag(ui, th, data.in_profiles_tag, TagVariant::Default, false);
                } else if crate::Button::new(data.add_label)
                    .variant(crate::ButtonVariant::Ghost)
                    .size(crate::ControlSize::Sm)
                    .leading_icon(&|ui, rect, c| {
                        tasty_icons::PLUS.image(rect.height(), c).paint_at(ui, rect)
                    })
                    .show(ui, th)
                    .clicked()
                {
                    clicked = true;
                }
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.spacing_mut().item_spacing.y = STRUCT_GAP_1.value();
                    let w = ui.available_width();
                    selectable_text(
                        ui,
                        h.alias,
                        th.text_secondary(),
                        th.font_size_body.value(),
                        false,
                        false,
                        TextWrap::Truncate(w),
                    );
                    selectable_text(
                        ui,
                        h.target,
                        th.text_muted(),
                        th.font_size_caption.value(),
                        true,
                        false,
                        TextWrap::Truncate(w),
                    );
                });
            });
        });
    });
    vspace(ui, SSH_GAP);
    ui.painter().hline(
        ui.max_rect().x_range(),
        ui.cursor().top(),
        egui::Stroke::new(th.border_width.value(), th.separator),
    );
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

/// 프로토콜 제외 집합 draft를 편집한다. 체크된 항목은 제외되지 않은 항목이다.
/// 전체 선택·해제·초기화는 draft를 즉시 바꾸며 적용 버튼만 true를 반환한다.
/// 팝업 배치와 실제 필터 적용은 호출자가 처리한다.
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
        .drag_to_scroll(false)
        .show(ui, |ui| {
            for item in items {
                ui.horizontal(|ui| {
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
    ui.horizontal(|ui| {
        if ghost_button(ui, th, labels.select_all).clicked() {
            draft.clear();
        }
        if ghost_button(ui, th, labels.deselect_all).clicked() {
            *draft = items.iter().map(|i| i.name.to_string()).collect();
        }
    });
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
