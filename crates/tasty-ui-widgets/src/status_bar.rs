//! 작업영역(작업 컬럼) 하단 StatusBar 의 **순수 시각** — 디자인
//! `ui_kits/terminal/work.jsx` 의 `StatusBar` 컴포넌트 대응.
//!
//! ## 구성 (디자인 canonical)
//! - 높이 `theme.status_bar_height`(24), `bg_app` 배경 + 상단 `border_width` separator.
//! - 좌측: 브랜치 점(`accent_success`)+이름 / surfaceId / `<shell> · <cols>×<rows>`.
//! - 우측(clickable): `<단축키> palette` 칩(팔레트 오픈) + 테마 토글(점 + 테마명).
//!
//! ## 이 crate 가 소유하지 않는 것
//! - **`egui::Area` / `LayerId`** — 부유 배치와 z-order 는 본체 정책이라 호출자가
//!   소유한다. 이 view 는 넘겨받은 [`egui::Ui`] 안에 크기를 할당하고 **그 rect 기준**
//!   으로만 그린다(절대 화면 좌표 비의존) — 그래서 갤러리 specimen 처럼 화면 원점이
//!   아닌 카드 안에 놓여도 좌표가 어긋나지 않는다.
//! - **i18n** — 이 crate 는 `tasty-i18n` 을 의존하지 않는다(`multi_select` 와 동일
//!   정책). 라벨·tooltip 문자열은 [`StatusBarData`] 필드로 호출자가 주입한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

// 디자인 inline 레이아웃 값(work.jsx `StatusBar`: cell padding 0 10px, gap 6,
// dot 7×7). bar view 의 로컬 레이아웃 상수로 둔다.
const CELL_PAD_X: LogicalPx = LogicalPx(10.0);
const CELL_GAP: LogicalPx = LogicalPx(6.0);
const DOT_SIZE: LogicalPx = LogicalPx(7.0);

/// view 입력 — 한 프레임 분의 StatusBar 표시 데이터.
#[derive(Clone, Debug, Default)]
pub struct StatusBarData {
    /// git 브랜치명(focus surface 의 cwd 기준). repo 가 아니면 `None` → 미표시.
    pub branch: Option<String>,
    /// focus surface id(숫자). "Copy Terminal ID" 가 복사하는 값과 동일.
    pub surface_id: Option<u32>,
    /// 셸/포그라운드 프로세스명(terminal 한정).
    pub shell: Option<String>,
    /// 그리드 크기 (cols, rows) (terminal 한정).
    pub grid: Option<(usize, usize)>,
    /// 현재 테마명(capitalize 표시용 원본 id).
    pub theme_id: String,
    /// 현재 테마가 light 인지(테마 토글 점 색 결정: light=yellow, dark=mauve).
    pub theme_is_light: bool,
    /// 팔레트 칩 라벨 전체(예: `"Cmd+K palette"`). 단축키 미설정이면 단어만.
    /// 그리기와 우측 클러스터 폭 계산이 **같은 이 필드**를 읽는다 — 두 곳에서
    /// 독립 조립하면 폭이 어긋난다.
    pub palette_label: String,
    /// 팔레트 칩 hover tooltip.
    pub palette_tooltip: String,
    /// 테마 토글 hover tooltip.
    pub theme_tooltip: String,
}

/// view 가 보고하는 사용자 클릭 액션.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusBarAction {
    /// 팔레트 칩 클릭 → 커맨드 팔레트 토글.
    OpenPalette,
    /// 테마 토글 클릭 → latte ↔ mocha 전환.
    ToggleTheme,
}

/// [`draw_status_bar_view`] 의 반환값 — 클릭 액션 + 가장자리 리사이즈 우선권 판정.
#[derive(Clone, Debug, Default)]
pub struct StatusBarDrawResult {
    pub actions: Vec<StatusBarAction>,
    /// 마우스가 상태바의 실제 클릭 가능 요소(팔레트 칩·테마 토글) 위인지.
    /// 본체에서 `AppState.resize_edge_widget_hovered` 에 OR 로 합성된다.
    pub resize_priority_hovered: bool,
}

/// 순수 시각 — `Theme` + [`StatusBarData`] 로 하단 바(`width` × `status_bar_height`)를
/// 그리고, 사용자 클릭을 [`StatusBarAction`] 으로 수집해 반환한다.
///
/// 전달된 `ui` 에서 크기를 할당하고 **그 반환 `Rect` 기준**으로 separator 위치와
/// spacer 폭을 계산한다 — 화면 절대 좌표에 의존하지 않으므로 본체(Area 안)와
/// 갤러리(카드 안) 어디에 놓여도 같은 결과가 나온다.
pub fn draw_status_bar_view(
    ui: &mut egui::Ui,
    th: &Theme,
    width: LogicalPx,
    data: &StatusBarData,
) -> StatusBarDrawResult {
    let mut actions = Vec::new();
    let mut resize_priority_hovered = false;
    let font = egui::FontId::monospace(th.font_size_caption.value());
    let muted: egui::Color32 = th.text_muted().into();
    let hover: egui::Color32 = th.text_secondary().into();
    let success: egui::Color32 = th.accent_success().into();
    // divergence: light/dark 테마 표시 도트. warning/agent role 이 아니라 테마 종류 표시용이나
    // 전용 토큰이 없어 값-보존 위해 accent_warning()/accent_agent() 사용(픽셀 동일).
    let theme_dot: egui::Color32 = if data.theme_is_light {
        th.accent_warning().into()
    } else {
        th.accent_agent().into()
    };
    let bg: egui::Color32 = th.bg_app().into();
    let bar_h = th.status_bar_height;

    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(width.value(), bar_h.value()),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 0.0, bg);
    // 상단 1px separator (디자인: borderTop 1px separator).
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(th.border_width.value(), th.separator),
    );

    let mut bar = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    bar.spacing_mut().item_spacing.x = 0.0;

    // ── 좌측 클러스터 ──
    // 브랜치 점 + 이름 (repo 일 때만).
    if let Some(branch) = &data.branch {
        dot_text_cell(&mut bar, bar_h, &font, success, success, branch, DOT_SIZE);
    }
    // surfaceId.
    if let Some(sid) = data.surface_id {
        text_cell(&mut bar, bar_h, &font, muted, &sid.to_string());
    }
    // shell · cols×rows.
    if let (Some(shell), Some((cols, rows))) = (&data.shell, data.grid) {
        text_cell(
            &mut bar,
            bar_h,
            &font,
            muted,
            &format!("{shell} · {cols}×{rows}"),
        );
    }

    // flex spacer — 할당된 rect 기준(절대 좌표 비의존).
    let used = bar.min_rect().width();
    let right_w = right_cluster_width(&bar, &font, data).value();
    let spacer = (rect.width() - used - right_w).max(0.0);
    bar.add_space(spacer);

    // ── 우측 클러스터 (clickable) ──
    // 팔레트 칩: "<Cmd+K> palette".
    let palette_resp = button_cell(&mut bar, bar_h, &font, muted, hover, &data.palette_label)
        .on_hover_text(&data.palette_tooltip);
    resize_priority_hovered |= palette_resp.hovered();
    if palette_resp.clicked() {
        actions.push(StatusBarAction::OpenPalette);
    }
    // 테마 토글: 점 + 테마명(capitalize).
    let theme_style = DotCellStyle {
        color: muted,
        hover,
        dot: theme_dot,
    };
    let theme_resp = dot_button_cell(
        &mut bar,
        bar_h,
        &font,
        &theme_style,
        &capitalize(&data.theme_id),
        DOT_SIZE,
    )
    .on_hover_text(&data.theme_tooltip);
    resize_priority_hovered |= theme_resp.hovered();
    if theme_resp.clicked() {
        actions.push(StatusBarAction::ToggleTheme);
    }

    StatusBarDrawResult {
        actions,
        resize_priority_hovered,
    }
}

/// 텍스트 너비 측정.
fn measure(ui: &egui::Ui, text: &str, font: &egui::FontId, color: egui::Color32) -> LogicalPx {
    LogicalPx(ui.fonts(|f| {
        f.layout_no_wrap(text.to_owned(), font.clone(), color)
            .size()
            .x
    }))
}

/// 우측 클러스터(팔레트 칩 + 테마 토글)의 총 너비를 미리 계산(spacer 산정용).
fn right_cluster_width(ui: &egui::Ui, font: &egui::FontId, data: &StatusBarData) -> LogicalPx {
    let muted = egui::Color32::PLACEHOLDER;
    let palette_w =
        measure(ui, &data.palette_label, font, muted).value() + CELL_PAD_X.value() * 2.0;
    let theme_w = DOT_SIZE.value()
        + CELL_GAP.value()
        + measure(ui, &capitalize(&data.theme_id), font, muted).value()
        + CELL_PAD_X.value() * 2.0;
    LogicalPx(palette_w + theme_w)
}

/// 텍스트만 있는 셀(비클릭).
fn text_cell(
    ui: &mut egui::Ui,
    h: LogicalPx,
    font: &egui::FontId,
    color: egui::Color32,
    text: &str,
) {
    let w = measure(ui, text, font, color).value() + CELL_PAD_X.value() * 2.0;
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, h.value()), egui::Sense::hover());
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        color,
    );
}

/// 점 + 텍스트 셀(비클릭).
fn dot_text_cell(
    ui: &mut egui::Ui,
    h: LogicalPx,
    font: &egui::FontId,
    dot: egui::Color32,
    text_color: egui::Color32,
    text: &str,
    dot_size: LogicalPx,
) {
    let dot_size = dot_size.value();
    let w = dot_size
        + CELL_GAP.value()
        + measure(ui, text, font, text_color).value()
        + CELL_PAD_X.value() * 2.0;
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, h.value()), egui::Sense::hover());
    let dot_center = egui::pos2(r.left() + CELL_PAD_X.value() + dot_size / 2.0, r.center().y);
    ui.painter().circle_filled(dot_center, dot_size / 2.0, dot);
    ui.painter().text(
        egui::pos2(
            r.left() + CELL_PAD_X.value() + dot_size + CELL_GAP.value(),
            r.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        text,
        font.clone(),
        text_color,
    );
}

/// 텍스트 버튼 셀(클릭 + hover 색 전환).
fn button_cell(
    ui: &mut egui::Ui,
    h: LogicalPx,
    font: &egui::FontId,
    color: egui::Color32,
    hover: egui::Color32,
    text: &str,
) -> egui::Response {
    let w = measure(ui, text, font, color).value() + CELL_PAD_X.value() * 2.0;
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, h.value()), egui::Sense::click());
    let c = if resp.hovered() { hover } else { color };
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        text,
        font.clone(),
        c,
    );
    resp
}

/// [`dot_button_cell`] 의 색 3종(점/텍스트/hover). Theme 파생값으로 호출부에서 채운다.
struct DotCellStyle {
    color: egui::Color32,
    hover: egui::Color32,
    dot: egui::Color32,
}

/// 점 + 텍스트 버튼 셀(클릭 + hover 색 전환). 점 색은 hover 와 무관하게 고정.
fn dot_button_cell(
    ui: &mut egui::Ui,
    h: LogicalPx,
    font: &egui::FontId,
    style: &DotCellStyle,
    text: &str,
    dot_size: LogicalPx,
) -> egui::Response {
    let dot_size = dot_size.value();
    let w = dot_size
        + CELL_GAP.value()
        + measure(ui, text, font, style.color).value()
        + CELL_PAD_X.value() * 2.0;
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, h.value()), egui::Sense::click());
    let c = if resp.hovered() {
        style.hover
    } else {
        style.color
    };
    let dot_center = egui::pos2(r.left() + CELL_PAD_X.value() + dot_size / 2.0, r.center().y);
    ui.painter()
        .circle_filled(dot_center, dot_size / 2.0, style.dot);
    ui.painter().text(
        egui::pos2(
            r.left() + CELL_PAD_X.value() + dot_size + CELL_GAP.value(),
            r.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        text,
        font.clone(),
        c,
    );
    resp
}

/// 첫 글자 대문자화(디자인: 테마명 capitalize).
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => format!("{}{}", first.to_uppercase(), chars.as_str()),
        None => String::new(),
    }
}
