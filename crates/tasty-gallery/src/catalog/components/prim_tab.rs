//! 시안 `Tab` 컴포넌트의 정적 예제. 아이콘 · 제목 · 오른쪽 점 · 닫기 순서다.
//! 점은 표면의 실행 중 여부만 나타낸다(busy = 초록 점, idle = 점 없음). `attached` 는 idle 점에
//! 연보라 고리를 두르고, `notif` 는 제목을 경고색으로 칠한다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::icons::{CLOSE, MARKDOWN, MockGlyph, TERMINAL};
use crate::catalog::spec::{StageVariant, TokenChip, meta, note, stage};

struct TabSpec {
    label: &'static str,
    icon: MockGlyph,
    active: bool,
    busy: bool,
    attached: bool,
    notif: bool,
}

const TABS: [TabSpec; 3] = [
    TabSpec {
        label: "build.sh",
        icon: TERMINAL,
        active: true,
        busy: true,
        attached: false,
        notif: false,
    },
    TabSpec {
        label: "README.md",
        icon: MARKDOWN,
        active: false,
        busy: false,
        attached: true,
        notif: false,
    },
    TabSpec {
        label: "server.log",
        icon: TERMINAL,
        active: false,
        busy: false,
        attached: false,
        notif: true,
    },
];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Tight, |ui| {
        let strip_h = theme.item_height_tab.value();
        let tab_w = theme.tab_width.value();
        let strip_w = tab_w * TABS.len() as f32;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(strip_w, strip_h), egui::Sense::hover());
        let painter = ui.painter_at(rect);
        let sep = egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui_premultiplied(),
        );

        painter.rect_filled(rect, 0.0, egui::Color32::from(theme.bg_sidebar()));
        painter.hline(rect.x_range(), rect.bottom(), sep);

        for (i, tab) in TABS.iter().enumerate() {
            let x = rect.left() + tab_w * i as f32;
            let tab_rect =
                egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(tab_w, strip_h));
            draw_tab(ui, &painter, theme, tab_rect, tab);
            // 시안 탭은 오른쪽 테두리로 칸을 나눈다.
            painter.vline(tab_rect.right(), tab_rect.y_range(), sep);
        }
    });

    meta(
        ui,
        theme,
        &[
            ("height", "24px control-height-tab"),
            ("width", "150px tab-width"),
            ("active", "accent top bar + panel fill"),
            ("close", "hover-revealed"),
        ],
        &[
            TokenChip::new(
                "bg-panel",
                "active fill",
                egui::Color32::from(theme.bg_panel()),
            ),
            TokenChip::new(
                "accent-primary",
                "active bar",
                egui::Color32::from(theme.accent_primary()),
            ),
            TokenChip::without_color("separator", "dividers"),
        ],
    );
    note(
        ui,
        theme,
        "The body tab strip (src/adapters/ui/tab_bar/tab.rs) tints the icon with the label \
         colour; this spec draws the kit text-muted icon.",
    );
}

fn draw_tab(
    ui: &egui::Ui,
    painter: &egui::Painter,
    theme: &Theme,
    rect: egui::Rect,
    tab: &TabSpec,
) {
    if tab.active {
        painter.rect_filled(rect, 0.0, egui::Color32::from(theme.bg_panel()));
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(rect.width(), theme.tab_indicator_width().value()),
        );
        painter.rect_filled(bar, 0.0, egui::Color32::from(theme.accent_primary()));
    }

    // 시안 `.tasty-tab`: padding 0 space-sm 0 space-md, gap space-sm.
    let gap = theme.spacing_sm.value();
    let cy = rect.center().y;
    let label_color = if tab.notif {
        egui::Color32::from(theme.status_dot_warning())
    } else if tab.active {
        egui::Color32::from(theme.text_primary())
    } else {
        egui::Color32::from(theme.text_muted())
    };

    let icon = theme.icon_glyph_size_sm.value();
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(rect.left() + theme.spacing_md.value(), cy - icon * 0.5),
        egui::vec2(icon, icon),
    );
    // 시안 `.tasty-tab__icon` 은 상태와 무관하게 text-muted 다.
    tab.icon
        .image(icon, egui::Color32::from(theme.text_muted()))
        .paint_at(ui, icon_rect);

    // 오른쪽 끝부터 닫기 · 점 순서로 자리를 잡는다. 닫기는 활성 탭에서만 보인다(hover 는 정적 예제에서 생략).
    let close = theme.tab_close_size().value();
    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - gap - close * 0.5, cy),
        egui::vec2(close, close),
    );
    if tab.active {
        let glyph = theme.icon_glyph_size_xs.value();
        CLOSE
            .image(glyph, egui::Color32::from(theme.text_muted()))
            .paint_at(
                ui,
                egui::Rect::from_center_size(close_rect.center(), egui::vec2(glyph, glyph)),
            );
    }
    let mut label_right = close_rect.left() - gap;
    if tab.busy || tab.attached {
        let d = theme.tab_dot_size().value();
        let c = egui::pos2(label_right - d * 0.5, cy);
        let fill = if tab.busy {
            theme.status_dot_success()
        } else {
            theme.status_dot_idle()
        };
        painter.circle_filled(c, d * 0.5, egui::Color32::from(fill));
        if tab.attached {
            let ring = theme.status_dot_attached_ring_width().value();
            let offset = theme.status_dot_attached_ring_offset().value();
            painter.circle_stroke(
                c,
                d * 0.5 + offset + ring * 0.5,
                egui::Stroke::new(ring, egui::Color32::from(theme.status_dot_attached_ring())),
            );
        }
        label_right = c.x - d * 0.5 - gap;
    }

    // 시안 제목은 `text-overflow: ellipsis` 로 남는 폭에서 말줄임한다.
    let text_x = icon_rect.right() + gap;
    let mut job = egui::text::LayoutJob::single_section(
        tab.label.to_owned(),
        egui::TextFormat::simple(
            egui::FontId::proportional(theme.font_size_body.value()),
            label_color,
        ),
    );
    job.wrap = egui::text::TextWrapping {
        max_width: (label_right - text_x).max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('\u{2026}'),
    };
    let galley = painter.layout_job(job);
    painter.galley(
        egui::pos2(text_x, cy - galley.size().y * 0.5),
        galley,
        label_color,
    );
}
