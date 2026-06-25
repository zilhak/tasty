//! Layout — 2depth (Settings window idiom).
//!
//! 디자인 `ui_kits/terminal/overlays/settings_window.jsx` 의
//! *상단 L1 탭 + 좌측 L2 필터 사이드바 + 우측 폼 콘텐츠 + 하단 Save/Cancel* 골격 재현.
//!
//! - 상단(L1): `horizontal_tab_bar_with_arrows` (scroll-arrows 위젯). 디자인
//!   `gallery-alignment §3` 이 settings-only underline fork 를 금지하고 이 공유
//!   위젯 유지로 확정 — underline 은 같은 위젯의 스킨일 뿐.
//! - 좌측(L2): `two_depth_layout_filtered` 의 필터 Input + sub-section 리스트.
//! - 우측: `label-150` 고정컬럼 폼 row — Select / Input / Switch + 색 스와치.
//! - 하단: separator + Save / Cancel.
//!
//! Theme 만 의존. 본체 binary 미의존. 갤러리는 본체 i18n 시스템을 쓰지 않으므로
//! (모든 specimen 이 하드코딩 mock 라벨) settings_window.jsx 의 영문 라벨을 그대로 미러.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{select, switch, Button, ButtonVariant, Input};

// ── 디자인 확정 치수 (settings_window.jsx / design tokens) ──
// Theme 필드에 대응값이 없는 화면전용 고정값은 specimen const 로 두고 디자인
// 토큰명을 주석으로 명시한다 (하드코딩 아님 — 디자인 확정값의 로컬 미러).
/// Row 라벨 고정컬럼 폭 (settings_window.jsx `Row` `width:150`).
const LABEL_COL_WIDTH: f32 = 150.0;
/// 한 행 최소 높이 (`--tasty-settings-row-min-height` = 32).
const ROW_MIN_HEIGHT: f32 = 32.0;
/// form-control 폭 — `--tasty-field-width-xs` = 90.
const FIELD_WIDTH_XS: f32 = 90.0;
/// form-control 폭 — `--tasty-field-width-color` = 110.
const FIELD_WIDTH_COLOR: f32 = 110.0;
/// form-control 폭 — `--tasty-field-width-md` = 160.
const FIELD_WIDTH_MD: f32 = 160.0;
/// form-control 폭 — `--tasty-field-width-lg` = 200.
const FIELD_WIDTH_LG: f32 = 200.0;
/// 색 스와치 한 변 (`--tasty-swatch-size` = 16). radius 는 `corner_radius_sm`(2).
const SWATCH_SIZE: f32 = 16.0;
/// Note 본문 최대 폭 (`--tasty-measure-md` = 400).
const MEASURE_MD: f32 = 400.0;
/// 모달 고정 크기 (settings_window.jsx `width:824 height:472` — 화면전용 verbatim).
const MODAL_W: f32 = 824.0;
const MODAL_H: f32 = 472.0;

struct State {
    top: usize,
    sub: usize,
    /// L2 sub-section 필터 텍스트.
    filter: String,
    font_family: usize,
    color_scheme: usize,
    ligatures: bool,
    theme_defaults: bool,
    /// "Font size:" Input 버퍼.
    font_size: String,
    /// "Accent:" Input 버퍼 (hex). 스와치가 이 값을 파싱해 표시.
    accent: String,
}

impl Default for State {
    fn default() -> Self {
        Self {
            top: 2, // "Appearance" — settings_window.jsx 기본 선택.
            sub: 0, // "Theme".
            filter: String::new(),
            font_family: 0,
            color_scheme: 0,
            ligatures: true,
            theme_defaults: true,
            font_size: "14".to_owned(),
            accent: "#89b4fa".to_owned(),
        }
    }
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

// 디자인 L1 IA (settings_window.jsx `L1_TABS`).
const TOP_TABS: &[&str] = &[
    "General",
    "Terminal",
    "Appearance",
    "Keybindings",
    "File Handler",
    "Misc",
    "Plugins",
];

// 대표 L2 sub-section 리스트 (settings_window.jsx `L2.Appearance`) — 골격 데모용
// 고정 리스트. 본체는 top 탭마다 다른 sub 집합을 갖지만 specimen 은 가장 풍부한
// Appearance sub 집합(스와치 포함)을 대표로 보여준다.
const SUB_TABS: &[&str] = &[
    "Theme", "Colors", "General", "Display", "Tasty", "Terminal", "HTML",
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "디자인 settings_window.jsx 골격 — L1 가로 탭(scroll-arrows) + L2 필터 \
             사이드바 + label-150 폼 row + 하단 Save/Cancel.",
        )
        .color(egui::Color32::from(theme.subtext0))
        .size(theme.font_size_caption.value()),
    );
    ui.add_space(theme.spacing_sm.value());

    egui::Frame::group(ui.style()).show(ui, |ui| {
        // 모달 고정 크기 (824×472) — 화면전용 디자인 값 verbatim.
        ui.set_width(MODAL_W);
        ui.set_min_height(MODAL_H);
        ui.set_max_height(MODAL_H);

        let avail_h = ui.available_height();
        // 하단 Save/Cancel 영역: 버튼 높이 + 상하 패딩.
        let bottom_h = theme.item_height_interactive.value() + theme.spacing_md.value() * 2.0;
        let content_h = (avail_h - bottom_h).max(0.0);

        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), content_h),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                draw_top_tabs(ui, theme);
                ui.separator();
                ui.add_space(theme.spacing_xs.value());
                draw_split(ui, theme);
            },
        );

        ui.separator();
        draw_bottom_buttons(ui, theme);
    });
}

fn draw_top_tabs(ui: &mut egui::Ui, _theme: &Theme) {
    let cur_top = STATE.with(|s| s.borrow().top);
    let mut new_top = cur_top;
    let tabs: Vec<(usize, &str)> = TOP_TABS.iter().copied().enumerate().collect();
    ui.horizontal(|ui| {
        tasty_ui_widgets::horizontal_tab_bar_with_arrows(
            ui,
            "layout_2depth_top_scroll",
            &tabs,
            &mut new_top,
        );
    });
    if new_top != cur_top {
        STATE.with(|s| {
            let mut st = s.borrow_mut();
            st.top = new_top;
            st.sub = 0;
        });
    }
}

/// 좌측 필터 + sub-section 리스트 (Frame) + 우측 콘텐츠.
/// `two_depth_layout_filtered` 호출 — 좌측 상단 필터 Input 슬롯 포함.
fn draw_split(ui: &mut egui::Ui, theme: &Theme) {
    let available_height = ui.available_height();
    STATE.with(|s| {
        let mut st = s.borrow_mut();
        // 필터는 위젯이 `&mut String` 으로 그리고, 항목 필터링은 left 클로저가
        // 수행한다. content 가 `&mut st` 를 빌리므로 필터 버퍼는 잠시 빼내어
        // 별도 로컬로 다루고, 항목 매칭용 값은 미리 스냅샷한다 (1 프레임 지연은
        // 시각적으로 무해).
        let mut filter = std::mem::take(&mut st.filter);
        let filter_snapshot = filter.to_lowercase();
        let cur_sub = st.sub;
        let mut new_sub = cur_sub;

        tasty_ui_widgets::two_depth_layout_filtered(
            ui,
            theme,
            available_height,
            &mut filter,
            "Filter sections…",
            |ui| {
                for (idx, label) in SUB_TABS.iter().enumerate() {
                    if !filter_snapshot.is_empty()
                        && !label.to_lowercase().contains(&filter_snapshot)
                    {
                        continue;
                    }
                    if ui.selectable_label(cur_sub == idx, *label).clicked() {
                        new_sub = idx;
                    }
                }
            },
            |ui| draw_content(ui, theme, &mut st),
        );

        st.filter = filter;
        st.sub = new_sub;
    });
}

fn draw_content(ui: &mut egui::Ui, theme: &Theme, st: &mut State) {
    let top = TOP_TABS.get(st.top).copied().unwrap_or("?");
    let sub = SUB_TABS.get(st.sub).copied().unwrap_or("?");
    egui::ScrollArea::vertical()
        .id_salt("layout_2depth_content_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            tasty_ui_widgets::tab_content_frame(ui, |ui| {
                section_heading(ui, theme, &format!("{top} · {sub}"));
                ui.add_space(theme.spacing_sm.value());

                // field-width-lg(200) — Select.
                form_row(ui, theme, "Font family:", |ui| {
                    let opts = ["D2Coding", "JetBrains Mono", "Cascadia Code"];
                    select(
                        ui,
                        theme,
                        "settings_font_family",
                        &mut st.font_family,
                        &opts,
                        FIELD_WIDTH_LG,
                        true,
                    );
                });
                // field-width-xs(90) — Input + 단위 addon.
                form_row(ui, theme, "Font size:", |ui| {
                    Input::new()
                        .mono(true)
                        .addon("px")
                        .width(FIELD_WIDTH_XS)
                        .show(ui, theme, &mut st.font_size);
                });
                // field-width-md(160) — Select.
                form_row(ui, theme, "Color scheme:", |ui| {
                    let opts = ["Follow theme", "Light", "Dark"];
                    select(
                        ui,
                        theme,
                        "settings_color_scheme",
                        &mut st.color_scheme,
                        &opts,
                        FIELD_WIDTH_MD,
                        true,
                    );
                });
                form_row(ui, theme, "Ligatures:", |ui| {
                    switch(ui, theme, &mut st.ligatures, None, true);
                });

                ui.add_space(theme.spacing_md.value());
                section_heading(ui, theme, "Tasty chrome");
                ui.add_space(theme.spacing_sm.value());

                // field-width-color(110) — Input + 16px 스와치(radius 2).
                form_row(ui, theme, "Accent:", |ui| {
                    Input::new()
                        .mono(true)
                        .width(FIELD_WIDTH_COLOR)
                        .show(ui, theme, &mut st.accent);
                    // Input↔swatch gap 은 row 의 item_spacing.x(=space-lg 16,
                    // form_row 에서 설정) 가 제공한다 — 디자인 Row 는 모든 flex
                    // 자식 사이가 gap:16 이라 add_space 불필요.
                    // 스와치 색은 Theme 에서 (기본 accent = #89b4fa = accent_primary).
                    swatch(ui, theme, theme.accent_primary().to_egui());
                });
                form_row(ui, theme, "Use theme defaults:", |ui| {
                    switch(ui, theme, &mut st.theme_defaults, None, true);
                });

                ui.add_space(theme.spacing_sm.value());
                note(
                    ui,
                    theme,
                    "Override the Tasty app chrome (sidebar, tabs, title bar) independently of \
                     the terminal surface. Turn on \u{201c}Use theme defaults\u{201d} to follow \
                     the selected preset.",
                );
            });
        });
}

/// Mono 섹션 헤딩 (settings_window.jsx `Mono`: mono · `font-size-micro`(10) ·
/// uppercase · text-muted). egui 는 letter-spacing 미지원이라 생략.
fn section_heading(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .font(egui::FontId::monospace(theme.font_size_micro.value()))
            .color(theme.text_muted().to_egui()),
    );
}

/// label-150 고정컬럼 + 우측 컨트롤 row (settings_window.jsx `Row`).
/// gap 16(`space-lg`), min-height 32(`--tasty-settings-row-min-height`).
fn form_row(ui: &mut egui::Ui, theme: &Theme, label: &str, control: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.set_min_height(ROW_MIN_HEIGHT);
        ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
        let (rect, _) = ui.allocate_exact_size(
            egui::vec2(LABEL_COL_WIDTH, ROW_MIN_HEIGHT),
            egui::Sense::hover(),
        );
        ui.painter().text(
            egui::pos2(rect.min.x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(theme.font_size_body.value()),
            theme.text_secondary().to_egui(),
        );
        control(ui);
    });
}

/// 16px 색 스와치 — radius `corner_radius_sm`(2) + `border_strong` 1px 보더.
fn swatch(ui: &mut egui::Ui, theme: &Theme, color: egui::Color32) {
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(SWATCH_SIZE, SWATCH_SIZE), egui::Sense::hover());
    let radius = theme.corner_radius_sm.value();
    ui.painter().rect_filled(rect, radius, color);
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
        egui::StrokeKind::Inside,
    );
}

/// 보조 설명 문단 (settings_window.jsx `Note`: `measure-md`(400) 폭 · text-muted).
fn note(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.scope(|ui| {
        ui.set_max_width(MEASURE_MD);
        ui.label(
            egui::RichText::new(text)
                .size(theme.font_size_caption.value())
                .color(theme.text_muted().to_egui()),
        );
    });
}

fn draw_bottom_buttons(ui: &mut egui::Ui, theme: &Theme) {
    ui.add_space(theme.spacing_sm.value());
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        // footer gap:8 (space-sm). egui 기본 item_spacing 세금 제거 후 명시.
        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
        // mock: click 결과 무시 — 갤러리 시각 검증 전용.
        let _save = Button::new("Save")
            .variant(ButtonVariant::Primary)
            .show(ui, theme);
        let _cancel = Button::new("Cancel")
            .variant(ButtonVariant::Ghost)
            .show(ui, theme);
    });
}
