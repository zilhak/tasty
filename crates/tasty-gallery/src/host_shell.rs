//! 갤러리 egui 셸 — 디자인(4) gallery 의 4영역 문서 셸.
//!
//! 2×2 그리드: 좌상 brand(232×52) / 우상 top(crumb + 세그 토글) / 좌 nav(232) /
//! 우 main(활성 페이지 전체를 스크롤하는 문서 본문).
//!
//! nav 의 Catalog 그룹은 5 페이지 링크, "On this page" 그룹은 활성 페이지의
//! Section 앵커. main 은 활성 페이지의 모든 Section/Spec 을 `spec` 헬퍼로 렌더한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::catalog::{self, Category, Page};

/// brand/nav 폭 (research §1.1 grid-template-columns 232px).
const NAV_WIDTH: LogicalPx = LogicalPx(232.0);
/// brand/top 높이 (research §1.1 grid-template-rows 52px).
const HEADER_HEIGHT: LogicalPx = LogicalPx(52.0);
/// 본문 가독 컬럼 상한 (research §1.2 `.g-page` max-width 1080px).
const PAGE_MAX_WIDTH: LogicalPx = LogicalPx(1080.0);

/// 갤러리에 노출하는 빌트인 테마 식별자.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeId {
    Mocha,
    Latte,
}

impl ThemeId {
    /// 해당 id 의 *기본 색상 세트* 를 만들고 base mode 를 적용해 Theme 으로 반환.
    fn build(self, is_light: bool) -> Theme {
        let mut t = match self {
            ThemeId::Mocha => tasty_themes::mocha_fallback(),
            ThemeId::Latte => latte_theme(),
        };
        t.set_is_light(is_light);
        t
    }
}

/// 빌트인 `LATTE_TOML_TEXT` 를 Mocha base 위에 partial 로 적용해 Theme 생성.
fn latte_theme() -> Theme {
    use tasty_type_appearance::theme::Theme as ThemeStruct;
    let file = tasty_themes::ThemeFile::parse(tasty_themes::LATTE_TOML_TEXT)
        .expect("builtin latte.toml ships valid");
    let (partial, is_light) = file.to_partial();
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.apply_partial(&partial);
    ThemeStruct::with_colors(colors, is_light.unwrap_or(true))
}

/// UI scale 세그 stops (Appearance›Display 매핑과 동일).
const UI_SCALE_STOPS: [(&str, f32); 3] = [("0.8", 0.8), ("1.0", 1.0), ("1.2", 1.2)];

/// 갤러리 전역 상태.
pub struct GalleryState {
    /// 현재 적용 중인 테마. 본체 글로벌과 격리된 갤러리 전용 인스턴스.
    pub theme: Theme,
    /// 선택된 테마 식별자 (Theme 구조체엔 id 가 없어 별도 보관).
    pub theme_id: ThemeId,
    /// 문서 페이지 트리 (Foundations/Components/Icons/Overlays/Layouts).
    pub pages: Vec<Page>,
    /// 현재 활성 페이지 index (`pages` 기준).
    pub active_page: usize,
    /// 사이드바 zoom 배율 (UI scale 세그).
    pub ui_scale: f32,
    /// SPECS 토글 상태 (4px grid 오버레이 — 본 단계는 상태만, 오버레이는 후순위).
    pub specs_on: bool,
    /// 다음 frame 에서 visuals/zoom 을 ctx 에 재적용할지.
    pub needs_reapply: bool,
}

impl GalleryState {
    pub fn new() -> Self {
        let theme_id = ThemeId::Mocha;
        Self {
            theme: theme_id.build(false),
            theme_id,
            pages: catalog::pages(),
            active_page: 0,
            ui_scale: 1.0,
            specs_on: false,
            needs_reapply: true,
        }
    }

    /// 페이지 index 를 범위 내로 고정해 활성화 (스크린샷 배치 등 외부 진입점용).
    pub fn select_page(&mut self, idx: usize) {
        self.active_page = idx.min(self.pages.len().saturating_sub(1));
    }
}

impl Default for GalleryState {
    fn default() -> Self {
        Self::new()
    }
}

/// 한 frame draw.
pub fn draw(ctx: &egui::Context, state: &mut GalleryState) {
    if state.needs_reapply {
        tasty_egui_theme::apply_theme_to_egui(&state.theme, ctx);
        ctx.set_zoom_factor(state.ui_scale);
        state.needs_reapply = false;
    }

    let sidebar_bg = egui::Color32::from(state.theme.bg_sidebar());
    let main_bg = egui::Color32::from(state.theme.bg_app());

    // ── 상단: brand(232) + top bar ──
    egui::TopBottomPanel::top("g_header")
        .exact_height(HEADER_HEIGHT.value())
        .frame(egui::Frame::new().fill(sidebar_bg))
        .show(ctx, |ui| header_ui(ui, state));

    // ── 좌측: nav ──
    egui::SidePanel::left("g_nav")
        .exact_width(NAV_WIDTH.value())
        .resizable(false)
        .frame(egui::Frame::new().fill(sidebar_bg).inner_margin(egui::Margin {
            left: 10,
            right: 10,
            top: 12,
            bottom: 24,
        }))
        .show(ctx, |ui| nav_ui(ui, state));

    // ── 우측: main 문서 본문 ──
    egui::CentralPanel::default()
        .frame(egui::Frame::new().fill(main_bg).inner_margin(0))
        .show(ctx, |ui| main_ui(ui, state));
}

/// 상단 헤더 — brand 블록(좌 232) + top bar(crumb + Specs/Theme/Scale 세그).
fn header_ui(ui: &mut egui::Ui, state: &mut GalleryState) {
    // Theme 의존 색/치수를 Copy 로 스냅샷 (이후 state 변이와 borrow 충돌 방지).
    let (primary, muted, accent, melon, border, surf_raised, surf_active, separator) = {
        let t = &state.theme;
        (
            egui::Color32::from(t.text_primary()),
            egui::Color32::from(t.text_muted()),
            egui::Color32::from(t.accent_primary()),
            egui::Color32::from(t.brand_melon_flesh()),
            egui::Color32::from(t.border_default()),
            egui::Color32::from(t.surface_raised()),
            egui::Color32::from(t.surface_active()),
            egui::Color32::from(t.separator),
        )
    };
    let seg = SegStyle {
        border,
        surf_raised,
        surf_active,
        primary,
        muted,
        radius: state.theme.corner_radius.value(),
        border_w: state.theme.border_width.value(),
        height: state.theme.item_height_interactive.value(),
        font: state.theme.font_size_term_sm.value(),
    };
    let logo = state.theme.sidebar_logo_size.value();
    let f_word = state.theme.sidebar_wordmark_font_size.value();
    let f_micro = state.theme.font_size_micro.value();
    let f_body = state.theme.font_size_body.value();
    let radius_sm = state.theme.corner_radius_sm.value();
    let pad_lg = state.theme.spacing_lg.value();
    let pad_sm = state.theme.spacing_sm.value();

    let page_label = state
        .pages
        .get(state.active_page)
        .map(|p| p.category.label())
        .unwrap_or("");
    let theme_id = state.theme_id;
    let ui_scale = state.ui_scale;
    let specs_on = state.specs_on;

    // 수집한 액션 — 렌더 후 state 에 반영 (closure 내 state 변이 회피).
    let mut act_theme: Option<ThemeId> = None;
    let mut act_specs: Option<bool> = None;
    let mut act_scale: Option<f32> = None;

    ui.horizontal_centered(|ui| {
        // brand 블록 (좌 232).
        ui.allocate_ui_with_layout(
            egui::vec2(NAV_WIDTH.value(), HEADER_HEIGHT.value()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add_space(pad_lg);
                let (r, _) =
                    ui.allocate_exact_size(egui::vec2(logo, logo), egui::Sense::hover());
                ui.painter().rect_filled(r, radius_sm, accent);
                ui.add_space(pad_sm);
                ui.spacing_mut().item_spacing.x = 0.0;
                ui.label(
                    egui::RichText::new("tasty")
                        .size(f_word)
                        .strong()
                        .color(primary),
                );
                ui.label(egui::RichText::new(".").size(f_word).strong().color(melon));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(pad_lg);
                    ui.label(egui::RichText::new("gallery").size(f_micro).color(muted));
                });
            },
        );

        // top bar (나머지 폭).
        ui.add_space(pad_lg);
        ui.label(
            egui::RichText::new(page_label)
                .size(f_body)
                .strong()
                .color(primary),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(pad_lg);
            // Theme 세그 (Mocha / Latte) — 우측 끝.
            if let Some(i) = seg_with_label(
                ui,
                &seg,
                f_micro,
                "Theme",
                &[
                    ("Mocha", theme_id == ThemeId::Mocha),
                    ("Latte", theme_id == ThemeId::Latte),
                ],
            ) {
                act_theme = Some(if i == 0 { ThemeId::Mocha } else { ThemeId::Latte });
            }
            ui.add_space(pad_lg);
            // UI scale 세그.
            let scale_items: Vec<(&str, bool)> = UI_SCALE_STOPS
                .iter()
                .map(|(l, s)| (*l, (ui_scale - s).abs() < 0.001))
                .collect();
            if let Some(i) = seg_with_label(ui, &seg, f_micro, "Scale", &scale_items) {
                act_scale = Some(UI_SCALE_STOPS[i].1);
            }
            ui.add_space(pad_lg);
            // Specs 세그 (Off / On).
            if let Some(i) = seg_with_label(
                ui,
                &seg,
                f_micro,
                "Specs",
                &[("Off", !specs_on), ("On", specs_on)],
            ) {
                act_specs = Some(i == 1);
            }
        });
    });

    // 헤더 하단 separator + brand 우측 separator.
    let rect = ui.max_rect();
    let stroke = egui::Stroke::new(seg.border_w, separator);
    ui.painter().hline(rect.x_range(), rect.bottom(), stroke);
    ui.painter()
        .vline(rect.left() + NAV_WIDTH.value(), rect.y_range(), stroke);

    // 액션 반영.
    if let Some(id) = act_theme
        && id != state.theme_id
    {
        state.theme_id = id;
        state.theme = id.build(state.theme.is_light);
        state.needs_reapply = true;
    }
    if let Some(on) = act_specs {
        state.specs_on = on;
    }
    if let Some(s) = act_scale
        && (state.ui_scale - s).abs() >= 0.001
    {
        state.ui_scale = s;
        state.needs_reapply = true;
    }
}

/// 좌측 nav — Catalog(5 페이지 링크) + On this page(활성 페이지 Section 앵커).
fn nav_ui(ui: &mut egui::Ui, state: &mut GalleryState) {
    let (primary, muted, secondary, surf_active, separator) = {
        let t = &state.theme;
        (
            egui::Color32::from(t.text_primary()),
            egui::Color32::from(t.text_muted()),
            egui::Color32::from(t.text_secondary()),
            egui::Color32::from(t.surface_active()),
            egui::Color32::from(t.separator),
        )
    };
    let f_heading = state.theme.font_size_micro.value();
    let f_lbl = state.theme.font_size_body.value();
    let f_desc = state.theme.font_size_micro.value();
    let radius_sm = state.theme.corner_radius_sm.value();
    let pad_sm = state.theme.spacing_sm.value();
    let pad_lg = state.theme.spacing_lg.value();
    let row_h = state.theme.item_height_interactive.value();

    let active = state.active_page;
    let pages: Vec<(&'static str, &'static str)> = state
        .pages
        .iter()
        .map(|p| (p.category.label(), p.category.desc()))
        .collect();
    let sections: Vec<&'static str> = state
        .pages
        .get(active)
        .map(|p| p.sections.iter().map(|s| s.title).collect())
        .unwrap_or_default();

    let mut act_page: Option<usize> = None;

    egui::ScrollArea::vertical()
        .id_salt("g_nav_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            nav_heading(ui, "Catalog", f_heading, muted);
            for (i, (lbl, desc)) in pages.iter().enumerate() {
                if nav_link(
                    ui,
                    NavLinkStyle {
                        primary,
                        muted,
                        surf_active,
                        radius: radius_sm,
                        font_lbl: f_lbl,
                        font_desc: f_desc,
                        height: row_h,
                    },
                    lbl,
                    desc,
                    i == active,
                ) {
                    act_page = Some(i);
                }
            }

            ui.add_space(pad_lg);
            nav_heading(ui, "On this page", f_heading, muted);
            for title in &sections {
                ui.horizontal(|ui| {
                    ui.add_space(pad_lg);
                    // border-left separator.
                    let (r, _) = ui.allocate_exact_size(
                        egui::vec2(1.0, row_h * 0.8),
                        egui::Sense::hover(),
                    );
                    ui.painter().vline(
                        r.center().x,
                        r.y_range(),
                        egui::Stroke::new(1.0, separator),
                    );
                    ui.add_space(pad_sm);
                    ui.label(
                        egui::RichText::new(*title)
                            .size(f_lbl)
                            .color(secondary),
                    );
                });
            }
        });

    if let Some(i) = act_page
        && i != state.active_page
    {
        state.active_page = i;
    }
}

/// 우측 main — 활성 페이지 문서. pagehead + Section/Spec 트리.
fn main_ui(ui: &mut egui::Ui, state: &GalleryState) {
    let theme = &state.theme;
    let Some(page) = state.pages.get(state.active_page) else {
        ui.label("No page.");
        return;
    };

    egui::ScrollArea::vertical()
        .id_salt("g_main_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                let avail = ui.available_width();
                let content_w = avail.min(PAGE_MAX_WIDTH.value());
                let side = ((avail - content_w) / 2.0).max(0.0);
                // 좌우 여백 + 본문 컬럼 (research §1.2 padding 34 40).
                ui.add_space(side + theme.spacing_xl.value() + theme.spacing_lg.value());
                ui.vertical(|ui| {
                    ui.set_max_width(content_w - theme.spacing_xl.value() * 2.0);
                    ui.add_space(theme.spacing_xl.value() + theme.spacing_md.value());
                    page_head(ui, theme, page.category);
                    for sec in &page.sections {
                        crate::catalog::spec::section(ui, theme, sec.title);
                        for sp in &sec.specs {
                            crate::catalog::spec::spec(ui, theme, sp.title, sp.when);
                            // 각 specimen 을 고유 id scope 로 감싼다 — 여러 draw 가
                            // 같은 페이지에 쌓일 때 내부 위젯/ScrollArea id 충돌 방지.
                            ui.push_id(sp.id, |ui| (sp.draw)(ui, theme));
                        }
                    }
                    ui.add_space(theme.spacing_xl.value() * 4.0);
                });
            });
        });
}

/// pagehead — h1 + intro + (Foundations 만) HowTo 3컬럼 배너.
fn page_head(ui: &mut egui::Ui, theme: &Theme, category: Category) {
    ui.label(
        egui::RichText::new(category.label())
            .size(theme.font_size_prose_h1.value())
            .strong()
            .color(egui::Color32::from(theme.text_primary())),
    );
    ui.add_space(theme.spacing_sm.value());
    ui.label(
        egui::RichText::new(category.intro())
            .size(theme.font_size_max.value())
            .color(egui::Color32::from(theme.text_secondary())),
    );
    if category.howto() {
        ui.add_space(theme.spacing_lg.value());
        let cells = [
            ("Usage", "Each specimen is the real Theme-driven widget."),
            ("Tokens used", "Every value resolves from a semantic token."),
            ("Specs toggle", "Turn On to overlay the 4px grid (WIP)."),
        ];
        egui::Frame::new()
            .fill(egui::Color32::from(theme.bg_panel()))
            .stroke(egui::Stroke::new(
                theme.border_width.value(),
                egui::Color32::from(theme.separator),
            ))
            .corner_radius(theme.corner_radius.value())
            .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
            .show(ui, |ui| {
                ui.columns(3, |cols| {
                    for (i, (k, v)) in cells.iter().enumerate() {
                        cols[i].label(
                            egui::RichText::new(k.to_uppercase())
                                .size(theme.font_size_micro.value())
                                .color(egui::Color32::from(theme.accent_primary())),
                        );
                        cols[i].add_space(theme.spacing_xs.value());
                        cols[i].label(
                            egui::RichText::new(*v)
                                .size(theme.font_size_term_sm.value())
                                .color(egui::Color32::from(theme.text_secondary())),
                        );
                    }
                });
            });
    }
}

fn nav_heading(ui: &mut egui::Ui, text: &str, size: f32, color: egui::Color32) {
    ui.add_space(size);
    ui.label(egui::RichText::new(text.to_uppercase()).size(size).color(color));
    ui.add_space(size * 0.4);
}

#[derive(Clone, Copy)]
struct NavLinkStyle {
    primary: egui::Color32,
    muted: egui::Color32,
    surf_active: egui::Color32,
    radius: f32,
    font_lbl: f32,
    font_desc: f32,
    height: f32,
}

/// nav Catalog 링크 한 줄 — 좌 lbl + 우 desc, 활성 시 surface-active 배경.
fn nav_link(
    ui: &mut egui::Ui,
    s: NavLinkStyle,
    lbl: &str,
    desc: &str,
    active: bool,
) -> bool {
    let fg = if active { s.primary } else { s.muted };
    let bg = if active {
        s.surf_active
    } else {
        egui::Color32::TRANSPARENT
    };
    let frame = egui::Frame::new()
        .fill(bg)
        .corner_radius(s.radius)
        .inner_margin(egui::Margin::symmetric(8, 4));
    let resp = frame
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(lbl).size(s.font_lbl).color(fg));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(egui::RichText::new(desc).size(s.font_desc).color(s.muted));
                });
            });
        })
        .response;
    let resp = resp.interact(egui::Sense::click());
    let _ = s.height;
    resp.clicked()
}

#[derive(Clone, Copy)]
struct SegStyle {
    border: egui::Color32,
    surf_raised: egui::Color32,
    surf_active: egui::Color32,
    primary: egui::Color32,
    muted: egui::Color32,
    radius: f32,
    border_w: f32,
    height: f32,
    font: f32,
}

/// 라벨 붙은 세그 — mono micro 라벨 + 세그 컨트롤. 클릭된 옵션 index 반환.
fn seg_with_label(
    ui: &mut egui::Ui,
    s: &SegStyle,
    label_font: f32,
    label: &str,
    items: &[(&str, bool)],
) -> Option<usize> {
    // right_to_left 컨텍스트라 세그를 먼저, 라벨을 그 좌측에 둔다.
    let clicked = seg(ui, s, items);
    ui.label(
        egui::RichText::new(label.to_uppercase())
            .size(label_font)
            .color(s.muted),
    );
    clicked
}

/// 세그먼트 컨트롤 — border 1px + radius, 버튼들 사이 border-left.
fn seg(ui: &mut egui::Ui, s: &SegStyle, items: &[(&str, bool)]) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::new()
        .fill(s.surf_raised)
        .stroke(egui::Stroke::new(s.border_w, s.border))
        .corner_radius(s.radius)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(|ui| {
                for (i, (label, active)) in items.iter().enumerate() {
                    if i > 0 {
                        let (r, _) = ui.allocate_exact_size(
                            egui::vec2(s.border_w, s.height),
                            egui::Sense::hover(),
                        );
                        ui.painter().vline(
                            r.center().x,
                            r.y_range(),
                            egui::Stroke::new(s.border_w, s.border),
                        );
                    }
                    let fg = if *active { s.primary } else { s.muted };
                    let bg = if *active {
                        s.surf_active
                    } else {
                        egui::Color32::TRANSPARENT
                    };
                    let btn = egui::Button::new(
                        egui::RichText::new(*label).size(s.font).color(fg),
                    )
                    .fill(bg)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(0.0)
                    .min_size(egui::vec2(0.0, s.height));
                    if ui.add(btn).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
    clicked
}
