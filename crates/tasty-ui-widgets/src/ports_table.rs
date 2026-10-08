//! 포트 스캐너 표의 열 정의와 표 꾸밈. 본체 popup 과 갤러리 예제가 같은 정의로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

use crate::chip::{TagVariant, tag, tag_width};
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

    /// 열이 줄어들 수 있는 하한. 고정 열은 이 폭을 그대로 쓴다. Address·Process만 해당 토큰이
    /// 있다. 나머지는 이 표 전용 값이며 같은 숫자의 다른 역할 토큰으로 대체하지 않는다.
    pub fn floor(self, theme: &Theme) -> LogicalPx {
        match self {
            PortsColumn::Port => LogicalPx(84.0),
            PortsColumn::Proto => LogicalPx(76.0),
            PortsColumn::Address => theme.port_addr_col_min_width(),
            PortsColumn::Process => theme.port_process_col_min_width(),
            PortsColumn::Workspace => LogicalPx(120.0),
            PortsColumn::Tab => LogicalPx(80.0),
            PortsColumn::State => LogicalPx(140.0),
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
/// 말줄임하고 Tag 는 그대로 보인다. PID 가 있으면 Tag 의 응답을 돌려준다.
pub fn ports_process_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    name: &str,
    pid: Option<&str>,
) -> Option<egui::Response> {
    ui.horizontal(|ui| {
        let gap = theme.spacing_sm.value();
        ui.spacing_mut().item_spacing.x = gap;
        let tag_reserve = pid.map_or(0.0, |p| tag_width(ui, theme, p) + gap);
        let name_max = (ui.available_width() - tag_reserve).max(0.0);
        ui.scope(|ui| {
            ui.set_max_width(name_max);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(name)
                        .color(theme.text_primary().to_egui())
                        .size(theme.font_size_body.value()),
                )
                .truncate(),
            );
        });
        pid.map(|p| tag(ui, theme, p, TagVariant::Default, false))
    })
    .inner
}
