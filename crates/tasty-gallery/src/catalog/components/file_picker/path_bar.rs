//! `file_picker` specimen 의 path bar — breadcrumb(깊으면 가운데 생략) + refresh.
//! 권위 원본: `gallery/overlays-shared.jsx` `FpCrumbs`(`elide`) · `FilePickerFrame` path bar.

use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::{IconButton, IconButtonVariant};

use super::{CRUMB_GLYPH, CRUMB_MAX_W, FRAME_W, HEADER_PAD_L, HOST, PATH_H, Variant, elide};
use crate::catalog::icons;
use crate::catalog::widgets::dialog as kit;
use tasty_type_appearance::theme::Theme;

pub(super) fn path_bar(ui: &mut egui::Ui, theme: &Theme, v: Variant) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(FRAME_W.value(), PATH_H.value()),
        egui::Sense::hover(),
    );
    ui.painter()
        .rect_filled(rect, 0.0, theme.bg_sidebar().to_egui());
    ui.painter().hline(
        rect.x_range(),
        rect.bottom(),
        egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()),
    );
    let inner = egui::Rect::from_min_max(
        egui::pos2(rect.left() + HEADER_PAD_L.value(), rect.top()),
        egui::pos2(rect.right() - theme.spacing_sm.value(), rect.bottom()),
    );
    // refresh(flex:none) 가 오른쪽 끝을 먼저 차지하고, breadcrumb 은 남은 폭(flex:1;
    // min-width:0; overflow:hidden) 안에서만 그린다 — 경로가 길어도 버튼을 밀지 않는다.
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .show(&mut child, theme, &|ui, rect, c| {
            icons::REFRESH.image(rect.height(), c).paint_at(ui, rect)
        });
    let crumbs_rect = egui::Rect::from_min_max(
        inner.min,
        egui::pos2(child.cursor().right().max(inner.left()), inner.max.y),
    );
    let mut crumbs_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(crumbs_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    crumbs_ui.set_clip_rect(crumbs_rect.intersect(ui.clip_rect()));
    crumbs(&mut crumbs_ui, theme, v);
}

struct Crumb {
    label: &'static str,
    root: bool,
    current: bool,
}

const fn crumb(label: &'static str) -> Crumb {
    Crumb {
        label,
        root: false,
        current: false,
    }
}

/// 브레드크럼 — root(mono) → 중간(accent 링크) → current(bold, 비클릭).
/// 깊은 경로는 가운데를 `…` 로 접는다 — root + `…` + 마지막 두 성분(디자인 `FpCrumbs elide`).
fn crumbs(ui: &mut egui::Ui, theme: &Theme, v: Variant) {
    const REMOTE_CRUMBS: &[Crumb] = &[
        Crumb {
            label: HOST,
            root: true,
            current: false,
        },
        crumb("home"),
        crumb("deploy"),
        Crumb {
            label: "agents-prod",
            root: false,
            current: true,
        },
    ];
    const LOCAL_CRUMBS: &[Crumb] = &[
        Crumb {
            label: "/",
            root: true,
            current: false,
        },
        crumb("Users"),
        crumb("maya"),
        Crumb {
            label: "projects",
            root: false,
            current: true,
        },
    ];
    // 디자인 seed 1:1 (overlays-shared.jsx `deepTail`).
    const DEEP_CRUMBS: &[Crumb] = &[
        Crumb {
            label: "/",
            root: true,
            current: false,
        },
        crumb("Users"),
        crumb("maya"),
        crumb("tasty"),
        crumb("config"),
        crumb("keybindings"),
        crumb("exports"),
        Crumb {
            label: "2026-09",
            root: false,
            current: true,
        },
    ];
    const ELLIPSIS: Crumb = crumb("…");
    let items = if v.remote {
        REMOTE_CRUMBS
    } else if v.deep {
        DEEP_CRUMBS
    } else {
        LOCAL_CRUMBS
    };
    let shown: Vec<&Crumb> = if v.deep && items.len() > 3 {
        let mut out = vec![&items[0], &ELLIPSIS];
        out.extend(&items[items.len() - 2..]);
        out
    } else {
        items.iter().collect()
    };
    ui.spacing_mut().item_spacing.x = STRUCT_GAP_2.value();
    for (i, it) in shown.iter().enumerate() {
        if i > 0 {
            kit::icon(
                ui,
                icons::CHEVRON_RIGHT,
                CRUMB_GLYPH,
                theme.text_disabled().to_egui(),
            );
        }
        let color = if it.current {
            theme.text_primary()
        } else {
            theme.accent_primary()
        };
        let font = if it.root {
            egui::FontId::monospace(theme.font_size_caption.value())
        } else {
            egui::FontId::proportional(theme.font_size_caption.value())
        };
        let text = elide(ui, it.label, font.clone(), CRUMB_MAX_W);
        let mut rt = egui::RichText::new(text).font(font).color(color.to_egui());
        if it.current {
            rt = rt.strong();
        }
        ui.label(rt);
    }
}
