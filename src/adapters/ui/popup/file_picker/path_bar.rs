//! 파일 피커 path bar — breadcrumb 과 상위·새로고침 버튼. 넘치는 경로를 흡수하는 자리다
//! (`docs/features/native-file-picker/index.md` "긴 경로 — 넘침은 path bar 가 흡수한다").

use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::tokens::STRUCT_GAP_2;
use tasty_ui_widgets::{IconButton, IconButtonVariant, MenuItemVariant, menu_item};

use super::{CRUMB_GLYPH, FilePickerAction, FilePickerProps};
use crate::adapters::ui::icons;
use crate::theme::Theme;

/// 경로 breadcrumb 한 성분의 최대 폭 — 넘치면 말줄임한다(디자인 `FpCrumbs` span
/// `maxWidth:180`). 대응 Theme 토큰이 없는 구조 폭이고, 갤러리 specimen 이 같은 값을 같은
/// 이름으로 갖는다.
const CRUMB_MAX_W: LogicalPx = LogicalPx(180.0);

/// breadcrumb 한 칸 — 실제 성분이거나, 가운데를 접은 `…`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CrumbSlot {
    /// `crumbs[i]`.
    Crumb(usize),
    /// 숨긴 조상들의 인덱스 범위.
    Hidden(std::ops::Range<usize>),
}

/// breadcrumb 에 무엇을 보일지. `elide` 면 root + `…` + 마지막 두 성분(현재 폴더와 그
/// 부모 — 위치를 알려주는 둘이다). 접을 것이 없는 짧은 경로는 그대로 둔다.
pub(super) fn crumb_slots(len: usize, elide: bool) -> Vec<CrumbSlot> {
    if elide && len > 3 {
        vec![
            CrumbSlot::Crumb(0),
            CrumbSlot::Hidden(1..len - 2),
            CrumbSlot::Crumb(len - 2),
            CrumbSlot::Crumb(len - 1),
        ]
    } else {
        (0..len).map(CrumbSlot::Crumb).collect()
    }
}

/// path bar — 오른쪽 버튼(flex:none)이 먼저 자리 잡고, breadcrumb 이 남은 폭(flex:1;
/// min-width:0; overflow:hidden) 안에서만 그린다.
pub(super) fn path_bar(
    ui: &mut egui::Ui,
    props: &FilePickerProps<'_>,
    action: &mut FilePickerAction,
) {
    let th = props.theme;
    let (row, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), th.item_height_interactive.value()),
        egui::Sense::hover(),
    );
    let mut buttons = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("file_picker_path_buttons")
            .max_rect(row)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    buttons.spacing_mut().item_spacing.x = STRUCT_GAP_2.value();
    if IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .show(&mut buttons, th, &|ui, rect, c| {
            icons::REFRESH.image(rect.height(), c).paint_at(ui, rect)
        })
        .clicked()
    {
        *action = FilePickerAction::Refresh;
    }
    if IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .show(&mut buttons, th, &|ui, rect, c| {
            icons::CHEVRON_UP.image(rect.height(), c).paint_at(ui, rect)
        })
        .clicked()
    {
        *action = FilePickerAction::NavigateUp;
    }
    let crumbs_right = (buttons.min_rect().left() - th.spacing_sm.value()).max(row.left());
    let crumbs_rect = egui::Rect::from_min_max(row.min, egui::pos2(crumbs_right, row.bottom()));
    let mut crumbs_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt("file_picker_crumbs")
            .max_rect(crumbs_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    crumbs_ui.set_clip_rect(crumbs_rect.intersect(ui.clip_rect()));
    draw_crumbs(&mut crumbs_ui, props, action);
}

/// 크럼 라벨 하나를 `CRUMB_MAX_W` 에서 말줄임한 galley.
fn crumb_galley(
    ui: &egui::Ui,
    th: &Theme,
    label: &str,
    root: bool,
    current: bool,
) -> std::sync::Arc<egui::Galley> {
    let size = th.font_size_caption.value();
    let font = if root {
        egui::FontId::monospace(size)
    } else {
        egui::FontId::proportional(size)
    };
    let color = if current {
        th.text_primary()
    } else {
        th.accent_primary()
    };
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_owned(), font, color.into());
    job.wrap = egui::text::TextWrapping::truncate_at_width(CRUMB_MAX_W.value());
    ui.fonts(|f| f.layout_job(job))
}

fn draw_crumbs(ui: &mut egui::Ui, props: &FilePickerProps<'_>, action: &mut FilePickerAction) {
    let th = props.theme;
    let gap = STRUCT_GAP_2.value();
    ui.spacing_mut().item_spacing.x = gap;
    let n = props.crumbs.len();
    let galleys: Vec<_> = props
        .crumbs
        .iter()
        .enumerate()
        .map(|(i, c)| crumb_galley(ui, th, &c.label, i == 0, i + 1 == n))
        .collect();
    let separator_w = CRUMB_GLYPH.value() + gap * 2.0;
    let full_w: f32 =
        galleys.iter().map(|g| g.size().x).sum::<f32>() + separator_w * n.saturating_sub(1) as f32;
    // 다 들어가면 전부 보이고, 넘치면 가운데를 접는다(디자인 "deep paths elide in the middle").
    let elide = full_w > ui.available_width();

    for (slot_idx, slot) in crumb_slots(n, elide).into_iter().enumerate() {
        if slot_idx > 0 {
            ui.add(icons::CHEVRON_RIGHT.image(CRUMB_GLYPH.value(), th.text_disabled().into()));
        }
        match slot {
            CrumbSlot::Crumb(i) => {
                let is_current = i + 1 == n;
                let galley = galleys[i].clone();
                let sense = if is_current {
                    egui::Sense::hover()
                } else {
                    egui::Sense::click()
                };
                let (rect, resp) = ui.allocate_exact_size(galley.size(), sense);
                let color = if is_current {
                    th.text_primary()
                } else {
                    th.accent_primary()
                };
                ui.painter().galley(rect.min, galley, color.into());
                if !is_current {
                    if resp.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if resp.clicked() {
                        *action = FilePickerAction::NavigateTo(i);
                    }
                }
            }
            CrumbSlot::Hidden(range) => hidden_crumbs(ui, props, range, action),
        }
    }
}

/// `…` 크럼 — 클릭하면 숨긴 조상들을 메뉴로 나열한다.
fn hidden_crumbs(
    ui: &mut egui::Ui,
    props: &FilePickerProps<'_>,
    range: std::ops::Range<usize>,
    action: &mut FilePickerAction,
) {
    let th = props.theme;
    let galley = crumb_galley(ui, th, "…", false, false);
    let (rect, resp) = ui.allocate_exact_size(galley.size(), egui::Sense::click());
    ui.painter()
        .galley(rect.min, galley, th.accent_primary().into());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let resp = resp.on_hover_text(
        props
            .hidden_folders_label
            .replace("{}", &range.len().to_string()),
    );
    let popup_id = ui.make_persistent_id("file_picker_hidden_crumbs");
    if resp.clicked() {
        ui.memory_mut(|m| m.toggle_popup(popup_id));
    }
    egui::popup_below_widget(
        ui,
        popup_id,
        &resp,
        egui::PopupCloseBehavior::CloseOnClick,
        |ui| {
            // 메뉴 폭 = 가장 긴 조상 라벨 + 행 좌우 패딩(측정값 — 새 폭을 정하지 않는다).
            let font = egui::FontId::proportional(th.font_size_body.value());
            let widest = props.crumbs[range.clone()]
                .iter()
                .map(|c| {
                    ui.painter()
                        .layout_no_wrap(c.label.clone(), font.clone(), egui::Color32::PLACEHOLDER)
                        .size()
                        .x
                })
                .fold(0.0_f32, f32::max);
            ui.set_min_width(widest + th.menu_item_padding_x().value() * 2.0);
            for i in range.clone() {
                if menu_item(
                    ui,
                    th,
                    None,
                    &props.crumbs[i].label,
                    None,
                    MenuItemVariant::Normal,
                    false,
                    true,
                )
                .clicked()
                {
                    *action = FilePickerAction::NavigateTo(i);
                }
            }
        },
    );
}
