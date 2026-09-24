//! 플러그인 관리 창의 Installed·Attention·Add plugin 예제.
//! 본체 뷰는 Context에 직접 패널을 붙이므로 여기서는 같은 구성을 주어진 영역에 그린다.

mod add;
mod attention;
mod installed;

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::STRUCT_GAP_2;

use crate::catalog::icons::{CLOSE, PLUG};
use crate::catalog::spec::{self, StageVariant, TokenChip};

/// 본체 SEGMENT_TAB_LABEL_PRIMITIVE_12와 같은 12px 글꼴. 대응 semantic 토큰이 없다.
const SEGMENT_TAB_LABEL_PRIMITIVE_12: LogicalPx = LogicalPx(12.0);

/// 세그먼트 탭 셋 — 본체 `PluginsUiState.tab`. 세 탭은 서로 다른 본문을 그린다.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    /// Installed 는 상세가 세 갈래다 — 무선택 / 선택 / uninstall 확인.
    Installed { detail: installed::Detail },
    /// Attention 은 대상이 0 이면 본문이 통째로 안내로 바뀌고 세그먼트 배지도 사라진다.
    Attention { empty: bool },
    /// `Add plugin` 은 상태가 둘이라 어느 쪽을 그릴지 함께 든다.
    Add { preview: bool },
}

/// 본체 Installed·Attention 목록과 같은 너비 접근자를 사용한다.
fn list_w(theme: &Theme) -> f32 {
    theme.plugins_side_panel_width().value()
}

/// 예제 창의 크기. 상세 정보를 한눈에 비교하도록 Installed 화면은 더 높게 잡는다.
fn stage_size(theme: &Theme, tab: Tab) -> egui::Vec2 {
    let h = match tab {
        Tab::Installed { .. } => theme.measure_xl,
        _ => theme.measure_sm,
    };
    egui::vec2(list_w(theme) + theme.measure_md.value(), h.value())
}

/// 한 세그먼트 탭의 표시 상태 — 본체 `segment_tab` 의 뒤쪽 인자 4개를 묶은 것.
struct Segment<'a> {
    label: &'a str,
    count: Option<usize>,
    danger: bool,
    selected: bool,
}

/// 본체 `segment_tab` 전사 — 라벨 + (count 또는 danger 배지), selected 면 채운 배경.
fn segment_tab(
    ui: &mut egui::Ui,
    theme: &Theme,
    p: &egui::Painter,
    at: egui::Pos2,
    seg: &Segment<'_>,
) -> f32 {
    let Segment {
        label,
        count,
        danger,
        selected,
    } = *seg;
    let label_color = if selected {
        theme.text_primary().to_egui()
    } else {
        theme.text_muted().to_egui()
    };
    let count_color = if selected {
        theme.text_secondary().to_egui()
    } else {
        theme.text_muted().to_egui()
    };
    let label_font = egui::FontId::proportional(SEGMENT_TAB_LABEL_PRIMITIVE_12.value());
    let label_galley = p.layout_no_wrap(label.to_string(), label_font, label_color);

    let badge = danger && count.is_some_and(|c| c > 0);
    let badge_galley = badge.then(|| {
        p.layout_no_wrap(
            count.unwrap_or(0).to_string(),
            egui::FontId::proportional(theme.badge_font_size().value()),
            theme.text_on_accent().to_egui(),
        )
    });
    let count_galley = (!danger)
        .then(|| {
            count.map(|c| {
                p.layout_no_wrap(
                    c.to_string(),
                    egui::FontId::monospace(theme.font_size_micro.value()),
                    count_color,
                )
            })
        })
        .flatten();

    let pad_x = theme.spacing_md.value();
    let gap = theme.spacing_sm.value();
    let height = theme.item_height_tab.value() + STRUCT_GAP_2.value();
    let badge_h = theme.spacing_lg.value();
    let badge_pad = theme.spacing_xs.value();

    let mut width = label_galley.size().x + pad_x * 2.0;
    if let Some(g) = &count_galley {
        width += gap + g.size().x;
    }
    if let Some(g) = &badge_galley {
        width += gap + (g.size().x + badge_pad * 2.0).max(badge_h);
    }

    let rect = egui::Rect::from_min_size(at, egui::vec2(width, height));
    if selected {
        p.rect(
            rect,
            theme.corner_radius.value(),
            theme.surface_active().to_egui(),
            egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
            egui::StrokeKind::Inside,
        );
    }

    let mut x = rect.min.x + pad_x;
    p.galley(
        egui::pos2(x, rect.center().y - label_galley.size().y * 0.5),
        label_galley.clone(),
        label_color,
    );
    x += label_galley.size().x;
    if let Some(g) = count_galley {
        x += gap;
        p.galley(
            egui::pos2(x, rect.center().y - g.size().y * 0.5),
            g,
            count_color,
        );
    }
    if let Some(g) = badge_galley {
        x += gap;
        let bw = (g.size().x + badge_pad * 2.0).max(badge_h);
        let brect = egui::Rect::from_min_size(
            egui::pos2(x, rect.center().y - badge_h * 0.5),
            egui::vec2(bw, badge_h),
        );
        p.rect_filled(
            brect,
            theme.corner_radius.value(),
            theme.accent_danger().to_egui(),
        );
        p.galley(
            egui::pos2(
                brect.center().x - g.size().x * 0.5,
                brect.center().y - g.size().y * 0.5,
            ),
            g,
            theme.text_on_accent().to_egui(),
        );
    }
    // 선택되지 않은 탭도 같은 폭을 차지한다 (본체 allocate_exact_size 와 동일).
    let _ = ui;
    width
}

/// 헤더 밴드 (높이 48) — 본체 `TopBottomPanel::top("plugins_header")`.
/// 선택 세그먼트와 필터 입력의 유무가 `tab` 에 달려 있다.
fn header(ui: &mut egui::Ui, theme: &Theme, rect: egui::Rect, tab: Tab) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());
    p.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );

    let cy = rect.center().y;
    let mut x = rect.min.x + theme.spacing_md.value();

    // 헤더 아이콘은 주의 환기가 아닌 장식용 색을 쓴다.
    let icon = theme.icon_glyph_size_md.value();
    PLUG.image(icon, theme.plugins_header_glyph().to_egui())
        .paint_at(
            ui,
            egui::Rect::from_center_size(egui::pos2(x + icon * 0.5, cy), egui::vec2(icon, icon)),
        );
    x += icon + theme.spacing_xs.value();

    let title_font = egui::FontId::proportional(theme.font_size_max.value());
    let title = p.layout_no_wrap(
        "Plugins".to_string(),
        title_font,
        theme.text_primary().to_egui(),
    );
    p.galley(
        egui::pos2(x, cy - title.size().y * 0.5),
        title.clone(),
        theme.text_primary().to_egui(),
    );
    x += title.size().x + theme.spacing_sm.value();

    let div_h = theme.spacing_lg.value() + theme.spacing_xs.value();
    p.rect_filled(
        egui::Rect::from_min_size(
            egui::pos2(x, cy - div_h * 0.5),
            egui::vec2(theme.border_width.value(), div_h),
        ),
        0.0,
        theme.separator.to_egui(),
    );
    x += theme.border_width.value() + theme.spacing_sm.value();

    let tab_y = cy - (theme.item_height_tab.value() + STRUCT_GAP_2.value()) * 0.5;
    x += segment_tab(
        ui,
        theme,
        &p,
        egui::pos2(x, tab_y),
        &Segment {
            label: "Installed",
            count: Some(installed::ROWS.len()),
            danger: false,
            selected: matches!(tab, Tab::Installed { .. }),
        },
    ) + STRUCT_GAP_2.value();
    x += segment_tab(
        ui,
        theme,
        &p,
        egui::pos2(x, tab_y),
        &Segment {
            label: "Attention",
            // 0개일 때 배지가 사라지는 규칙도 이 함수에서 확인한다.
            count: Some(match tab {
                Tab::Attention { empty: true } => 0,
                _ => attention::ENTRIES.len(),
            }),
            danger: true,
            selected: matches!(tab, Tab::Attention { .. }),
        },
    ) + STRUCT_GAP_2.value();
    segment_tab(
        ui,
        theme,
        &p,
        egui::pos2(x, tab_y),
        &Segment {
            label: "Add plugin",
            count: None,
            danger: false,
            selected: matches!(tab, Tab::Add { .. }),
        },
    );

    let close = theme.item_height_interactive.value();
    let close_rect = egui::Rect::from_min_size(
        egui::pos2(
            rect.max.x - theme.spacing_sm.value() - close,
            cy - close * 0.5,
        ),
        egui::vec2(close, close),
    );
    CLOSE
        .image(
            theme.icon_glyph_size_md.value(),
            theme.text_secondary().to_egui(),
        )
        .paint_at(
            ui,
            egui::Rect::from_center_size(
                close_rect.center(),
                egui::Vec2::splat(theme.icon_glyph_size_md.value()),
            ),
        );

    if !matches!(tab, Tab::Installed { .. }) {
        return;
    }

    let filter_w = theme.field_width_lg.value();
    let filter_h = theme.item_height_interactive.value();
    let filter_rect = egui::Rect::from_min_size(
        egui::pos2(
            close_rect.min.x - theme.spacing_sm.value() - filter_w,
            cy - filter_h * 0.5,
        ),
        egui::vec2(filter_w, filter_h),
    );
    p.rect(
        filter_rect,
        theme.corner_radius.value(),
        theme.surface_raised().to_egui(),
        egui::Stroke::new(theme.border_width.value(), theme.border_default().to_egui()),
        egui::StrokeKind::Inside,
    );
    p.text(
        egui::pos2(
            filter_rect.min.x + theme.spacing_sm.value(),
            filter_rect.center().y,
        ),
        egui::Align2::LEFT_CENTER,
        "Filter installed…",
        egui::FontId::proportional(theme.font_size_body.value()),
        theme.text_placeholder().to_egui(),
    );
}

fn window(ui: &mut egui::Ui, theme: &Theme, tab: Tab) {
    let (rect, _) = ui.allocate_exact_size(stage_size(theme, tab), egui::Sense::hover());
    ui.painter().rect_filled(
        rect,
        theme.corner_radius.value(),
        theme.bg_panel().to_egui(),
    );

    let header_h =
        theme.item_height_interactive.value() + theme.spacing_lg.value() + theme.spacing_xs.value();
    let header_rect = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), header_h));
    header(ui, theme, header_rect, tab);

    let body_top = header_rect.bottom();
    let body = egui::Rect::from_min_max(egui::pos2(rect.min.x, body_top), rect.max);

    let panes = |body: egui::Rect| {
        let list_rect =
            egui::Rect::from_min_max(body.min, egui::pos2(body.min.x + list_w(theme), body.max.y));
        let detail_rect = egui::Rect::from_min_max(egui::pos2(list_rect.max.x, body_top), body.max);
        (list_rect, detail_rect)
    };
    let divider = |ui: &mut egui::Ui, x: f32| {
        ui.painter().vline(
            x,
            egui::Rangef::new(body_top, body.max.y),
            egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
        );
    };

    match tab {
        Tab::Installed { detail } => {
            let (l, d) = panes(body);
            installed::list_pane(ui, theme, l, detail);
            installed::detail_pane(ui, theme, d, detail);
            divider(ui, l.max.x);
        }
        Tab::Attention { empty } => {
            let (l, d) = panes(body);
            if empty {
                attention::empty_list_pane(ui, theme, l);
                attention::empty_detail_pane(ui, theme, d);
            } else {
                attention::list_pane(ui, theme, l);
                attention::detail_pane(ui, theme, d);
            }
            divider(ui, l.max.x);
        }
        Tab::Add { preview: false } => add::input_pane(ui, theme, body),
        Tab::Add { preview: true } => add::preview_pane(ui, theme, body),
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    for (label, tab) in [
        (
            "Installed",
            Tab::Installed {
                detail: installed::Detail::Selected(0),
            },
        ),
        (
            "Installed — health error",
            Tab::Installed {
                detail: installed::Detail::Selected(1),
            },
        ),
        (
            "Installed — no selection",
            Tab::Installed {
                detail: installed::Detail::None,
            },
        ),
        (
            "Installed — uninstall confirm",
            Tab::Installed {
                detail: installed::Detail::ConfirmUninstall(0),
            },
        ),
        ("Attention", Tab::Attention { empty: false }),
        ("Attention — empty", Tab::Attention { empty: true }),
        ("Add plugin — path input", Tab::Add { preview: false }),
        (
            "Add plugin — manifest preview (untrusted)",
            Tab::Add { preview: true },
        ),
    ] {
        spec::cluster(ui, theme, label, |ui| {
            spec::stage(ui, theme, StageVariant::Solo, |ui| window(ui, theme, tab));
        });
    }
    spec::cluster(ui, theme, "attention reasons (4)", |ui| {
        attention::reason_cards(ui, theme);
    });

    spec::meta(
        ui,
        theme,
        &[
            (
                "header",
                "높이 48 · plug + 타이틀 + 1px 구분선 + 세그먼트 3",
            ),
            (
                "segments",
                "Installed N(mono count) / Attention N(danger 배지) / Add plugin",
            ),
            (
                "list",
                "폭 `plugins_side_panel_width` · 아바타 32 · 이름 13 + 부제 10 muted",
            ),
            ("builtin", "이름 뒤 `•` · 상세는 accent-agent 배지"),
            (
                "installed detail",
                "identity → 설명 → (health 박스) → authors/homepage → Status/Configure → surface kinds → permissions → commands → 경로 → uninstall",
            ),
            ("health error", "enabled + error 인 행만 우측 danger dot"),
            (
                "attention",
                "같은 2열 · 2번째 줄이 사유 라벨(severity 색) · 상세는 배너 + 사유 detail + 액션 바 · 0 건이면 배지가 사라지고 본문이 안내로 바뀐다",
            ),
            (
                "add",
                "단일 열 · 경로 입력(입력+Verify / 구분선 / 폴더 찾기)과 매니페스트 프리뷰 두 상태",
            ),
        ],
        &[
            TokenChip::new(
                "accent-decorative",
                "header plug",
                theme.plugins_header_glyph().to_egui(),
            ),
            TokenChip::new(
                "accent-danger",
                "attention badge · health dot",
                theme.accent_danger().to_egui(),
            ),
            TokenChip::new(
                "surface-active",
                "selected row · tab",
                theme.surface_active().to_egui(),
            ),
            TokenChip::new("bg-sidebar", "header · list", theme.bg_sidebar().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "세 탭은 서로 다른 일을 한다 — Installed 는 설치된 plugin 의 상태·권한·명령을 보고, \
         Attention 은 서명·권한 변경으로 등록이 거부됐거나 런타임에서 실패한 plugin 만 모아 \
         이유와 필요한 조치를 보여주며, Add plugin 은 로컬 폴더 경로로 설치한다. 검색 입력은 \
         목록이 있는 Installed 탭에서만 나타난다. 사용자가 직접 끈 plugin 은 error 가 아니라 \
         정상 종료이므로 danger dot 이 붙지 않는다.",
    );

    spec::note(
        ui,
        theme,
        "Attention 의 severity 는 사유로 갈린다 — 서명 계열(신뢰 안 됨 · 무효)은 danger, \
         권한 변경과 런타임 오류는 warning 이다. 이것은 Installed 목록의 health dot 과 다른 \
         축이다: health dot 은 '실행 중 실패' 하나만 보고, Attention 은 등록 거부까지 함께 \
         모은다. Add 는 미신뢰 plugin 에 공개키가 있을 때만 Add 버튼이 살아 있다 — 공개키가 \
         없거나 서명 오류면 신뢰를 등록할 방법이 없어 버튼이 꺼진다.",
    );
}
