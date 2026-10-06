//! 탐색기 내용 영역의 상태 화면 — 빈 폴더 · 권한 거부 · 불러오는 중.
//! 시안 `ExpState`(갤러리 Spec "Empty / permission / loading")를 따른다: 가운데 정렬한 글리프 ·
//! 제목(body, text-secondary) · 선택 보조 줄(caption, text-muted)을 space-sm 간격으로 쌓는다.
//! 권한 거부는 글리프와 제목을 accent-warning 으로 칠하고, 불러오는 중은 글리프 자리에 Spinner 를 둔다.
//! 시안의 패널 배경·테두리는 갤러리 전시 칸이고 본체에서는 내용 영역 자체가 그 자리다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::Spinner;

use crate::adapters::ui::icons::Icon;

/// 시안 글리프 확대 비율(`transform: scale(1.6)`). 대응 토큰이 없다.
const GLYPH_SCALE: f32 = 1.6;
/// 시안 보조 줄 최대 폭(`maxWidth: 200`). Theme 역할에 연결하지 않은 화면 전용 고정 치수다(ADR-0035).
const SUB_MAX_W: LogicalPx = LogicalPx(200.0);

/// 상태 화면의 글리프 — 아이콘 또는 Spinner.
#[derive(Clone, Copy)]
pub(super) enum StateGlyph {
    Icon(Icon),
    Spinner,
}

/// 상태 화면 하나의 내용.
pub(super) struct StateScreen<'a> {
    pub glyph: StateGlyph,
    /// 권한 거부처럼 경고 톤으로 칠하는 상태.
    pub warning: bool,
    pub title: &'a str,
    pub sub: Option<&'a str>,
}

/// 받은 영역 전체를 차지하고 그 가운데에 상태 블록을 그린다. 그린 블록의 사각형을 돌려준다.
pub(super) fn show(ui: &mut egui::Ui, theme: &Theme, s: &StateScreen<'_>) -> egui::Rect {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ui.available_height()),
        egui::Sense::hover(),
    );
    let warning = theme.accent_warning().to_egui();
    let glyph_fg = if s.warning {
        warning
    } else {
        theme.text_muted().to_egui()
    };
    let title_fg = if s.warning {
        warning
    } else {
        theme.text_secondary().to_egui()
    };
    // transform: scale 은 배치에 영향이 없다. 배치는 원래 글리프 크기로 하고 그림만 확대한다.
    let glyph_box = theme.icon_glyph_size_md.value();
    let glyph = glyph_box * GLYPH_SCALE;
    let gap = theme.spacing_sm.value();
    let inner_w = (rect.width() - theme.spacing_lg.value() * 2.0).max(0.0);
    let center = |text: &str, size: f32, color: egui::Color32, max_w: f32| {
        let mut job = egui::text::LayoutJob::simple(
            text.to_owned(),
            egui::FontId::proportional(size),
            color,
            max_w,
        );
        job.halign = egui::Align::Center;
        ui.painter().layout_job(job)
    };
    let title = center(s.title, theme.font_size_body.value(), title_fg, inner_w);
    let sub = s.sub.map(|t| {
        center(
            t,
            theme.font_size_caption.value(),
            theme.text_muted().to_egui(),
            SUB_MAX_W.value().min(inner_w),
        )
    });
    let block_h =
        glyph_box + gap + title.rect.height() + sub.as_ref().map_or(0.0, |g| gap + g.rect.height());
    let top = rect.center().y - block_h * 0.5;
    let glyph_rect = egui::Rect::from_center_size(
        egui::pos2(rect.center().x, top + glyph_box * 0.5),
        egui::vec2(glyph, glyph),
    );
    match s.glyph {
        StateGlyph::Icon(icon) => {
            icon.image(glyph, glyph_fg).paint_at(ui, glyph_rect);
        }
        StateGlyph::Spinner => {
            let mut slot = ui.new_child(egui::UiBuilder::new().max_rect(glyph_rect));
            Spinner::new().size(glyph).show(&mut slot, theme);
        }
    }
    let mut y = top + glyph_box + gap;
    // 가운데 정렬 job 의 원점은 줄 가운데다.
    let title_h = title.rect.height();
    ui.painter()
        .galley(egui::pos2(rect.center().x, y), title, title_fg);
    y += title_h;
    if let Some(g) = sub {
        y += gap;
        let h = g.rect.height();
        ui.painter().galley(
            egui::pos2(rect.center().x, y),
            g,
            theme.text_muted().to_egui(),
        );
        y += h;
    }
    egui::Rect::from_min_max(egui::pos2(rect.left(), top), egui::pos2(rect.right(), y))
}
