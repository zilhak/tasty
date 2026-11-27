//! 시안 `Tab` 컴포넌트의 정적 예제. 아이콘 · 제목 · 오른쪽 점 · 닫기 순서다.
//! 점은 표면의 실행 중 여부만 나타낸다(busy = 초록 점, idle = 점 없음). `attached` 는 idle 점에
//! 연보라 고리를 두르고, `notif` 는 제목을 경고색으로 칠한다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Attention, tab_title_color};

use crate::catalog::icons::{CLOSE, MARKDOWN, MockGlyph, TERMINAL};
use crate::catalog::spec::{StageVariant, TokenChip, meta, note, stage};

struct TabSpec {
    label: &'static str,
    icon: MockGlyph,
    active: bool,
    busy: bool,
    attached: bool,
    notif: bool,
    /// 제목의 attention. 있으면 본체와 같은 `tab_title_color`로 칠한다(활성 탭에서도 이긴다).
    attention: Option<Attention>,
}

const TABS: [TabSpec; 3] = [
    TabSpec {
        label: "build.sh",
        icon: TERMINAL,
        active: true,
        busy: true,
        attached: false,
        notif: false,
        attention: None,
    },
    TabSpec {
        label: "README.md",
        icon: MARKDOWN,
        active: false,
        busy: false,
        attached: true,
        notif: false,
        attention: None,
    },
    TabSpec {
        label: "server.log",
        icon: TERMINAL,
        active: false,
        busy: false,
        attached: false,
        notif: true,
        attention: None,
    },
];

/// 시안 둘째 Stage: attention을 지우지 못한 활성 탭이 제목 색을 지킨다. (캡션, 활성 탭, 옆 rest 탭).
const ACTIVE_ATTENTION: [(&str, TabSpec, TabSpec); 2] = [
    (
        "active · needs-input · OS window inactive",
        TabSpec {
            label: "deploy.sh",
            icon: TERMINAL,
            active: true,
            busy: false,
            attached: false,
            notif: false,
            attention: Some(Attention::NeedsInput),
        },
        REST_TAB,
    ),
    (
        "active · completion · surface hard occupied",
        TabSpec {
            label: "README.md",
            icon: MARKDOWN,
            active: true,
            busy: false,
            attached: false,
            notif: false,
            attention: Some(Attention::Completion),
        },
        REST_TAB,
    ),
];

const REST_TAB: TabSpec = TabSpec {
    label: "server.log",
    icon: TERMINAL,
    active: false,
    busy: false,
    attached: false,
    notif: false,
    attention: None,
};

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Tight, |ui| {
        strip(ui, theme, &TABS.iter().collect::<Vec<_>>());
    });
    stage(ui, theme, StageVariant::Column, |ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
        for (cap, active, rest) in &ACTIVE_ATTENTION {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                ui.label(
                    egui::RichText::new(*cap)
                        .size(theme.font_size_caption.value())
                        .color(egui::Color32::from(theme.text_muted())),
                );
                strip(ui, theme, &[active, rest]);
            });
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
         colour; this spec draws the kit icon, text-secondary on an active tab (with its \
         close) and text-muted on the others.",
    );
}

/// sidebar 바탕의 탭 줄. 탭마다 오른쪽 테두리로 칸을 나누고 줄 아래에 구분선을 긋는다.
fn strip(ui: &mut egui::Ui, theme: &Theme, tabs: &[&TabSpec]) {
    let strip_h = theme.item_height_tab.value();
    let tab_w = theme.tab_width.value();
    let strip_w = tab_w * tabs.len() as f32;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(strip_w, strip_h), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let sep = egui::Stroke::new(
        theme.border_width.value(),
        theme.separator.to_egui_premultiplied(),
    );

    painter.rect_filled(rect, 0.0, egui::Color32::from(theme.bg_sidebar()));
    painter.hline(rect.x_range(), rect.bottom(), sep);

    for (i, tab) in tabs.iter().enumerate() {
        let x = rect.left() + tab_w * i as f32;
        let tab_rect =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), egui::vec2(tab_w, strip_h));
        draw_tab(ui, &painter, theme, tab_rect, tab);
        // 시안 탭은 오른쪽 테두리로 칸을 나눈다.
        painter.vline(tab_rect.right(), tab_rect.y_range(), sep);
    }
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
    let label_color = if let Some(attention) = tab.attention {
        egui::Color32::from(tab_title_color(theme, Some(attention), tab.active))
    } else if tab.notif {
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
    // 활성 탭의 아이콘·닫기는 시안 Tab.active 계약의 text-secondary다. 비활성 탭은 text-muted 그대로다.
    let glyph_ink = if tab.active {
        egui::Color32::from(theme.text_secondary())
    } else {
        egui::Color32::from(theme.text_muted())
    };
    tab.icon.image(icon, glyph_ink).paint_at(ui, icon_rect);

    // 오른쪽 끝부터 닫기 · 점 순서로 자리를 잡는다. 닫기는 활성 탭에서만 보인다(hover 는 정적 예제에서 생략).
    let close = theme.tab_close_size().value();
    let close_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - gap - close * 0.5, cy),
        egui::vec2(close, close),
    );
    if tab.active {
        let glyph = theme.icon_glyph_size_xs.value();
        CLOSE.image(glyph, glyph_ink).paint_at(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::components::painted::{mocha_latte, paint};

    /// 둘째 Stage의 활성 탭이 attention 제목 색으로, 옆 rest 탭이 rest 색으로 칠해지는지 그린 글자로 본다.
    #[test]
    fn active_attention_tabs_keep_their_title_tint() {
        for (name, th) in mocha_latte() {
            assert_ne!(th.tab_fg_needs_input(), th.tab_fg_active(), "{name}");
            assert_ne!(th.tab_fg_completion(), th.tab_fg_active(), "{name}");
            let painted = paint(|ui| draw(ui, &th));
            let colors = |label: &str| -> Vec<egui::Color32> {
                painted
                    .texts
                    .iter()
                    .filter(|t| t.text == label)
                    .map(|t| t.color)
                    .collect()
            };
            for (attention, label, want) in [
                (Attention::NeedsInput, "deploy.sh", th.tab_fg_needs_input()),
                (Attention::Completion, "README.md", th.tab_fg_completion()),
            ] {
                assert_eq!(want, tab_title_color(&th, Some(attention), true));
                assert!(
                    colors(label).contains(&egui::Color32::from(want)),
                    "{name} {label}: {:?}",
                    colors(label)
                );
            }
            // 둘째 Stage의 두 줄 모두 옆 rest 탭 제목을 rest 색으로 칠한다(첫 줄의 notif 탭은 따로다).
            let rest = egui::Color32::from(tab_title_color(&th, None, false));
            let rests = colors(REST_TAB.label);
            assert_eq!(
                rests.iter().filter(|c| **c == rest).count(),
                ACTIVE_ATTENTION.len(),
                "{name} {}: {rests:?}",
                REST_TAB.label
            );
            // 활성 탭 셋(첫 줄 build.sh와 둘째 Stage 둘)의 아이콘·닫기는 text-secondary, 나머지 아이콘은 text-muted다.
            // Latte처럼 두 잉크가 같은 테마에서는 합계만 가를 수 있다.
            let tints = |c: egui::Color32| painted.glyphs.iter().filter(|(_, t)| *t == c).count();
            let active = (1 + ACTIVE_ATTENTION.len()) * 2;
            let rest_icons = TABS.len() - 1 + ACTIVE_ATTENTION.len();
            let (secondary, muted) = (th.text_secondary().into(), th.text_muted().into());
            if secondary == muted {
                assert_eq!(tints(secondary), active + rest_icons, "{name}");
            } else {
                assert_eq!(tints(secondary), active, "{name}");
                assert_eq!(tints(muted), rest_icons, "{name}");
            }
            let caps = ACTIVE_ATTENTION.map(|(cap, _, _)| cap);
            for cap in caps {
                assert_eq!(colors(cap).len(), 1, "{name} {cap}");
            }
        }
    }
}
