//! Picker rows keep disabled file ink separate from opacity and hit testing.
use super::*;

/// 목록 한 행. 클릭 의도가 있으면 돌려준다.
pub(super) fn entry_row(
    ui: &mut egui::Ui,
    props: &FilePickerProps<'_>,
    entry: &FilePickerEntryView,
) -> Option<FilePickerAction> {
    let th = props.theme;
    let inert = matches!(props.mode, FilePickerMode::Folder { .. }) && !entry.is_dir;
    let selected = !inert && props.selected.iter().any(|s| s == &entry.name);
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), row_height(th).value()),
        if inert {
            egui::Sense::hover()
        } else {
            egui::Sense::click()
        },
    );
    let state = tasty_ui_widgets::RowState::of(
        true,
        selected,
        !inert && resp.hovered(),
        !inert && resp.is_pointer_button_down_on(),
    );
    let parent = tasty_ui_widgets::RowParent::Base;
    state.paint_fill(ui.painter(), rect, 0.0, th);
    if selected {
        let bar = egui::Rect::from_min_size(
            rect.min,
            egui::vec2(th.selection_edge_width.value(), rect.height()),
        );
        ui.painter()
            .rect_filled(bar, 0.0, th.accent_primary_on_active().to_egui());
    }
    if resp.hovered() && !inert {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let (glyph, mut glyph_color) = match entry.is_dir {
        true => (icons::FOLDER, folder_glyph_ink(th, state)),
        false => (icons::FILE, file_glyph_ink(th, state)),
    };
    let c = cols(rect, th);
    let glyph_size = th.icon_glyph_size_md.value();
    let icon_rect = egui::Rect::from_min_size(
        egui::pos2(c.icon_left.value(), rect.center().y - glyph_size * 0.5),
        egui::vec2(glyph_size, glyph_size),
    );
    if inert {
        glyph_color = th.fp_folder_file_fg();
    }
    glyph
        .image(glyph_size, glyph_color.into())
        .paint_at(ui, icon_rect);
    let Cols {
        name_left,
        name_right,
        size_right,
        modified_right,
        ..
    } = c;
    let name_color = if inert {
        th.fp_folder_file_fg().to_egui()
    } else {
        state.label(th, parent, th.text_secondary()).to_egui()
    };
    let mut job = egui::text::LayoutJob::simple_singleline(
        entry.name.clone(),
        egui::FontId::proportional(th.font_size_body.value()),
        name_color,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(
        (name_right - name_left).max(LogicalPx(0.0)).value(),
    );
    let galley = ui.fonts(|f| f.layout_job(job));
    ui.painter().galley(
        egui::pos2(name_left.value(), rect.center().y - galley.size().y * 0.5),
        galley,
        name_color,
    );
    let mono = egui::FontId::monospace(th.font_size_caption.value());
    let meta_fg = if inert {
        th.fp_folder_file_fg()
    } else {
        state.meta(th, parent)
    };
    ui.painter().text(
        egui::pos2(size_right.value(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        &entry.size_display,
        mono.clone(),
        meta_fg.into(),
    );
    ui.painter().text(
        egui::pos2(modified_right.value(), rect.center().y),
        egui::Align2::RIGHT_CENTER,
        &entry.modified_display,
        mono,
        meta_fg.into(),
    );
    if inert {
        if let FilePickerMode::Folder { file_tooltip, .. } = props.mode {
            resp.on_hover_text(file_tooltip);
        }
        return None;
    }
    if resp.double_clicked() {
        Some(if entry.is_dir {
            FilePickerAction::NavigateInto(entry.name.clone())
        } else {
            FilePickerAction::ConfirmEntry(entry.name.clone())
        })
    } else if resp.clicked() {
        Some(FilePickerAction::Select(entry.name.clone()))
    } else {
        None
    }
}

/// 폴더 글리프 잉크. 색이 뜻을 담는 글리프라 행 바탕 칸의 primary 글리프 칸을 읽는다(kit `FpRow`).
fn folder_glyph_ink(
    th: &Theme,
    state: tasty_ui_widgets::RowState,
) -> tasty_type_appearance::color::HexColor {
    use tasty_ui_widgets::RowState as S;
    match state {
        S::SelectedHover | S::SelectedPress => th.accent_primary_on_active(),
        S::Hover | S::Press => th.accent_primary(),
        S::Selected | S::Rest | S::DropTarget | S::Disabled => th.accent_primary_raised(),
    }
}

/// 파일 글리프 잉크. 쉼·선택은 text-muted, hover 는 `fp-row-glyph-fg-hover`, 선택+hover 는 text-on-active 다(kit `FpRow`).
fn file_glyph_ink(
    th: &Theme,
    state: tasty_ui_widgets::RowState,
) -> tasty_type_appearance::color::HexColor {
    use tasty_ui_widgets::RowState as S;
    match state {
        S::SelectedHover | S::SelectedPress => th.text_on_active(),
        S::Hover | S::Press => th.fp_row_glyph_fg_hover(),
        S::Selected | S::Rest | S::DropTarget | S::Disabled => th.text_muted(),
    }
}
