//! `Combobox` — 편집형 입력 + 히스토리 드롭다운 (디자인 combobox / autocomplete).
//!
//! 새 디자인 언어가 아니라 기존 프리미티브의 **합성**이다:
//! - 트리거(편집 필드) = [`crate::Input`] 그대로(mono 변형·leading 아이콘·focus ring).
//! - 드롭다운 컨테이너 = `menu container` 토큰(surface-raised / border-default /
//!   corner-radius) + `shadow_popover` lift.
//! - 후보 행 = `navigation/MenuItem` 언어(control-height 28, space-md 패딩). 단 경로
//!   가독을 위해 우측 clip 대신 **middle-ellipsis**(파일명 꼬리 보존)로 그린다.
//!
//! 계약(결정 `TD-3/DECISIONS.md`):
//! - 경로 폰트 = mono caption(11) — `mono(true)`. 일반 combobox 는 proportional body(13).
//! - **hover vs keyboard-active 2단계 분리**: pointer hover = `overlay-hover`(약한 워시),
//!   ↑/↓ keyboard 커서 = `surface-active`(더 진함). 겹치면 keyboard-active 우선.
//! - 긴 경로 = middle-ellipsis.
//! - v1 = 필터 없는 히스토리 dropdown(편집 진입 시 최근 목록을 그대로 노출).
//!
//! 키보드 내비(↑/↓/Enter/Esc)는 **스캐폴드**다 — 실제 키 forward(작업1 IME 트랙) 이후
//! plugin 주소창 배선에서 종단 검증한다. 여기선 action 반환 골격과 순수 인덱스 로직만
//! 확정한다(단위테스트로 회귀 격리).

use tasty_type_appearance::theme::Theme;

use crate::icon_button::IconPainter;

/// Combobox 한 프레임의 사용자 행위.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComboboxAction {
    /// 아무 일도 없음(키보드 커서 이동만 포함 — 그건 `active` 갱신으로 반영됨).
    None,
    /// 입력 버퍼 텍스트가 바뀜.
    Edited,
    /// 후보 행을 확정(클릭 또는 keyboard-active 행에서 Enter). 인덱스는 `entries` 기준.
    Pick(usize),
    /// active 행 없이 Enter — 현재 버퍼를 그대로 확정(navigate).
    Submit,
    /// Esc — 드롭다운 닫기 + 버퍼 원복(원복은 호출측 책임).
    Cancel,
}

/// Combobox 빌더. 프레젠테이션 설정만 담고, 상태(`buf`/`entries`/`active`)는 `show` 인자.
pub struct Combobox<'a> {
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
}

impl<'a> Combobox<'a> {
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
        }
    }

    pub fn placeholder(mut self, placeholder: &'a str) -> Self {
        self.placeholder = placeholder;
        self
    }

    /// 후보 0개일 때 드롭다운에 표시할 비상호작용 라벨(예: "No recent files").
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

    /// 트리거 + (포커스 시) 드롭다운을 그린다.
    ///
    /// - `buf`: 편집 버퍼(트리거 텍스트).
    /// - `entries`: 후보(최신순 최대 10). 빈 슬라이스면 empty 행.
    /// - `active`: keyboard-active 행 index(호출측 소유 상태). 첫 오픈 시 `None`.
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
    ) -> ComboboxAction {
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
        let resp = trigger.show(ui, theme, buf);

        let mut action = if resp.changed() {
            ComboboxAction::Edited
        } else {
            ComboboxAction::None
        };

        // 편집(포커스) 중이면 열림 — 브라우저 주소창형(포커스 즉시 히스토리 노출).
        let open = self.enabled && resp.has_focus();
        if !open {
            return action;
        }

        // 키보드 스캐폴드 — 실제 키 forward 이후 종단 검증(작업1 트랙).
        let (down, up, enter, esc) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowDown),
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::Enter),
                i.key_pressed(egui::Key::Escape),
            )
        });
        let n = entries.len();
        if down {
            *active = step_active(*active, n, true);
        }
        if up {
            *active = step_active(*active, n, false);
        }

        // 드롭다운 — 트리거 아래 floating(레이아웃 불변). space-xs 오프셋.
        let area_id = ui.make_persistent_id(("tasty_combobox", self.id_salt));
        let origin = resp.rect.left_bottom() + egui::vec2(0.0, theme.spacing_xs.value());
        let clicked = egui::Area::new(area_id)
            .order(egui::Order::Foreground)
            .fixed_pos(origin)
            .constrain(true)
            .show(ui.ctx(), |ui| {
                ui.set_width(width);
                combobox_dropdown(
                    ui,
                    theme,
                    entries,
                    self.empty_label,
                    self.mono,
                    self.row_icon,
                    *active,
                )
            })
            .inner;

        // 행위 우선순위: Esc > Enter/click > Edited.
        if esc {
            action = ComboboxAction::Cancel;
        } else if enter {
            action = active.map_or(ComboboxAction::Submit, ComboboxAction::Pick);
        } else if let Some(i) = clicked {
            action = ComboboxAction::Pick(i);
        }
        action
    }
}

/// 드롭다운 컨테이너 + 후보 행을 **현재 ui 위치**에 그린다(호출측이 Area/inline 결정).
///
/// 컨테이너 = surface-raised / border-default 1px / corner-radius / `shadow_popover` lift.
/// 행 = MenuItem 언어(control-height, space-md 패딩) + middle-ellipsis 경로.
/// `active`(keyboard 커서)는 surface-active, pointer hover 는 overlay-hover(2단계 분리).
/// 반환: 클릭된 행 index.
pub fn combobox_dropdown(
    ui: &mut egui::Ui,
    theme: &Theme,
    entries: &[&str],
    empty_label: &str,
    mono: bool,
    row_icon: Option<IconPainter<'_>>,
    active: Option<usize>,
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
                for (i, entry) in entries.iter().enumerate() {
                    if candidate_row(ui, theme, entry, mono, row_icon, active == Some(i)).clicked()
                    {
                        clicked = Some(i);
                    }
                }
            }
        });
    clicked
}

/// 후보 행 하나 — MenuItem 구조 전사 + middle-ellipsis. 클릭 응답 반환.
fn candidate_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    path: &str,
    mono: bool,
    row_icon: Option<IconPainter<'_>>,
    keyboard_active: bool,
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
    let g = painter.layout_no_wrap(shown, font, egui::Color32::PLACEHOLDER);
    let pos = egui::pos2(x, rect.center().y - g.rect.height() * 0.5);
    painter.galley(pos, g, fg);

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

/// keyboard-active 인덱스 한 칸 이동(wrap-around). 첫 오픈(`None`)에서 아래=0, 위=마지막.
fn step_active(cur: Option<usize>, n: usize, forward: bool) -> Option<usize> {
    if n == 0 {
        return None;
    }
    Some(match cur {
        None => {
            if forward {
                0
            } else {
                n - 1
            }
        }
        Some(i) => {
            if forward {
                (i + 1) % n
            } else {
                (i + n - 1) % n
            }
        }
    })
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
    fn step_active_wraps_and_starts_from_edge() {
        // 첫 오픈: 아래=최상단, 위=최하단.
        assert_eq!(step_active(None, 3, true), Some(0));
        assert_eq!(step_active(None, 3, false), Some(2));
        // 이동 + wrap-around.
        assert_eq!(step_active(Some(0), 3, true), Some(1));
        assert_eq!(step_active(Some(2), 3, true), Some(0));
        assert_eq!(step_active(Some(0), 3, false), Some(2));
        // 빈 목록은 active 없음.
        assert_eq!(step_active(None, 0, true), None);
        assert_eq!(step_active(Some(0), 0, false), None);
    }

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
}
