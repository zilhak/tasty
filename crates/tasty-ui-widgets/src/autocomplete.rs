//! `AutoComplete` — 자유입력 트리거 + 후보 드롭다운(typeahead).
//!
//! 닫힌 선택(Select)이 아니라 **입력하면 후보가 좁혀지는** 진짜 typeahead 다. 새 디자인
//! 언어가 아니라 기존 프리미티브의 **합성**이다(디자인 `forms/AutoComplete`):
//! - 트리거(편집 필드) = [`crate::Input`] 그대로(mono 변형·leading 아이콘·focus ring).
//! - 드롭다운 컨테이너 = `menu container` 토큰(surface-raised / border-default /
//!   corner-radius) + `shadow_popover` lift.
//! - 후보 행 = `navigation/MenuItem` 언어(control-height 28, space-md 패딩). 단 경로
//!   가독을 위해 우측 clip 대신 **middle-ellipsis**(파일명 꼬리 보존)로 그린다.
//!
//! 계약(디자인 `AutoComplete.jsx`):
//! - **필터**: `match` = `Substring`(기본·경로친화) · `Prefix` · `None`. 트리거 텍스트를
//!   질의로 후보를 좁힌다(대소문자 무시). `None` 은 필터 없이 전체 노출(구 히스토리 동작).
//! - **highlight**: 매치 구간을 accent-primary 로 강조(egui 폰트 weight 한계상 색만 —
//!   `button.rs` semibold 관례와 동일).
//! - **maxDropdownHeight**: 기본 `autocomplete_max_height`(220 ≈ 7행) 초과 시 리스트
//!   **내부 세로 스크롤** + shrink-to-fit.
//! - **hover vs keyboard-active 2단계 분리**: pointer hover = `overlay-hover`(약한 워시),
//!   ↑/↓ keyboard 커서 = `surface-active`(더 진함). 겹치면 keyboard-active 우선.
//! - **Pick 계약**: 확정 시 **선택된 후보 문자열을 직접 반환**한다(필터된 가시 목록 기준).
//!   호출측이 원본 인덱스로 되돌릴 필요가 없어 필터 도입에 따른 인덱스 오매핑을 원천 차단.
//! - empty/no-match: muted `empty_label` 행 1개.
//!
//! 키보드 내비(↑/↓/Enter/Esc)는 **스캐폴드**다 — 실제 키 forward 는 plugin 주소창 배선에서
//! 종단 검증한다. 여기선 action 반환 골격과 순수 필터/인덱스/ellipsis 로직만 확정한다
//! (단위테스트로 회귀 격리).

use tasty_type_appearance::theme::Theme;

use crate::icon_button::IconPainter;
use crate::keyboard_cursor;

/// 후보 필터 모드.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchMode {
    /// 후보 라벨 어디든 질의가 부분 문자열로 포함(기본 — 경로친화).
    Substring,
    /// 후보 라벨이 질의로 시작.
    Prefix,
    /// 필터 없음 — 전체 후보 노출(구 히스토리 동작).
    None,
}

/// AutoComplete 한 프레임의 사용자 행위.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutoCompleteAction {
    /// 아무 일도 없음(키보드 커서 이동만 포함 — 그건 `active` 갱신으로 반영됨).
    None,
    /// 입력 버퍼 텍스트가 바뀜(질의 변경).
    Edited,
    /// 후보 행을 확정(클릭 또는 keyboard-active 행에서 Enter). **선택된 후보 문자열**을
    /// 담는다(필터된 가시 목록 기준 — 호출측이 인덱스를 역매핑하지 않는다).
    Pick(String),
    /// active 행 없이 Enter — 현재 버퍼를 그대로 확정(navigate).
    Submit,
    /// Esc — 드롭다운 닫기 + 버퍼 원복(원복은 호출측 책임).
    Cancel,
}

/// `AutoComplete::show` 한 프레임 결과 — 행위 + 트리거 응답.
///
/// 트리거(`Input`)의 [`egui::Response`] 를 그대로 노출한다. 호출측이 포커스 상태
/// (`has_focus`/`gained_focus`/`lost_focus`)로 편집모드를 추적하거나 Esc 확정 후
/// `surrender_focus()` 하도록 하기 위함이다(플러그인 주소창 배선 계약).
pub struct AutoCompleteResponse {
    /// 이번 프레임의 사용자 행위.
    pub action: AutoCompleteAction,
    /// 트리거(편집 필드) 응답.
    pub response: egui::Response,
}

/// AutoComplete 빌더. 프레젠테이션 설정만 담고, 상태(`buf`/`entries`/`active`)는 `show` 인자.
pub struct AutoComplete<'a> {
    id_salt: &'a str,
    placeholder: &'a str,
    empty_label: &'a str,
    mono: bool,
    enabled: bool,
    /// 트리거·드롭다운 폭. `None` 이면 가용 폭.
    width: Option<f32>,
    /// 트리거 leading 아이콘(선택).
    icon: Option<IconPainter<'a>>,
    /// 후보 행 공통 leading 아이콘(선택 — 히스토리 dropdown 은 파일 아이콘).
    row_icon: Option<IconPainter<'a>>,
    /// 트리거 텍스트 색 override(선택). 미지정 시 `input_fg`(text-primary).
    trigger_text_color: Option<egui::Color32>,
    /// 필터 모드. 기본 `Substring`.
    match_mode: MatchMode,
    /// 매치 구간 highlight 여부. 기본 `true`.
    highlight: bool,
    /// 드롭다운 최대 높이 override. `None` 이면 `theme.autocomplete_max_height()`(220).
    max_dropdown_height: Option<f32>,
}

impl<'a> AutoComplete<'a> {
    pub fn new(id_salt: &'a str) -> Self {
        Self {
            id_salt,
            placeholder: "",
            empty_label: "",
            mono: false,
            enabled: true,
            width: None,
            icon: None,
            row_icon: None,
            trigger_text_color: None,
            match_mode: MatchMode::Substring,
            highlight: true,
            max_dropdown_height: None,
        }
    }

    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// 후보 0개일 때 드롭다운에 표시할 비상호작용 라벨(예: "No matching path").
    pub fn empty_label(mut self, empty_label: &'a str) -> Self {
        self.empty_label = empty_label;
        self
    }

    /// 경로 변형 — 입력·후보 텍스트 monospace + caption(11).
    pub fn mono(mut self, mono: bool) -> Self {
        self.mono = mono;
        self
    }

    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }

    /// 트리거 leading 아이콘 painter(text-muted 색으로 호출됨).
    pub fn icon(mut self, icon: IconPainter<'a>) -> Self {
        self.icon = Some(icon);
        self
    }

    /// 후보 행 공통 leading 아이콘 painter(text-muted 색으로 호출됨).
    pub fn row_icon(mut self, row_icon: IconPainter<'a>) -> Self {
        self.row_icon = Some(row_icon);
        self
    }

    /// 트리거 텍스트 색 override — 미지정 시 `input_fg`(text-primary). 주소창 idle 표시가
    /// 비편집 시 text-secondary 로 낮추는 용도. 값은 `Theme` 토큰에서 파생해야 한다.
    pub fn trigger_text_color(mut self, color: egui::Color32) -> Self {
        self.trigger_text_color = Some(color);
        self
    }

    /// 필터 모드 — 기본 `Substring`. `None` 이면 필터 없이 전체 후보 노출.
    pub fn match_mode(mut self, match_mode: MatchMode) -> Self {
        self.match_mode = match_mode;
        self
    }

    /// 매치 구간 highlight — 기본 `true`. 필터를 쓰지 않는(=`None`) 용례는 꺼도 좋다.
    pub fn highlight(mut self, highlight: bool) -> Self {
        self.highlight = highlight;
        self
    }

    /// 드롭다운 최대 높이(logical px) override. 미지정 시 `autocomplete_max_height`(220).
    pub fn max_dropdown_height(mut self, max_height: f32) -> Self {
        self.max_dropdown_height = Some(max_height);
        self
    }

    /// 트리거 + (포커스 시) 후보 드롭다운을 그린다.
    ///
    /// - `buf`: 편집 버퍼(트리거 텍스트 = 필터 질의).
    /// - `entries`: 후보(원본 목록). 필터가 켜지면 이 목록을 좁혀 그린다.
    /// - `active`: keyboard-active 행 index(호출측 소유 상태, **필터된 가시 목록 기준**).
    ///
    /// 드롭다운은 트리거 아래 floating popover(`shadow_popover` lift)로 뜬다 —
    /// 주변 레이아웃을 밀어내지 않는다(§3 브라우저 주소창형).
    pub fn show(
        self,
        ui: &mut egui::Ui,
        theme: &Theme,
        buf: &mut String,
        entries: &[&str],
        active: &mut Option<usize>,
    ) -> AutoCompleteResponse {
        let width = self.width.unwrap_or_else(|| ui.available_width());

        // 트리거 = Input 그대로(mono·아이콘·focus ring 계약을 재사용).
        let mut trigger = crate::Input::new()
            .placeholder(self.placeholder)
            .mono(self.mono)
            .enabled(self.enabled)
            .width(width);
        if let Some(icon) = self.icon {
            trigger = trigger.icon(icon);
        }
        if let Some(color) = self.trigger_text_color {
            trigger = trigger.text_color(color);
        }
        let resp = trigger.show(ui, theme, buf);

        let mut action = if resp.changed() {
            AutoCompleteAction::Edited
        } else {
            AutoCompleteAction::None
        };

        // 포커스 중이면 열림(브라우저 주소창형 — 포커스 즉시 후보 노출).
        let focused = self.enabled && resp.has_focus();
        // singleline TextEdit 은 Enter/Esc 에서 **같은 프레임에 포커스를 넘긴다** → 그
        // 프레임엔 `has_focus()` 가 이미 false 다. Enter/Esc 확정을 놓치지 않도록 이번
        // 프레임에 포커스를 잃은 경우(`lost_focus`)까지 "관여(engaged)"로 본다.
        let engaged = self.enabled && (resp.has_focus() || resp.lost_focus());
        if !engaged {
            return AutoCompleteResponse {
                action,
                response: resp,
            };
        }

        // 질의 = 트리거 버퍼. 이 값으로 후보를 좁히고(typeahead) 매치 구간을 강조한다.
        let query = buf.clone();
        let filtered = filter_entries(entries, &query, self.match_mode);

        let (down, up, enter, esc) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        let n = filtered.len();
        // active 는 필터된 가시 목록 기준. 필터가 좁혀지면 범위를 벗어날 수 있어 clamp.
        if let Some(i) = *active {
            *active = if n == 0 {
                Option::None
            } else {
                Some(i.min(n - 1))
            };
        }
        // active 이동·드롭다운 렌더는 실제 포커스(열림)일 때만. 닫히는 프레임엔 스킵.
        // 커서 규약(끝에서 순환·첫 오픈은 진행 방향 끝)은 [`crate::keyboard_cursor`] 한 벌을
        // 부른다 — 여기엔 비활성 행 개념이 없어 마스크는 `None`.
        if focused {
            if down {
                *active = keyboard_cursor::step_active(*active, n, None, true);
            }
            if up {
                *active = keyboard_cursor::step_active(*active, n, None, false);
            }
        }

        // 드롭다운 — 트리거 아래 floating(레이아웃 불변). space-xs 오프셋.
        let clicked = if focused {
            let area_id = ui.make_persistent_id(("tasty_autocomplete", self.id_salt));
            let origin = resp.rect.left_bottom() + egui::vec2(0.0, theme.spacing_xs.value());
            let max_height = self
                .max_dropdown_height
                .unwrap_or_else(|| theme.autocomplete_max_height().value());
            egui::Area::new(area_id)
                .order(egui::Order::Foreground)
                .fixed_pos(origin)
                .constrain(true)
                .show(ui.ctx(), |ui| {
                    ui.set_width(width);
                    autocomplete_dropdown(
                        ui,
                        theme,
                        self.id_salt,
                        &filtered,
                        self.empty_label,
                        self.mono,
                        self.row_icon,
                        *active,
                        &query,
                        self.highlight,
                        max_height,
                    )
                })
                .inner
        } else {
            Option::None
        };

        // 행위 우선순위: Esc > Enter/click > Edited.
        if esc {
            action = AutoCompleteAction::Cancel;
        } else if enter {
            action = active
                .and_then(|i| filtered.get(i))
                .map(|s| AutoCompleteAction::Pick((*s).to_string()))
                .unwrap_or(AutoCompleteAction::Submit);
        } else if let Some(s) = clicked.and_then(|i| filtered.get(i)) {
            action = AutoCompleteAction::Pick((*s).to_string());
        }
        AutoCompleteResponse {
            action,
            response: resp,
        }
    }
}

/// 드롭다운 컨테이너 + (스크롤되는) 후보 행을 **현재 ui 위치**에 그린다(호출측이 Area/inline 결정).
///
/// 컨테이너 = surface-raised / border-default 1px / corner-radius / `shadow_popover` lift.
/// 행 = MenuItem 언어(control-height, space-md 패딩) + middle-ellipsis 경로 + 매치 highlight.
/// `active`(keyboard 커서)는 surface-active, pointer hover 는 overlay-hover(2단계 분리).
/// `entries` 는 **이미 필터된 가시 목록**이고, 리스트가 `max_height` 를 넘으면 내부 세로
/// 스크롤(적으면 shrink-to-fit). 반환: 클릭된 행 index(가시 목록 기준).
#[allow(clippy::too_many_arguments)]
pub fn autocomplete_dropdown(
    ui: &mut egui::Ui,
    theme: &Theme,
    id_salt: &str,
    entries: &[&str],
    empty_label: &str,
    mono: bool,
    row_icon: Option<IconPainter<'_>>,
    active: Option<usize>,
    query: &str,
    highlight: bool,
    max_height: f32,
) -> Option<usize> {
    let shadow = theme.shadow_popover().to_egui();
    let mut clicked = None;
    egui::Frame::new()
        .fill(theme.surface_raised().to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            theme.border_default().to_egui(),
        ))
        .corner_radius(theme.corner_radius.value())
        .shadow(shadow)
        .inner_margin(egui::Margin::same(theme.spacing_xs.value() as i8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if entries.is_empty() {
                empty_row(ui, theme, empty_label);
            } else {
                // maxDropdownHeight 초과 시 내부 스크롤, 적으면 shrink-to-fit.
                egui::ScrollArea::vertical()
                    .id_salt(("tasty_autocomplete_list", id_salt))
                    .max_height(max_height)
                    .auto_shrink([true, true])
                    .show(ui, |ui| {
                        for (i, entry) in entries.iter().enumerate() {
                            if candidate_row(
                                ui,
                                theme,
                                entry,
                                mono,
                                row_icon,
                                active == Some(i),
                                query,
                                highlight,
                            )
                            .clicked()
                            {
                                clicked = Some(i);
                            }
                        }
                    });
            }
        });
    clicked
}

/// 후보 행 하나 — MenuItem 구조 전사 + middle-ellipsis + 매치 highlight. 클릭 응답 반환.
#[allow(clippy::too_many_arguments)]
fn candidate_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    path: &str,
    mono: bool,
    row_icon: Option<IconPainter<'_>>,
    keyboard_active: bool,
    query: &str,
    highlight: bool,
) -> egui::Response {
    let height = theme.menu_item_height().value();
    let pad_x = theme.menu_item_padding_x().value();
    let gap = theme.spacing_sm.value();
    let radius = theme.menu_item_radius().value();
    let icon_glyph = theme.icon_glyph_size_md.value();
    let font = row_font(theme, mono);
    let width = ui.available_width();

    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());

    // 배경: keyboard-active → surface-active(진함), hover → overlay-hover(약함). 2단계 분리.
    if keyboard_active {
        ui.painter()
            .rect_filled(rect, radius, theme.surface_active().to_egui());
    } else if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            radius,
            theme.menu_item_bg_hover().to_egui_premultiplied(),
        );
    }

    // fg: idle text-secondary, hover/active text-primary (MenuItem hover 승격).
    let highlighted = keyboard_active || resp.hovered();
    let fg = if highlighted {
        theme.text_primary().to_egui()
    } else {
        theme.text_secondary().to_egui()
    };
    let match_fg = theme.accent_primary().to_egui();

    let mut x = rect.left() + pad_x;
    if let Some(paint) = row_icon {
        let irect = egui::Rect::from_center_size(
            egui::pos2(x + icon_glyph * 0.5, rect.center().y),
            egui::vec2(icon_glyph, icon_glyph),
        );
        paint(ui, irect, theme.text_muted().to_egui());
        x += icon_glyph + gap;
    }

    // 라벨 — 남은 폭에 middle-ellipsis(파일명 꼬리 보존).
    let avail = (rect.right() - pad_x - x).max(0.0);
    let painter = ui.painter();
    let shown = elide_middle(path, avail, |s| {
        painter
            .layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
            .rect
            .width()
    });

    // 매치 구간 강조 — 표시(가능하면 elide 된) 문자열에서 첫 매치 run 을 accent 색으로.
    let run = if highlight {
        match_run(&shown, query)
    } else {
        None
    };
    let center_y = rect.center().y;
    match run {
        Some((s, e)) => {
            let chars: Vec<char> = shown.chars().collect();
            let pre: String = chars[..s].iter().collect();
            let mid: String = chars[s..e].iter().collect();
            let post: String = chars[e..].iter().collect();
            for (seg, color) in [(pre, fg), (mid, match_fg), (post, fg)] {
                if seg.is_empty() {
                    continue;
                }
                let g = painter.layout_no_wrap(seg, font.clone(), egui::Color32::PLACEHOLDER);
                let pos = egui::pos2(x, center_y - g.rect.height() * 0.5);
                let w = g.rect.width();
                painter.galley(pos, g, color);
                x += w;
            }
        }
        None => {
            let g = painter.layout_no_wrap(shown, font, egui::Color32::PLACEHOLDER);
            let pos = egui::pos2(x, center_y - g.rect.height() * 0.5);
            painter.galley(pos, g, fg);
        }
    }

    resp
}

/// empty 행 — 비상호작용 muted 라벨(hover/선택 chrome 없음).
fn empty_row(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    let height = theme.menu_item_height().value();
    let pad_x = theme.menu_item_padding_x().value();
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    let g = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme.font_size_body.value()),
        egui::Color32::PLACEHOLDER,
    );
    let pos = egui::pos2(rect.left() + pad_x, rect.center().y - g.rect.height() * 0.5);
    ui.painter().galley(pos, g, theme.text_muted().to_egui());
}

fn row_font(theme: &Theme, mono: bool) -> egui::FontId {
    if mono {
        egui::FontId::monospace(theme.font_size_caption.value())
    } else {
        egui::FontId::proportional(theme.font_size_body.value())
    }
}

/// 후보를 질의로 좁힌다(대소문자 무시). 질의가 비었거나 `None` 모드면 전체를 반환.
/// 순서는 원본 순서 보존. 반환은 **가시 목록**이며 `Pick` 은 이 목록의 문자열을 돌려준다.
fn filter_entries<'a>(entries: &[&'a str], query: &str, mode: MatchMode) -> Vec<&'a str> {
    if query.is_empty() || matches!(mode, MatchMode::None) {
        return entries.to_vec();
    }
    let q = query.to_lowercase();
    entries
        .iter()
        .copied()
        .filter(|e| {
            let l = e.to_lowercase();
            match mode {
                MatchMode::Prefix => l.starts_with(&q),
                MatchMode::Substring => l.contains(&q),
                MatchMode::None => true,
            }
        })
        .collect()
}

/// `text` 안에서 `query` 의 첫 대소문자 무시 매치 구간을 **char 인덱스** `(start, end)` 로
/// 반환. 질의가 비었거나 매치가 없으면 `None`. char 단위 비교라 유니코드 경계도 안전.
fn match_run(text: &str, query: &str) -> Option<(usize, usize)> {
    let q: Vec<char> = query.chars().collect();
    if q.is_empty() {
        return None;
    }
    let t: Vec<char> = text.chars().collect();
    if q.len() > t.len() {
        return None;
    }
    for start in 0..=(t.len() - q.len()) {
        if (0..q.len()).all(|k| char_eq_ci(t[start + k], q[k])) {
            return Some((start, start + q.len()));
        }
    }
    None
}

/// 두 char 를 대소문자 무시 비교. ASCII 밖(유니코드)도 `to_lowercase` 폴딩으로 처리.
fn char_eq_ci(a: char, b: char) -> bool {
    a == b || a.to_lowercase().eq(b.to_lowercase())
}

/// `max_w` 폭에 맞도록 문자열 가운데를 `…` 로 접는다(머리/꼬리 보존, 꼬리 우선).
/// `measure` 는 문자열 폭(px). 전체가 들어가면 원문 그대로.
fn elide_middle(text: &str, max_w: f32, measure: impl Fn(&str) -> f32) -> String {
    if measure(text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n <= 1 {
        return text.to_string();
    }
    // keep = 유지할 원문 문자 수(가운데 `…` 제외). 꼬리에 여분을 줘 파일명을 더 보존.
    for keep in (1..n).rev() {
        let head = keep / 2;
        let tail = keep - head;
        let mut s = String::with_capacity(keep + 3);
        s.extend(&chars[..head]);
        s.push('…');
        s.extend(&chars[n - tail..]);
        if measure(&s) <= max_w {
            return s;
        }
    }
    "…".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elide_middle_keeps_head_and_tail() {
        // 각 문자 폭 1 로 측정.
        let measure = |s: &str| s.chars().count() as f32;
        // 충분히 넓으면 원문.
        assert_eq!(elide_middle("abcdef", 10.0, measure), "abcdef");
        // 좁으면 가운데 `…`, 머리·꼬리 보존(꼬리 우선).
        let out = elide_middle("/home/user/notes/readme.md", 9.0, measure);
        assert!(out.contains('…'), "must contain ellipsis: {out}");
        assert!(measure(&out) <= 9.0, "must fit: {out} ({})", measure(&out));
        assert!(out.starts_with('/'), "head preserved: {out}");
        assert!(out.ends_with("md"), "tail (filename) preserved: {out}");
    }

    #[test]
    fn elide_middle_degenerate() {
        let measure = |s: &str| s.chars().count() as f32;
        // 0 폭이라도 최소 `…`.
        assert_eq!(elide_middle("abcdef", 0.0, measure), "…");
        // 단일 문자는 접지 않음.
        assert_eq!(elide_middle("x", 0.0, measure), "x");
    }

    #[test]
    fn filter_substring_prefix_none() {
        let items = [
            "~/Downloads",
            "~/work/tasty",
            "~/work/tasty-ui/src",
            "~/.config/tasty",
        ];
        // substring: 어디든 포함.
        assert_eq!(
            filter_entries(&items, "tasty", MatchMode::Substring),
            vec!["~/work/tasty", "~/work/tasty-ui/src", "~/.config/tasty"]
        );
        // prefix: 시작만.
        assert_eq!(
            filter_entries(&items, "~/work", MatchMode::Prefix),
            vec!["~/work/tasty", "~/work/tasty-ui/src"]
        );
        // none: 항상 전체.
        assert_eq!(
            filter_entries(&items, "tasty", MatchMode::None),
            items.to_vec()
        );
        // 빈 질의: 전체.
        assert_eq!(
            filter_entries(&items, "", MatchMode::Substring),
            items.to_vec()
        );
        // 대소문자 무시.
        assert_eq!(
            filter_entries(&items, "DOWN", MatchMode::Substring),
            vec!["~/Downloads"]
        );
    }

    #[test]
    fn pick_returns_visible_string_not_original_index() {
        // 필터가 목록을 좁히면 가시 인덱스 != 원본 인덱스. Pick 은 문자열을 직접
        // 반환하므로 호출측이 원본 인덱스로 되돌릴 필요가 없다(오매핑 원천 차단).
        let items = ["alpha", "beta", "gamma", "delta"];
        let filtered = filter_entries(&items, "l", MatchMode::Substring);
        // "l" 포함: alpha, delta (beta·gamma 는 제외).
        assert_eq!(filtered, vec!["alpha", "delta"]);
        // keyboard-active = 가시 목록의 1번(delta) — 원본에선 3번.
        let active = 1usize;
        let picked = filtered[active].to_string();
        assert_eq!(picked, "delta");
        // 원본 인덱스 1 은 beta 라, 인덱스 기반이었다면 엉뚱한 후보로 이동했을 것.
        assert_ne!(picked, items[active]);
    }

    #[test]
    fn match_run_case_insensitive_first_run() {
        // 기본 매치 — char 인덱스. "readme.md" 의 "me" 는 4..6.
        assert_eq!(match_run("readme.md", "me"), Some((4, 6)));
        // 대소문자 무시.
        assert_eq!(match_run("README.md", "me"), Some((4, 6)));
        // 첫 매치만.
        assert_eq!(match_run("ababab", "ab"), Some((0, 2)));
        // 없음 / 빈 질의.
        assert_eq!(match_run("readme", "zzz"), None);
        assert_eq!(match_run("readme", ""), None);
        // 질의가 더 길면 없음.
        assert_eq!(match_run("ab", "abc"), None);
    }

    #[test]
    fn match_run_unicode_safe() {
        // 멀티바이트 경계에서도 char 인덱스로 안전.
        assert_eq!(match_run("경로/readme", "readme"), Some((3, 9)));
    }
}
