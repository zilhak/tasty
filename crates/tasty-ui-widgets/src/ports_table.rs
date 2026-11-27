//! 포트 스캐너 표의 열 정의와 표 꾸밈. 본체 popup 과 갤러리 예제가 같은 정의로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::chip::TagVariant;
use crate::status_dot::{StatusKind, status_dot};
use crate::table::{Table, TableAlign, TableColumn, TableColumnWidth};

/// 포트 표의 일곱 열. 별 열은 항상 맨 앞에 있으며 이 목록에 넣지 않는다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PortsColumn {
    Port,
    Proto,
    Address,
    Process,
    Workspace,
    Tab,
    State,
}

impl PortsColumn {
    /// 왼쪽에서 오른쪽 순서.
    pub const ALL: [PortsColumn; 7] = [
        PortsColumn::Port,
        PortsColumn::Proto,
        PortsColumn::Address,
        PortsColumn::Process,
        PortsColumn::Workspace,
        PortsColumn::Tab,
        PortsColumn::State,
    ];

    /// 열이 줄어들 수 있는 하한. 고정 열은 이 폭을 그대로 쓴다. 열마다 이 표 전용 토큰이 있다.
    pub fn floor(self, theme: &Theme) -> LogicalPx {
        match self {
            PortsColumn::Port => theme.port_port_col_width(),
            PortsColumn::Proto => theme.port_proto_col_width(),
            PortsColumn::Address => theme.port_addr_col_min_width(),
            PortsColumn::Process => theme.port_process_col_min_width(),
            PortsColumn::Workspace => theme.port_workspace_col_width(),
            PortsColumn::Tab => theme.port_tab_col_width(),
            PortsColumn::State => theme.port_state_col_width(),
        }
    }

    /// 남는 폭을 받는 열인지. Process만 하한에서 늘어나며 남는 폭을 모두 받는다. Address는
    /// 하한에 고정된다.
    pub fn flex(self) -> bool {
        matches!(self, PortsColumn::Process)
    }

    /// 공용 Table 열 폭. 가변 열은 하한을 `Flex` 최소 폭으로, 고정 열은 하한을 `Exact` 폭으로 둔다.
    pub fn width(self, theme: &Theme) -> TableColumnWidth {
        if self.flex() {
            TableColumnWidth::Flex {
                min_width: self.floor(theme),
            }
        } else {
            TableColumnWidth::Exact(self.floor(theme))
        }
    }

    /// 셀 가로 정렬. Port만 오른쪽 정렬이다.
    pub fn align(self) -> TableAlign {
        match self {
            PortsColumn::Port => TableAlign::Right,
            _ => TableAlign::Left,
        }
    }
}

/// 말줄임 Label 의 응답. 잘렸을 때 egui 가 스스로 붙이는 전체 글자 tooltip 은 붙이지 않았다. 그 tooltip 을 같은
/// 레이어 팝업이 열린 동안 숨길지는 호출부가 정하므로, 호출부가 [`ElidedLabel::on_elided`] 로 붙인다.
pub struct ElidedLabel {
    pub response: egui::Response,
    /// 잘렸으면 전체 글자.
    pub full: Option<String>,
}

impl ElidedLabel {
    /// 잘렸을 때만 `attach(응답, 전체 글자)` 를 부른다. 판정이 필요 없으면 `egui::Response::on_hover_text` 를 넘긴다.
    pub fn on_elided<R>(self, attach: impl FnOnce(egui::Response, String) -> R) -> Option<R> {
        self.full.map(|full| attach(self.response, full))
    }
}

/// `egui::Label::new(text).truncate()` 와 같게 놓고 그리되, 잘렸을 때 egui 가 붙이는 전체 글자 tooltip 은 붙이지
/// 않는다(egui 0.31 Label 에는 이 tooltip 을 끄는 설정이 없다). 그리기는 egui `Label::ui` 의 비대화형 경로와 같다.
fn truncated_label(ui: &mut egui::Ui, text: egui::RichText) -> ElidedLabel {
    let (pos, galley, response) = egui::Label::new(text).truncate().layout_in_ui(ui);
    let info =
        || egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), galley.text());
    response.widget_info(info);
    let full = galley.elided.then(|| galley.text().to_owned());
    if ui.is_rect_visible(response.rect) {
        let color = ui.style().visuals.text_color();
        let underline = if response.has_focus() || response.highlighted() {
            egui::Stroke::new(1.0, color)
        } else {
            egui::Stroke::NONE
        };
        if ui.style().interaction.selectable_labels {
            egui::text_selection::LabelSelectionState::label_text_selection(
                ui, &response, pos, galley, color, underline,
            );
        } else {
            let shape = egui::epaint::TextShape::new(pos, galley, color).with_underline(underline);
            ui.painter().add(shape);
        }
    }
    ElidedLabel { response, full }
}

/// 즐겨찾기 행에서 주소 뒤에 오는 상세와 상태 칸. 행의 남은 폭을 모두 차지한다.
/// 상세는 주소에서 `spacing_md` 떨어져 왼쪽에 붙고 넘치면 말줄임한다. 상태 점과 라벨은 행
/// 오른쪽 끝의 최소 폭(`port_favorites_state_min_width`) 칸 안에서 오른쪽에 붙으므로 상세가 길어도 점에
/// 닿지 않는다.
/// 상세 라벨(잘렸는지 포함, [`ElidedLabel`])과 상태 점의 응답을 차례로 돌려준다.
pub fn ports_favorite_detail_and_state(
    ui: &mut egui::Ui,
    theme: &Theme,
    detail: &str,
    kind: StatusKind,
    state: &str,
    pulse: bool,
    reduced_motion: bool,
) -> (ElidedLabel, egui::Response) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(
            ui.available_width(),
            theme.port_favorites_row_height().value(),
        ),
        egui::Sense::hover(),
    );
    let state_left =
        (rect.right() - theme.port_favorites_state_min_width().value()).max(rect.left());
    let detail_left = (rect.left() + theme.spacing_md.value()).min(state_left);
    let detail_rect = egui::Rect::from_x_y_ranges(detail_left..=state_left, rect.y_range());
    let state_rect = egui::Rect::from_x_y_ranges(state_left..=rect.right(), rect.y_range());

    let mut detail_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(detail_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    let detail = egui::RichText::new(detail)
        .color(theme.text_muted().to_egui())
        .size(theme.font_size_caption.value());
    let detail = truncated_label(&mut detail_ui, detail);

    let mut state_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(state_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    let state = status_dot(
        &mut state_ui,
        theme,
        kind,
        state,
        pulse,
        reduced_motion,
        crate::Ground::Panel,
    );
    (detail, state)
}

/// 맨 앞 별 열의 폭.
pub fn ports_star_column_width(theme: &Theme) -> TableColumnWidth {
    TableColumnWidth::Exact(theme.port_star_col_width())
}

/// 포트 표의 공용 Table. 행 선택, 가로 스크롤, 헤더 배경과 헤더 왼쪽 여백을 정해 둔다.
/// 열의 하한 합이 표 폭보다 크면 열을 줄이거나 숨기지 않고 본문을 가로로 스크롤한다.
pub fn ports_table<'a, K>(columns: Vec<TableColumn<'a, K>>, theme: &Theme) -> Table<'a, K> {
    Table::new(columns)
        .selectable(true)
        .horizontal_scroll(true)
        .header_fill(theme.bg_sidebar().to_egui())
        .header_pad_x(theme.table_cell_padding_x())
}

/// Process 셀: 프로세스 이름과 PID Tag. 이름과 Tag 사이는 `spacing_sm` 이다. 셀이 좁으면 이름만
/// 말줄임하고 Tag 는 그대로 보인다. 이름 라벨([`ElidedLabel`])과, PID 가 있으면 Tag 의 응답을 돌려준다.
pub fn ports_process_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    pid: Option<&str>,
) -> (ElidedLabel, Option<egui::Response>) {
    ui.horizontal(|ui| {
        let gap = theme.spacing_sm.value();
        ui.spacing_mut().item_spacing.x = gap;
        let tag_reserve = pid.map_or(0.0, |p| crate::tag_mono_width(ui, theme, p) + gap);
        let name_max = (ui.available_width() - tag_reserve).max(0.0);
        let name_fg = crate::table_cell_ink(ui, theme, theme.text_primary());
        let name = egui::RichText::new(name)
            .color(name_fg)
            .size(theme.font_size_body.value());
        let name = ui
            .scope(|ui| {
                ui.set_max_width(name_max);
                truncated_label(ui, name)
            })
            .inner;
        let tag = pid.map(|p| crate::tag_mono(ui, theme, p, TagVariant::Default, false));
        (name, tag)
    })
    .inner
}

#[cfg(test)]
mod favorite_row_tests {
    use super::*;

    fn theme(zoom: f32) -> Theme {
        Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, zoom)
    }

    /// 행 폭 `width` 안에서 주소 다음 자리부터 상세와 상태를 그리고 (행, 상세, 상태) 사각형을 돌려준다.
    fn draw(detail: &str, width: f32) -> (egui::Rect, egui::Rect, egui::Rect) {
        let theme = theme(1.0);
        let ctx = egui::Context::default();
        let mut out = None;
        for _ in 0..2 {
            drop(ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, theme.port_favorites_row_height().value()),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            let row_left = ui.cursor().left();
                            let (d, s) = ports_favorite_detail_and_state(
                                ui,
                                &theme,
                                detail,
                                StatusKind::Running,
                                "LISTEN",
                                false,
                                true,
                            );
                            let row = egui::Rect::from_x_y_ranges(
                                row_left..=row_left + width,
                                d.response.rect.y_range(),
                            );
                            out = Some((row, d.response.rect, s.rect));
                        },
                    );
                });
            }));
        }
        out.expect("drawn")
    }

    #[test]
    fn a_short_detail_starts_one_spacing_md_after_the_address() {
        let theme = theme(1.0);
        let (row, detail, state) = draw("node · 48213", 480.0);
        assert_eq!(detail.left(), row.left() + theme.spacing_md.value());
        assert_eq!(state.right(), row.right());
        let gap = state.left() - detail.right();
        assert!(
            gap >= theme.spacing_md.value(),
            "detail {detail:?} sits {gap}px from the state {state:?}"
        );
    }

    #[test]
    fn a_long_detail_ellipsizes_before_the_state_column() {
        let (row, detail, state) = draw(
            "node · 48213 · a workspace name long enough to run past the state column",
            320.0,
        );
        let state_left = row.right() - theme(1.0).port_favorites_state_min_width().value();
        assert!(
            detail.right() <= state_left,
            "detail {detail:?} runs into the state column at {state_left}"
        );
        assert!(
            state.left() >= state_left,
            "state {state:?} left of {state_left}"
        );
        assert_eq!(state.right(), row.right());
    }

    /// 상세가 상태 칸 하한 너머까지 긴 행을 그린 뒤 (행 오른쪽 끝, 그린 상세 글자의 오른쪽 끝)을 돌려준다.
    fn painted_long_detail(width: f32) -> (f32, f32) {
        let theme = theme(1.0);
        let detail = "node · 48213 · a workspace name long enough to run past the state column";
        let ctx = egui::Context::default();
        let mut row_right = 0.0;
        let mut text_right = None;
        for _ in 0..2 {
            let out = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, theme.port_favorites_row_height().value()),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            row_right = ui.cursor().left() + width;
                            ports_favorite_detail_and_state(
                                ui,
                                &theme,
                                detail,
                                StatusKind::Running,
                                "LISTEN",
                                false,
                                true,
                            );
                        },
                    );
                });
            });
            text_right = out.shapes.iter().find_map(|c| match &c.shape {
                egui::Shape::Text(t) if t.galley.job.text.starts_with("node") => {
                    Some(t.pos.x + t.galley.rect.width())
                }
                _ => None,
            });
        }
        (row_right, text_right.expect("the detail text is painted"))
    }

    /// 상세가 길면 그린 상세 글자는 상태 칸 하한(`port_favorites_state_min_width`, 배율 1에서 112) 왼쪽 경계에서
    /// 말줄임으로 끝난다. 경계를 넘지 않고, 말줄임이 남기는 한 글자 폭보다 더 앞에서 끝나지도 않는다.
    #[test]
    fn a_long_detail_ends_at_the_state_column_floor() {
        let theme = theme(1.0);
        let (row_right, text_right) = painted_long_detail(320.0);
        let floor_left = row_right - theme.port_favorites_state_min_width().value();
        assert!(
            text_right <= floor_left,
            "상세 글자 오른쪽 끝 {text_right} 이 상태 칸 하한 왼쪽 경계 {floor_left} 를 넘는다"
        );
        assert!(
            text_right > floor_left - theme.font_size_caption.value(),
            "상세 글자 오른쪽 끝 {text_right} 이 상태 칸 하한 왼쪽 경계 {floor_left} 보다 한 글자 넘게 앞에서 끝난다"
        );
    }

    #[test]
    fn the_state_column_floor_scales_with_ui_zoom() {
        assert_eq!(theme(1.0).port_favorites_state_min_width().value(), 112.0);
        assert_eq!(theme(1.2).port_favorites_state_min_width().value(), 134.0);
    }
}

#[cfg(test)]
mod elided_label_tests {
    use super::*;

    const LONG: &str = "a process name long enough to be cut in a narrow cell";

    /// 폭 `width` 칸에 프로세스 이름 칸을 그리고, 이름 칸 위에 포인터를 둔 뒤 전체 글자 tooltip 이 그려지는지
    /// 본다. `attach` 면 잘린 이름에 호출부가 `on_hover_text` 를 붙인다(갤러리 꼴). 돌려주는 값은
    /// (잘렸을 때의 전체 글자, tooltip 이 그려졌는가).
    fn hover_cell(name: &str, width: f32, attach: bool) -> (Option<String>, bool) {
        let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
        let ctx = egui::Context::default();
        let mut time = 0.0;
        // 한 프레임을 그리고 (이름 글자 도형 수, 이름 칸 영역, 잘렸을 때의 전체 글자)를 돌려준다.
        let mut frame = |events: Vec<egui::Event>| {
            time += 0.02;
            let input = egui::RawInput {
                events,
                time: Some(time),
                ..Default::default()
            };
            let mut cell = (egui::Rect::NOTHING, None);
            let out = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(width, 40.0), |ui| {
                        let (label, _) = ports_process_cell(ui, &theme, name, Some("42"));
                        cell = (label.response.rect, label.full.clone());
                        if attach {
                            label.on_elided(egui::Response::on_hover_text);
                        }
                    });
                });
            });
            let texts = out
                .shapes
                .iter()
                .filter(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.text() == name))
                .count();
            (texts, cell.0, cell.1)
        };
        frame(Vec::new());
        let (before, rect, full) = frame(Vec::new());
        // egui 는 짧은 간격의 위치 기록으로 포인터 이동 시각을 정하므로 두 걸음으로 옮긴다.
        frame(vec![egui::Event::PointerMoved(
            rect.center() + egui::vec2(3.0, 0.0),
        )]);
        frame(vec![egui::Event::PointerMoved(rect.center())]);
        let shown = (0..75).any(|_| frame(Vec::new()).0 > before);
        (full, shown)
    }

    #[test]
    fn a_cut_name_reports_its_full_text_and_draws_no_tooltip_of_its_own() {
        let (full, shown) = hover_cell(LONG, 120.0, false);
        assert_eq!(
            full.as_deref(),
            Some(LONG),
            "좁은 칸에서 이름이 잘렸다고 알리지 않는다"
        );
        assert!(
            !shown,
            "위젯이 잘린 이름에 egui 의 전체 글자 tooltip 을 스스로 그렸다"
        );
    }

    #[test]
    fn the_caller_attaching_the_full_text_shows_it_on_hover() {
        assert_eq!(
            hover_cell(LONG, 120.0, true),
            (Some(LONG.to_owned()), true),
            "호출부가 붙인 전체 글자 tooltip 이 hover 에 보이지 않는다"
        );
        // 대조: 잘리지 않은 이름은 전체 글자가 없고 tooltip 도 없다.
        assert_eq!(hover_cell("node", 480.0, true), (None, false));
    }
}
