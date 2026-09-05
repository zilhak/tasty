//! 갤러리 egui 셸 — 디자인(4) gallery 의 4영역 문서 셸.
//!
//! 2×2 그리드: 좌상 brand(232×52) / 우상 top(crumb + 세그 토글) / 좌 nav(232) /
//! 우 main(활성 페이지 전체를 스크롤하는 문서 본문).
//!
//! nav 의 Catalog 그룹은 페이지 링크, "On this page" 그룹은 활성 페이지의
//! Section 앵커. main 은 활성 페이지의 모든 Section/Spec 을 `spec` 헬퍼로 렌더한다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::catalog::{self, Category, Page};

/// brand/nav 폭 (research §1.1 grid-template-columns 232px).
/// 갤러리 좌측 네비게이션의 좌우 안쪽 여백. 디자인 전사값 10 으로 4px 그리드
/// 밖이다. `egui::Margin` 필드가 `i8` 이라 타입을 맞춰 둔다.
const NAV_PAD_X: i8 = 10;

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
///
/// `pub(crate)` — Chrome 카테고리(`catalog::chrome_loading`)가 앰비언트 테마
/// 토글과 무관하게 Latte 고정 variant specimen 을 그릴 때도 재사용한다.
pub(crate) fn latte_theme() -> Theme {
    use tasty_type_appearance::theme::Theme as ThemeStruct;
    let file = tasty_themes::ThemeFile::parse(tasty_themes::LATTE_TOML_TEXT)
        .expect("builtin latte.toml ships valid");
    let (partial, is_light) = file.to_partial();
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.apply_partial(&partial);
    ThemeStruct::with_colors(colors, is_light.unwrap_or(true))
}

/// UI scale 세그 stops — 배율을 [`AppearanceSettings::ui_scale_factor_for`] 에서 **읽는다**.
///
/// 종전에는 `[("0.8", 0.8), ("1.0", 1.0), ("1.2", 1.2)]` 하드코딩 사본이었고, 주석은
/// "Appearance›Display 매핑과 동일" 이라고 적고 있었는데 **동일하지 않았다** — 본체의
/// small 은 0.85 다. 사본이 조용히 갈라진 자리다.
///
/// 그 사본을 없앨 수 있는 이유는 **의존 방향이 맞기 때문**이다. 배율 집합의 정본
/// (`tasty-settings`)에 달린 핀(`the_supported_ui_scale_set_is_pinned`)은 자기를 읽지
/// **못하는** 소비자(`tasty-type-appearance`)의 사본만 겨냥해 두었고, 갤러리는 그 소비자가
/// 아니다 — `tasty-settings` 를 이미 의존하므로 부르면 된다. 이름이 있고 부를 수도 있는데
/// 그 자리만 안 부른 형태이고, 드리프트가 그 대가였다(ADR-0126 "이름이 있는데 그 자리만
/// 안 부른다").
fn ui_scale_stops() -> Vec<(String, f32)> {
    tasty_settings::UI_SCALE_CHOICES
        .iter()
        .map(|key| {
            let factor = tasty_settings::AppearanceSettings::ui_scale_factor_for(key);
            // `{factor}` 는 1.0 을 `"1"` 로 찍는다. 세그는 배율 숫자를 나란히 읽는
            // 자리라 소수 자리가 들쭉날쭉하면 안 된다 — 두 자리로 찍고 남는 0 하나만
            // 떼어 `0.85` / `1.0` / `1.2` 로 맞춘다.
            let padded = format!("{factor:.2}");
            let label = padded.strip_suffix('0').unwrap_or(&padded).to_string();
            (label, factor)
        })
        .collect()
}

/// 갤러리 전역 상태.
pub struct GalleryState {
    /// 현재 적용 중인 테마. 본체 글로벌과 격리된 갤러리 전용 인스턴스.
    pub theme: Theme,
    /// 선택된 테마 식별자 (Theme 구조체엔 id 가 없어 별도 보관).
    pub theme_id: ThemeId,
    /// 문서 페이지 트리 (Foundations/Components/Icons/Overlays/Layouts/Plugins).
    pub pages: Vec<Page>,
    /// 현재 활성 페이지 index (`pages` 기준).
    pub active_page: usize,
    /// 사이드바 zoom 배율 (UI scale 세그).
    pub ui_scale: f32,
    /// SPECS 토글 상태 (4px grid 오버레이 — 본 단계는 상태만, 오버레이는 후순위).
    pub specs_on: bool,
    /// 다음 frame 에서 visuals/zoom 을 ctx 에 재적용할지.
    pub needs_reapply: bool,
    /// 배치 스크린샷에서만 쓰는 본문 강제 스크롤 오프셋(px). 사람이 쓰는
    /// 실행에서는 항상 `None` 이라 스크롤은 평소대로 사용자 것이다.
    pub shot_scroll: Option<f32>,
    /// 좌상단 brand 로고 텍스처 (앱 아이콘 PNG 디코드 결과, 1회 캐시).
    brand_logo: Option<egui::TextureHandle>,
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
            shot_scroll: None,
            brand_logo: None,
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
        .frame(
            egui::Frame::new()
                .fill(sidebar_bg)
                .inner_margin(egui::Margin {
                    left: NAV_PAD_X,
                    right: NAV_PAD_X,
                    top: state.theme.spacing_md.value() as i8,
                    bottom: state.theme.spacing_xl.value() as i8,
                }),
        )
        .show(ctx, |ui| nav_ui(ui, state));

    // ── 우측: main 문서 본문 ──
    egui::CentralPanel::default()
        .frame(
            egui::Frame::new()
                .fill(main_bg)
                .inner_margin(egui::Margin::ZERO),
        )
        .show(ctx, |ui| main_ui(ui, state));
}

/// 상단 헤더 — brand 블록(좌 232) + top bar(crumb + Specs/Theme/Scale 세그).
fn header_ui(ui: &mut egui::Ui, state: &mut GalleryState) {
    // Theme 의존 색/치수를 Copy 로 스냅샷 (이후 state 변이와 borrow 충돌 방지).
    let (primary, muted, melon, border, surf_raised, surf_active, separator) = {
        let t = &state.theme;
        (
            egui::Color32::from(t.text_primary()),
            egui::Color32::from(t.text_muted()),
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

    let logo_tex = brand_logo_texture(ui.ctx(), &mut state.brand_logo);

    ui.horizontal_centered(|ui| {
        // brand 블록 (좌 232).
        ui.allocate_ui_with_layout(
            egui::vec2(NAV_WIDTH.value(), HEADER_HEIGHT.value()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.add_space(pad_lg);
                let (r, _) = ui.allocate_exact_size(egui::vec2(logo, logo), egui::Sense::hover());
                // 디자인 `.g-brand img`(22px, border-radius 없음) — 앱 아이콘을 그대로 렌더.
                egui::Image::from_texture(&logo_tex)
                    .fit_to_exact_size(egui::vec2(logo, logo))
                    .paint_at(ui, r);
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
                act_theme = Some(if i == 0 {
                    ThemeId::Mocha
                } else {
                    ThemeId::Latte
                });
            }
            ui.add_space(pad_lg);
            // UI scale 세그.
            let stops = ui_scale_stops();
            let scale_items: Vec<(&str, bool)> = stops
                .iter()
                .map(|(l, s)| (l.as_str(), (ui_scale - s).abs() < 0.001))
                .collect();
            if let Some(i) = seg_with_label(ui, &seg, f_micro, "Scale", &scale_items) {
                act_scale = Some(stops[i].1);
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

/// 좌상단 brand 로고 텍스처를 1회 디코드/업로드 후 캐시에서 핸들 반환.
fn brand_logo_texture(
    ctx: &egui::Context,
    cache: &mut Option<egui::TextureHandle>,
) -> egui::TextureHandle {
    cache
        .get_or_insert_with(|| {
            ctx.load_texture(
                "brand_logo",
                decode_brand_logo(),
                egui::TextureOptions::LINEAR,
            )
        })
        .clone()
}

/// 임베드한 256×256 앱 아이콘 PNG 를 egui 이미지로 디코드 (cwd 무관).
fn decode_brand_logo() -> egui::ColorImage {
    static ICON_PNG: &[u8] = include_bytes!("../../../assets/icons/icon_256.png");
    let decoder = png::Decoder::new(std::io::Cursor::new(ICON_PNG));
    let mut reader = decoder.read_info().expect("builtin brand icon PNG decodes");
    let mut buf = vec![0u8; reader.output_buffer_size().expect("brand icon buffer size")];
    let info = reader
        .next_frame(&mut buf)
        .expect("brand icon frame decodes");
    buf.truncate(info.buffer_size());
    egui::ColorImage::from_rgba_unmultiplied([info.width as usize, info.height as usize], &buf)
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
    let border_w = state.theme.border_width.value();
    let margin_h = state.theme.spacing_sm.value() as i8;
    let margin_v = state.theme.spacing_xs.value() as i8;

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
                        margin_h,
                        margin_v,
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
                        egui::vec2(border_w, row_h * 0.8),
                        egui::Sense::hover(),
                    );
                    ui.painter().vline(
                        r.center().x,
                        r.y_range(),
                        egui::Stroke::new(border_w, separator),
                    );
                    ui.add_space(pad_sm);
                    ui.label(egui::RichText::new(*title).size(f_lbl).color(secondary));
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

    // 중앙정렬 좌측 여백(side)은 **뷰포트 폭**(CentralPanel 가용폭, ScrollArea 바깥)으로만
    // 계산한다. ScrollArea inner 의 `available_width()` 는 자식 specimen 이 뷰포트보다 넓게
    // 그리면(가로 overflow) 그 폭으로 팽창해, 중앙정렬 여백을 부풀려 콘텐츠가 페이지마다
    // 다르게 우측으로 밀리는 버그가 있었다(예: Components 만 +124px). 디자인 `.g-page`
    // (max-width 1080 · margin 0 auto · padding 0 40)처럼 좌측 여백은 콘텐츠가 아니라
    // 뷰포트에만 종속돼야 한다 → 여기서 한 번만 계산한다.
    let viewport_w = ui.available_width();
    let content_w = viewport_w.min(PAGE_MAX_WIDTH.value());
    let side = ((viewport_w - content_w) / 2.0).max(0.0);
    // 디자인 .g-page 좌우 대칭 패딩 40 (= space-xl 24 + space-lg 16).
    let pad_x = theme.spacing_xl.value() + theme.spacing_lg.value();

    let mut main_scroll = egui::ScrollArea::vertical()
        .id_salt("g_main_scroll")
        .auto_shrink([false, false]);
    if let Some(y) = state.shot_scroll {
        main_scroll = main_scroll.vertical_scroll_offset(y);
    }
    main_scroll.show(ui, |ui| {
        ui.horizontal(|ui| {
            // 좌측 = 중앙정렬 여백(뷰포트 기준) + 페이지 좌패딩 40.
            ui.add_space(side + pad_x);
            ui.vertical(|ui| {
                // 본문 컬럼 = 페이지폭 − 좌우 대칭 패딩(40×2).
                // 매우 좁은 창에서 음수가 되지 않도록 0 으로 클램프.
                let col_w = (content_w - pad_x * 2.0).max(0.0);
                ui.set_max_width(col_w);
                // spec::note 가 이 폭으로 문단을 줄바꿈하도록 심어둔다. specimen 무대가
                // 컬럼보다 넓게 그리면 top_down max_rect 가 늘어나 note 의 available_width
                // 가 팽창하므로, note 는 available_width 대신 이 값을 wrap 폭으로 쓴다.
                ui.data_mut(|d| d.insert_temp(crate::catalog::spec::body_column_width_id(), col_w));
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
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(size)
            .color(color),
    );
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
    margin_h: i8,
    margin_v: i8,
}

/// nav Catalog 링크 한 줄 — 좌 lbl + 우 desc, 활성 시 surface-active 배경.
fn nav_link(ui: &mut egui::Ui, s: NavLinkStyle, lbl: &str, desc: &str, active: bool) -> bool {
    let fg = if active { s.primary } else { s.muted };
    let bg = if active {
        s.surf_active
    } else {
        egui::Color32::TRANSPARENT
    };
    let frame = egui::Frame::new()
        .fill(bg)
        .corner_radius(s.radius)
        .inner_margin(egui::Margin::symmetric(s.margin_h, s.margin_v));
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
                    let btn = egui::Button::new(egui::RichText::new(*label).size(s.font).color(fg))
                        .fill(bg)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(egui::CornerRadius::ZERO)
                        .min_size(egui::vec2(0.0, s.height));
                    if ui.add(btn).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
    clicked
}
