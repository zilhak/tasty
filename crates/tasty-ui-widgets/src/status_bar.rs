//! 작업영역(작업 컬럼) 하단 StatusBar 의 **순수 시각** — 디자인
//! `ui_kits/terminal/work.jsx` 의 `StatusBar` 컴포넌트 대응.
//!
//! ## 구성 (확정 — 명세는 `docs/features/workspace-status-bar/index.md`)
//! 이 바는 **포커스 surface 의 읽기 전용 요약 + 키보드 리마인더 하나**다. 어느 항목도
//! 목적지가 아니다(포커스를 옮기는 항목이 없다).
//!
//! - 높이 `theme.status_bar_height`(24), `bg_app` 배경 + 상단 `border_width` separator.
//! - 좌측(순서 고정): **git 브랜치**(글리프 + 이름) · **surface id**(mono `s3·p1`) ·
//!   **shell** · **grid**(`120×32`).
//! - 우측: **팔레트 단축키**(Kbd 키캡) · **테마 글리프**(`sun`/`theme`).
//!   테마 종류는 색이 아니라 글리프가 들고, 이름 텍스트는 달지 않는다.
//! - **값이 없는 항목은 자리째 없다** — dash 를 그리지 않는다. repo 밖이면 브랜치
//!   항목 자체가 없고, detached HEAD 면 그 자리에 short sha 가 온다(호출자가 넣는다).
//!
//! ## 좁아질 때 접는 순서 ([`drop_level`])
//! `1` grid → `2` shell → `3` surface id → `4` 팔레트 키캡 → `5` 브랜치 **텍스트**
//! (브랜치 글리프는 남는다). **테마 글리프는 어느 단계에서도 안 빠진다.**
//! 폭이 floor 보다 좁으면 마지막 단계에서 멈춘다 — 더 접을 것이 없다.
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

use crate::chip::{kbd, kbd_width};

// 디자인 inline 레이아웃 값(`gallery/layouts.jsx` 의 `statusbar` specimen: 컨테이너
// `padding: "0 10px"` + `gap: 10`). 4px 그리드 밖이지만 겨루는 컴포넌트 토큰이 없어
// 이 출처가 곧 근거다. **셀마다 패딩을 주는 형태가 아니다** — 바깥 여백 한 번과
// 항목 사이 gap 이라, 인접한 두 항목 사이는 20 이 아니라 10 이다.
const BAR_PAD_X: LogicalPx = LogicalPx(10.0);
const ITEM_GAP: LogicalPx = LogicalPx(10.0);

/// 브랜치 항목(글리프 + gap + 이름) 전체가 차지할 수 있는 최대 폭. 넘으면 이름을
/// 말줄임한다 — 디자인 성분의 `maxWidth: 160` 그대로다.
///
/// 값은 `size-160` 위에 있지만 **이 자리를 가리키는 컴포넌트 토큰이 없다.** 값이
/// 같은 `Theme` 필드는 `field_width_md`(폼 입력 폭)·`dag_minimap_width` 뿐이라 뜻이
/// 다르고, 같은 수라고 그 이름이 이 자리의 이름은 아니다. 그래서 `on_scale_length_literal`
/// 이 이 한 자리를 계상한다 — 처방은 "토큰으로 바꿔라" 가 아니라 "이 치수에 이름을
/// 줄 것인가" 이고, 그것은 디자인 결정이다.
const BRANCH_MAX_W: LogicalPx = LogicalPx(160.0);

/// 좌측 클러스터의 항목 수 — [`items_at`] 배열의 앞쪽 몇 칸이 좌측인지.
const LEFT_COUNT: usize = 4;

/// 마지막 축소 단계. 이보다 더 접을 것이 없다(테마 글리프는 안 빠진다).
const MAX_DROP_LEVEL: u8 = 5;

/// view 입력 — 한 프레임 분의 StatusBar 표시 데이터.
#[derive(Clone, Debug, Default)]
pub struct StatusBarData {
    /// 브랜치 슬롯의 표시 문자열 — 브랜치명, 또는 detached HEAD 면 short sha.
    /// repo 가 아니면 `None` → **항목 자체가 없다**(dash 를 그리지 않는다).
    pub branch: Option<String>,
    /// focus surface id(숫자). "Copy Terminal ID" 가 복사하는 값과 동일.
    pub surface_id: Option<u32>,
    /// 그 surface 를 담은 pane id. 있으면 `s<sid>·p<pane>`, 없으면 `s<sid>` 로 찍는다.
    pub pane_id: Option<u32>,
    /// 셸/포그라운드 프로세스명(terminal 한정).
    pub shell: Option<String>,
    /// 그리드 크기 (cols, rows) (terminal 한정).
    pub grid: Option<(usize, usize)>,
    /// 현재 테마가 light 인지 — `sun`(light) / `theme`(dark) 글리프를 가른다.
    pub theme_is_light: bool,
    /// 팔레트 단축키(예: `"Cmd+K"`). **키캡으로 그린다.** 바인딩이 없으면 빈 문자열
    /// 이고, 그때는 값이 없는 것이므로 **항목 자체가 없다**.
    pub palette_keys: String,
    /// 팔레트 키캡 hover tooltip.
    pub palette_tooltip: String,
    /// 테마 글리프 hover tooltip.
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
    // 디자인은 두 글꼴을 쓴다 — surface id 와 grid 만 mono(`--tasty-font-mono`)이고
    // 브랜치 이름·셸 이름은 UI 글꼴이다. 크기는 둘 다 caption(11).
    let mono = egui::FontId::monospace(th.font_size_caption.value());
    let text = egui::FontId::proportional(th.font_size_caption.value());
    let muted: egui::Color32 = th.text_muted().into();
    let hover: egui::Color32 = th.text_secondary().into();
    let item_glyph: egui::Color32 = th.statusbar_glyph().into();
    // 테마 표시는 색이 아니라 글리프가 든다 — light 는 `sun`, dark 는 `theme`.
    // 색은 그 자리의 전용 role(`statusbar-theme-glyph` → `glyph-dim`)이라 두 테마에서
    // 같다. 종류를 색으로 알리던 accent_warning()/accent_agent() 갈림은 없어졌다.
    let theme_glyph_tint: egui::Color32 = th.statusbar_theme_glyph().into();
    let theme_glyph = if data.theme_is_light {
        tasty_icons::SUN
    } else {
        tasty_icons::THEME
    };
    let glyph_size = th.icon_glyph_size_xs;
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

    let widths = measure_items(&bar, th, &text, &mono, glyph_size, data);
    let level = drop_level(&widths, width);
    let shown = items_at(&widths, level);

    let left_w = cluster_width(&shown[..LEFT_COUNT]).unwrap_or_default();
    let right_w = cluster_width(&shown[LEFT_COUNT..]).unwrap_or_default();
    let spacer = (width - BAR_PAD_X * 2.0 - left_w - right_w).max(LogicalPx::default());

    bar.add_space(BAR_PAD_X.value());

    // ── 좌측 클러스터 (읽기 전용) ──
    let mut drawn = 0usize;
    if let Some(branch) = &data.branch
        && shown[0].is_some()
    {
        gap_before(&mut bar, &mut drawn);
        // level 5 에서는 글리프만 남는다 — 이름을 접어도 "어느 브랜치인가" 는 사라지지만
        // "repo 안이다" 는 남는다.
        let name = (level < MAX_DROP_LEVEL).then_some(branch.as_str());
        branch_cell(
            &mut bar,
            bar_h,
            &text,
            muted,
            &BranchCellStyle {
                glyph_size,
                gap: th.spacing_xs,
                tint: item_glyph,
            },
            name,
        );
    }
    if shown[1].is_some() {
        gap_before(&mut bar, &mut drawn);
        text_cell(&mut bar, bar_h, &mono, muted, &surface_label(data), None);
    }
    if let Some(shell) = &data.shell
        && shown[2].is_some()
    {
        gap_before(&mut bar, &mut drawn);
        text_cell(&mut bar, bar_h, &text, muted, shell, None);
    }
    if let Some(grid) = data.grid
        && shown[3].is_some()
    {
        gap_before(&mut bar, &mut drawn);
        text_cell(&mut bar, bar_h, &mono, muted, &grid_label(grid), None);
    }

    // flex spacer — 할당된 rect 기준(절대 좌표 비의존).
    bar.add_space(spacer.value());

    // ── 우측 클러스터 ──
    // spacer 가 좌우를 가르므로 우측 첫 항목 앞에는 gap 을 또 넣지 않는다.
    drawn = 0;
    if shown[4].is_some() {
        gap_before(&mut bar, &mut drawn);
        let resp =
            kbd_cell(&mut bar, th, bar_h, &data.palette_keys).on_hover_text(&data.palette_tooltip);
        resize_priority_hovered |= resp.hovered();
        if resp.clicked() {
            actions.push(StatusBarAction::OpenPalette);
        }
    }
    gap_before(&mut bar, &mut drawn);
    let resp = glyph_button_cell(
        &mut bar,
        bar_h,
        theme_glyph,
        glyph_size,
        theme_glyph_tint,
        hover,
    )
    .on_hover_text(&data.theme_tooltip);
    resize_priority_hovered |= resp.hovered();
    if resp.clicked() {
        actions.push(StatusBarAction::ToggleTheme);
    }

    StatusBarDrawResult {
        actions,
        resize_priority_hovered,
    }
}

/// surface id 표기 — pane 을 알면 `s3·p1`, 모르면 `s3`.
fn surface_label(data: &StatusBarData) -> String {
    match (data.surface_id, data.pane_id) {
        (Some(sid), Some(pane)) => format!("s{sid}·p{pane}"),
        (Some(sid), None) => format!("s{sid}"),
        _ => String::new(),
    }
}

/// grid 표기 — 디자인의 `120×32`.
fn grid_label((cols, rows): (usize, usize)) -> String {
    format!("{cols}×{rows}")
}

/// 항목 하나씩의 폭 — **그리기 전에** 알아야 축소 단계를 고를 수 있다.
///
/// 값이 없는 항목은 `None` 이다 — 0 이 아니라 **자리가 없다**는 뜻이고, 그래서 그
/// 항목 앞뒤의 gap 도 들지 않는다. 디자인이 "dash 를 그리지 않는다" 로 말한 것이
/// 여기서는 `Option` 이다.
///
/// `branch` 는 항목 전체(글리프 + gap + 이름, [`BRANCH_MAX_W`] 로 상한),
/// `branch_glyph` 는 이름을 접었을 때 남는 글리프만.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct ItemWidths {
    branch: Option<LogicalPx>,
    branch_glyph: Option<LogicalPx>,
    surface: Option<LogicalPx>,
    shell: Option<LogicalPx>,
    grid: Option<LogicalPx>,
    palette: Option<LogicalPx>,
    theme: LogicalPx,
}

/// 축소 단계 `level` 에서 각 항목이 실제로 차지하는 폭. 순서는 디자인의 배치 순서와
/// 같다 — 브랜치 · surface id · shell · grid · 팔레트 키캡 · 테마 글리프.
fn items_at(w: &ItemWidths, level: u8) -> [Option<LogicalPx>; 6] {
    [
        if level >= 5 { w.branch_glyph } else { w.branch },
        (level < 3).then_some(w.surface).flatten(),
        (level < 2).then_some(w.shell).flatten(),
        (level < 1).then_some(w.grid).flatten(),
        (level < 4).then_some(w.palette).flatten(),
        Some(w.theme),
    ]
}

/// 폭의 합 — `LogicalPx` 는 `Sum` 을 구현하지 않아 접어서 더한다.
fn sum(widths: impl Iterator<Item = LogicalPx>) -> LogicalPx {
    widths.fold(LogicalPx::default(), |acc, w| acc + w)
}

/// 한 클러스터의 폭 — 보이는 항목의 합 + 그 사이 gap. **없는 항목은 gap 도 안 든다.**
fn cluster_width(items: &[Option<LogicalPx>]) -> Option<LogicalPx> {
    let shown = items.iter().flatten().count();
    if shown == 0 {
        return None;
    }
    Some(sum(items.iter().flatten().copied()) + ITEM_GAP * (shown - 1) as f32)
}

/// `level` 에서 바 전체가 요구하는 최소 폭(바깥 여백 + 항목 + 그 사이 gap).
/// 좌우 클러스터 **둘 다** 있으면 그 사이 gap 도 한 번 든다 — 그 자리가 flex spacer
/// 이고, 최소가 gap 이다.
fn total_at(w: &ItemWidths, level: u8) -> LogicalPx {
    let items = items_at(w, level);
    let left = cluster_width(&items[..LEFT_COUNT]);
    let right = cluster_width(&items[LEFT_COUNT..]);
    let between = if left.is_some() && right.is_some() {
        Some(ITEM_GAP)
    } else {
        None
    };
    sum([left, right, between].into_iter().flatten()) + BAR_PAD_X * 2.0
}

/// 주어진 폭에 **들어가는 가장 작은** 축소 단계. 어느 단계도 안 들어가면 마지막 단계
/// 에서 멈춘다 — 더 접을 것이 없고, 그때는 테마 글리프가 잘릴지언정 빠지지는 않는다.
fn drop_level(w: &ItemWidths, available: LogicalPx) -> u8 {
    (0..=MAX_DROP_LEVEL)
        .find(|&l| total_at(w, l) <= available)
        .unwrap_or(MAX_DROP_LEVEL)
}

/// 항목 사이 gap — 첫 항목 앞에는 안 넣는다.
fn gap_before(ui: &mut egui::Ui, drawn: &mut usize) {
    if *drawn > 0 {
        ui.add_space(ITEM_GAP.value());
    }
    *drawn += 1;
}

/// 한 줄 텍스트의 galley. `max_w` 가 있으면 그 폭에서 말줄임(`…`)한다.
fn laid(
    ui: &egui::Ui,
    text: &str,
    font: &egui::FontId,
    max_w: Option<LogicalPx>,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(
        text.to_owned(),
        font.clone(),
        egui::Color32::PLACEHOLDER,
    );
    if let Some(max_w) = max_w {
        job.wrap = egui::text::TextWrapping {
            max_width: max_w.value(),
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        };
    }
    ui.fonts(|f| f.layout_job(job))
}

/// 텍스트만 있는 셀(비클릭). 패딩은 바 바깥 여백이 들고 여기는 글자 폭만 차지한다.
fn text_cell(
    ui: &mut egui::Ui,
    h: LogicalPx,
    font: &egui::FontId,
    color: egui::Color32,
    text: &str,
    max_w: Option<LogicalPx>,
) {
    let galley = laid(ui, text, font, max_w);
    let w = galley.rect.width();
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, h.value()), egui::Sense::hover());
    let pos = egui::pos2(r.left(), r.center().y - galley.rect.height() / 2.0);
    ui.painter().galley(pos, galley, color);
}

/// 브랜치 셀 — 글리프(+ 이름). 이름이 `None` 이면 글리프만(축소 마지막 단계).
fn branch_cell(
    ui: &mut egui::Ui,
    h: LogicalPx,
    font: &egui::FontId,
    color: egui::Color32,
    style: &BranchCellStyle,
    name: Option<&str>,
) {
    let gs = style.glyph_size.value();
    let name_galley = name.map(|n| {
        laid(
            ui,
            n,
            font,
            Some(branch_name_budget(style.glyph_size, style.gap)),
        )
    });
    let gap = if name_galley.is_some() {
        style.gap
    } else {
        LogicalPx::default()
    };
    let text_w = name_galley.as_ref().map_or(0.0, |g| g.rect.width());
    let w = gs + gap.value() + text_w;
    let (r, _) = ui.allocate_exact_size(egui::vec2(w, h.value()), egui::Sense::hover());
    let grect = egui::Rect::from_center_size(
        egui::pos2(r.left() + gs / 2.0, r.center().y),
        egui::Vec2::splat(gs),
    );
    tasty_icons::GIT_BRANCH
        .image(gs, style.tint)
        .paint_at(ui, grect);
    if let Some(galley) = name_galley {
        let pos = egui::pos2(
            r.left() + gs + gap.value(),
            r.center().y - galley.rect.height() / 2.0,
        );
        ui.painter().galley(pos, galley, color);
    }
}

/// [`branch_cell`] 의 치수·색. 디자인 성분의 `gap: 4` 는 `space-xs` 이고, 그 값은
/// `Theme` 에서 온다 — 이 파일에 다시 적지 않는다.
struct BranchCellStyle {
    glyph_size: LogicalPx,
    /// 글리프와 이름 사이 gap(`space-xs`).
    gap: LogicalPx,
    /// 글리프 tint(`statusbar-glyph`) — hover 와 무관하게 고정이다(비클릭 항목).
    tint: egui::Color32,
}

/// 이름에 남는 폭 — 항목 상한에서 글리프와 그 gap 을 뺀 나머지.
fn branch_name_budget(glyph_size: LogicalPx, gap: LogicalPx) -> LogicalPx {
    (BRANCH_MAX_W - glyph_size - gap).max(LogicalPx::default())
}

/// 팔레트 키캡 셀 — [`crate::kbd`] 를 셀 안에 그리고 클릭을 받는다.
fn kbd_cell(ui: &mut egui::Ui, th: &Theme, h: LogicalPx, keys: &str) -> egui::Response {
    let w = kbd_width(ui.ctx(), th, keys);
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w.value(), h.value()), egui::Sense::click());
    let mut inner = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(r)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    kbd(&mut inner, th, keys);
    resp
}

/// 글리프만 있는 버튼 셀(클릭 + hover 밝아짐).
fn glyph_button_cell(
    ui: &mut egui::Ui,
    h: LogicalPx,
    glyph: tasty_icons::Icon,
    glyph_size: LogicalPx,
    tint: egui::Color32,
    hover: egui::Color32,
) -> egui::Response {
    let gs = glyph_size.value();
    let (r, resp) = ui.allocate_exact_size(egui::vec2(gs, h.value()), egui::Sense::click());
    let c = if resp.hovered() { hover } else { tint };
    let grect = egui::Rect::from_center_size(r.center(), egui::Vec2::splat(gs));
    glyph.image(gs, c).paint_at(ui, grect);
    resp
}

/// 한 프레임의 항목 폭을 전부 잰다 — 그리기와 **같은 galley·같은 토큰**으로 센다.
/// 값이 없는 항목은 `None` 이다(0 이 아니다 — [`ItemWidths`] 참조).
fn measure_items(
    ui: &egui::Ui,
    th: &Theme,
    text: &egui::FontId,
    mono: &egui::FontId,
    glyph_size: LogicalPx,
    data: &StatusBarData,
) -> ItemWidths {
    let measure = |s: &str, font: &egui::FontId, max_w: Option<LogicalPx>| {
        LogicalPx(laid(ui, s, font, max_w).rect.width())
    };
    let gap = th.spacing_xs;
    ItemWidths {
        branch: data.branch.as_ref().map(|name| {
            glyph_size + gap + measure(name, text, Some(branch_name_budget(glyph_size, gap)))
        }),
        branch_glyph: data.branch.as_ref().map(|_| glyph_size),
        surface: data
            .surface_id
            .map(|_| measure(&surface_label(data), mono, None)),
        shell: data.shell.as_ref().map(|s| measure(s, text, None)),
        grid: data.grid.map(|g| measure(&grid_label(g), mono, None)),
        palette: (!data.palette_keys.is_empty())
            .then(|| kbd_width(ui.ctx(), th, &data.palette_keys)),
        theme: glyph_size,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 모든 항목이 값을 가진 상태. 폭은 재지 않고 **자리 판정만** 본다 — 단계마다
    /// 어떤 항목이 남는지가 이 함수의 계약이다.
    fn full() -> ItemWidths {
        // 값은 **자리 판정용 픽스처**다 — 실제 토큰 값이 아니라 서로 구별되는 수이고,
        // `size-*` 스케일에 겹치지 않게 고른다(겹치면 `on_scale_length_literal` 의
        // 테스트 사각이 자란다 — 디자인 값을 옮겨 적는 자리가 아닌데도).
        ItemWidths {
            branch: Some(LogicalPx(100.0)),
            branch_glyph: Some(LogicalPx(13.0)),
            surface: Some(LogicalPx(41.0)),
            shell: Some(LogicalPx(31.0)),
            grid: Some(LogicalPx(45.0)),
            palette: Some(LogicalPx(61.0)),
            theme: LogicalPx(13.0),
        }
    }

    #[test]
    fn the_drop_order_is_grid_shell_surface_palette_then_branch_text() {
        let w = full();
        // 단계마다 **직전 단계에서 하나만** 사라진다.
        let seq: Vec<[Option<LogicalPx>; 6]> =
            (0..=MAX_DROP_LEVEL).map(|l| items_at(&w, l)).collect();
        assert_eq!(seq[0][3], w.grid, "level 0 은 아무것도 안 접는다");
        assert_eq!(seq[1][3], None, "1: grid");
        assert_eq!(seq[2][2], None, "2: shell");
        assert_eq!(seq[3][1], None, "3: surface id");
        assert_eq!(seq[4][4], None, "4: palette");
        assert_eq!(seq[5][0], w.branch_glyph, "5: 브랜치는 글리프만 남는다");
    }

    #[test]
    fn the_theme_glyph_never_drops() {
        let w = full();
        for level in 0..=MAX_DROP_LEVEL {
            assert_eq!(
                items_at(&w, level)[5],
                Some(w.theme),
                "level {level} 에서 테마 글리프가 빠졌다"
            );
        }
    }

    #[test]
    fn a_value_less_item_takes_no_slot_and_no_gap() {
        // 브랜치가 없는 상태 — 폭은 브랜치 있는 상태보다 **항목 폭 + gap 만큼** 작다.
        let with = full();
        let without = ItemWidths {
            branch: None,
            branch_glyph: None,
            ..with
        };
        let delta = total_at(&with, 0) - total_at(&without, 0);
        assert_eq!(delta, with.branch.unwrap() + ITEM_GAP);
    }

    #[test]
    fn the_level_is_the_smallest_one_that_fits() {
        let w = full();
        // 딱 맞는 폭에서는 그 단계가 고른다.
        for level in 0..=MAX_DROP_LEVEL {
            let exact = total_at(&w, level);
            assert_eq!(drop_level(&w, exact), level, "level {level} 이 딱 맞는 폭");
        }
    }

    #[test]
    fn below_the_floor_it_stops_at_the_last_level() {
        let w = full();
        let floor = total_at(&w, MAX_DROP_LEVEL);
        assert_eq!(drop_level(&w, floor - LogicalPx(0.5)), MAX_DROP_LEVEL);
        assert_eq!(drop_level(&w, LogicalPx::default()), MAX_DROP_LEVEL);
    }

    #[test]
    fn the_surface_label_names_the_pane_when_it_knows_it() {
        let mut data = StatusBarData {
            surface_id: Some(3),
            pane_id: Some(1),
            ..Default::default()
        };
        assert_eq!(surface_label(&data), "s3·p1");
        data.pane_id = None;
        assert_eq!(surface_label(&data), "s3");
    }
}
