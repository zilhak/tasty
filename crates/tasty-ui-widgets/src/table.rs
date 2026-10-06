//! 컬럼·고정 헤더·선택·스크롤을 제공하는 공용 표. 셀 내용은 호출자가 그린다.
//! 행 선택을 켜면 본문 라벨의 텍스트 선택을 꺼서 글자 위 클릭도 행에 전달한다.
//! 헤더의 정렬 클릭은 유지하며 행 선택을 끈 표는 egui의 텍스트 선택 설정을 따른다.

use egui_extras::{Column, TableBuilder};
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;

/// 컬럼 폭 (egui_extras [`Column`] 매핑).
#[derive(Clone, Copy)]
pub enum TableColumnWidth {
    /// 고정 폭 (리사이즈 불가).
    Exact(LogicalPx),
    /// 초기 폭 + 최소 폭.
    Initial {
        initial: LogicalPx,
        at_least: LogicalPx,
    },
    /// 남은 폭 균등 분배. `at_least` 최소폭(없으면 0.0), `clip` true 면 말줄임.
    Remainder { at_least: LogicalPx, clip: bool },
}

/// 셀 가로 정렬.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TableAlign {
    Left,
    Right,
}

/// 정렬 방향 (헤더 인디케이터 ▲ Asc / ▼ Desc).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TableSortDir {
    Asc,
    Desc,
}

/// 컬럼 정의: 제목·폭·정렬·정렬키.
pub struct TableColumn<'a, K> {
    /// 헤더 제목.
    pub title: &'a str,
    /// 컬럼 폭.
    pub width: TableColumnWidth,
    /// 헤더·(Right 시) 본문 셀 가로 정렬.
    pub align: TableAlign,
    /// `Some(key)` → 정렬 가능(active 시 ▲▼, 클릭 시 key 반환). `None` → 정적 헤더.
    pub sort_id: Option<K>,
}

/// 표 상호작용 결과.
pub struct TableOutput<K> {
    /// 정렬 가능 헤더가 클릭되면 그 컬럼의 정렬 키.
    pub clicked_sort: Option<K>,
    /// 본문 행이 클릭되면 그 행의 인덱스(`rows` 기준).
    pub clicked_row: Option<usize>,
    /// 본문 행이 우클릭(secondary)되면 그 행의 인덱스 — 컨텍스트 메뉴용.
    pub secondary_clicked_row: Option<usize>,
}

/// 공용 Table 빌더.
pub struct Table<'a, K> {
    columns: Vec<TableColumn<'a, K>>,
    active_sort: Option<(K, TableSortDir)>,
    header_fill: Option<egui::Color32>,
    header_pad_x: LogicalPx,
    header_pad_right: LogicalPx,
    header_height: Option<LogicalPx>,
    row_height: Option<LogicalPx>,
    max_scroll_height: Option<LogicalPx>,
    id_salt: Option<egui::Id>,
    selectable: bool,
    striped: bool,
    horizontal_scroll: bool,
}

impl<'a, K> Table<'a, K> {
    /// 컬럼 정의 목록으로 표를 만든다.
    pub fn new(columns: Vec<TableColumn<'a, K>>) -> Self {
        Self {
            columns,
            active_sort: None,
            header_fill: None,
            header_pad_x: LogicalPx(0.0),
            header_pad_right: LogicalPx(0.0),
            header_height: None,
            row_height: None,
            max_scroll_height: None,
            id_salt: None,
            selectable: false,
            striped: false,
            horizontal_scroll: false,
        }
    }

    /// 현재 활성 정렬 상태(컬럼 키 + 방향). 해당 컬럼 헤더에 ▲/▼ 를 그린다.
    pub fn active_sort(mut self, key: K, dir: TableSortDir) -> Self {
        self.active_sort = Some((key, dir));
        self
    }

    /// 여러 표가 함께 있을 때 각 표의 위젯·스크롤 ID를 구분한다. 미지정이면 부모 ID를 사용한다.
    pub fn id_salt(mut self, salt: impl std::hash::Hash) -> Self {
        self.id_salt = Some(egui::Id::new(salt));
        self
    }

    /// sticky 헤더 배경색. 미지정 시 칠하지 않는다(투명).
    pub fn header_fill(mut self, fill: egui::Color32) -> Self {
        self.header_fill = Some(fill);
        self
    }

    /// 헤더 셀 좌측 패딩(디자인 th padding-x). 기본 0.
    pub fn header_pad_x(mut self, pad: LogicalPx) -> Self {
        self.header_pad_x = pad;
        self
    }

    /// `TableAlign::Right` 헤더 셀의 오른쪽 여백. 본문 셀이 오른쪽에 같은 여백을 두는 열에서
    /// 제목 끝을 값 끝과 맞춘다. 왼쪽 정렬 헤더에는 적용하지 않는다. 기본 0.
    pub fn header_pad_right(mut self, pad: LogicalPx) -> Self {
        self.header_pad_right = pad;
        self
    }

    /// 헤더 행 높이. 미지정 시 `table_cell_height`.
    pub fn header_height(mut self, h: LogicalPx) -> Self {
        self.header_height = Some(h);
        self
    }

    /// 본문 행 높이. 미지정 시 `table_cell_height`.
    pub fn row_height(mut self, h: LogicalPx) -> Self {
        self.row_height = Some(h);
        self
    }

    /// 내부 ScrollArea 최대 높이(이 높이를 넘으면 본문이 스크롤된다).
    pub fn max_scroll_height(mut self, h: LogicalPx) -> Self {
        self.max_scroll_height = Some(h);
        self
    }

    /// 행 전체 클릭 선택 활성화.
    pub fn selectable(mut self, on: bool) -> Self {
        self.selectable = on;
        self
    }

    /// 행 줄무늬(zebra) 배경.
    pub fn striped(mut self, on: bool) -> Self {
        self.striped = on;
        self
    }

    /// 헤더와 본문을 함께 가로로 스크롤한다. 세로 스크롤에서는 헤더를 고정한다.
    /// Remainder가 스크롤 안에서 폭을 늘릴 수 있으므로 이 모드는 Exact 컬럼만 사용해야 한다.
    pub fn horizontal_scroll(mut self, on: bool) -> Self {
        self.horizontal_scroll = on;
        self
    }

    /// 표를 그린다.
    ///
    /// - `rows`: 본문 행 데이터.
    /// - `is_selected`: 해당 행이 선택 상태인지(선택 하이라이트).
    /// - `cell`: `(ui, theme, row, col_index)` 로 셀 1칸을 렌더.
    pub fn show<Row>(
        self,
        ui: &mut egui::Ui,
        theme: &Theme,
        rows: &[Row],
        is_selected: impl Fn(&Row) -> bool,
        mut cell: impl FnMut(&mut egui::Ui, &Theme, &Row, usize),
    ) -> TableOutput<K>
    where
        K: Copy + PartialEq,
    {
        // 헤더와 본문 행은 같은 `table-cell-height` 역할을 쓴다.
        let cell_h = theme.table_cell_height();
        let header_h = self.header_height.unwrap_or(cell_h);
        let row_h = self.row_height.unwrap_or(cell_h);

        let mut clicked_sort: Option<K> = None;
        let mut clicked_row: Option<usize> = None;
        let mut secondary_clicked_row: Option<usize> = None;

        let columns = &self.columns;
        let active_sort = self.active_sort;
        let header_pad_x = self.header_pad_x;
        let header_pad_right = self.header_pad_right;
        let selectable = self.selectable;
        let striped = self.striped;
        let max_scroll_height = self.max_scroll_height;
        let header_fill = self.header_fill;
        let horizontal_scroll = self.horizontal_scroll;
        // 헤더 배경이 가로 스크롤의 전체 콘텐츠 폭을 덮도록 미리 계산한다.
        let total_w = fixed_total_width(columns, LogicalPx(ui.spacing().item_spacing.x));

        let mut draw_core = |ui: &mut egui::Ui, band_w: LogicalPx| {
            // 셀 배경 API 대신 헤더 배경을 직접 그린다.
            if let Some(fill) = header_fill {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(ui.max_rect().left(), ui.cursor().top()),
                    egui::vec2(band_w.value(), header_h.value()),
                );
                ui.painter().rect_filled(rect, 0.0, fill);
            }

            let mut builder = TableBuilder::new(ui)
                .striped(striped)
                .resizable(false)
                // 행·텍스트 선택을 드래그 스크롤로 오인하지 않도록 패닝을 끈다.
                .drag_to_scroll(false)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
            if selectable {
                builder = builder.sense(egui::Sense::click());
            }
            if let Some(ms) = max_scroll_height {
                builder = builder.max_scroll_height(ms.value());
            }
            for col in columns {
                builder = builder.column(to_column(col.width));
            }

            let mut table = builder.header(header_h.value(), |mut header| {
                for col in columns {
                    header.col(|ui| {
                        if header_cell(ui, theme, col, active_sort, header_pad_x, header_pad_right)
                        {
                            clicked_sort = col.sort_id;
                        }
                    });
                }
            });
            // egui_extras 는 선택 행 배경을 텍스트 선택색으로, hover 행 배경을 위젯 hover 색으로 칠한다.
            // 행에는 `table-row-bg-selected`·`table-row-bg-hover`를 쓰고, 셀 안의 텍스트 선택과
            // 위젯 hover 는 원래 색을 유지한다.
            let text_selection_fill = table.ui_mut().visuals().selection.bg_fill;
            let widget_hover_fill = table.ui_mut().visuals().widgets.hovered.bg_fill;
            let row_visuals = table.ui_mut().visuals_mut();
            row_visuals.selection.bg_fill = theme.table_row_bg_selected().into();
            row_visuals.widgets.hovered.bg_fill =
                theme.table_row_bg_hover().to_egui_premultiplied();
            table.body(|mut body| {
                for (i, row) in rows.iter().enumerate() {
                    body.row(row_h.value(), |mut tr| {
                        tr.set_selected(is_selected(row));
                        for (c, col) in columns.iter().enumerate() {
                            tr.col(|ui| {
                                ui.visuals_mut().selection.bg_fill = text_selection_fill;
                                ui.visuals_mut().widgets.hovered.bg_fill = widget_hover_fill;
                                // 본문 라벨이 행 클릭을 가로채지 않게 한다. 헤더의 정렬 클릭에는 적용하지 않는다.
                                if selectable {
                                    ui.style_mut().interaction.selectable_labels = false;
                                }
                                match col.align {
                                    TableAlign::Left => cell(ui, theme, row, c),
                                    TableAlign::Right => {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| cell(ui, theme, row, c),
                                        );
                                    }
                                }
                            });
                        }
                        let row_resp = tr.response();
                        if row_resp.clicked() {
                            clicked_row = Some(i);
                        }
                        if row_resp.secondary_clicked() {
                            secondary_clicked_row = Some(i);
                        }
                    });
                }
            });
        };

        let mut run = |ui: &mut egui::Ui| {
            // egui_extras 는 행 사이에 부모의 세로 item_spacing 을 두고 선택·줄무늬 배경을 그 간격까지
            // 넓힌다. 표 안에서만 간격을 0으로 두어 지정한 높이가 곧 보이는 행 높이가 되게 한다.
            // 표 뒤의 위젯에는 scope 밖에서 부모의 간격이 그대로 적용된다.
            ui.scope(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                if horizontal_scroll {
                    // 헤더와 행을 같은 가로 스크롤 안에 놓는다.
                    egui::ScrollArea::horizontal()
                        .auto_shrink([false, true])
                        // 세로와 마찬가지로 가로 드래그 패닝도 끈다.
                        .drag_to_scroll(false)
                        .show(ui, |ui| {
                            ui.set_min_width(total_w.value());
                            let band = total_w.max(LogicalPx(ui.available_width()));
                            draw_core(ui, band);
                        });
                } else {
                    let band = LogicalPx(ui.max_rect().width());
                    draw_core(ui, band);
                }
            });
        };

        match self.id_salt {
            Some(salt) => {
                ui.push_id(salt, run);
            }
            None => run(ui),
        }

        TableOutput {
            clicked_sort,
            clicked_row,
            secondary_clicked_row,
        }
    }
}

/// 헤더 셀 1칸 렌더. 정렬 가능 컬럼이면 클릭 시 `true`.
fn header_cell<K: Copy + PartialEq>(
    ui: &mut egui::Ui,
    theme: &Theme,
    col: &TableColumn<'_, K>,
    active_sort: Option<(K, TableSortDir)>,
    pad_x: LogicalPx,
    pad_right: LogicalPx,
) -> bool {
    let is_active = match (col.sort_id, active_sort) {
        (Some(k), Some((ak, _))) => k == ak,
        _ => false,
    };
    let arrow = if is_active {
        match active_sort.expect("active when is_active").1 {
            TableSortDir::Asc => " ▲",
            TableSortDir::Desc => " ▼",
        }
    } else {
        ""
    };
    // 시안 머리글은 대문자 · `table-header-tracking` 이다.
    let title = col.title.to_uppercase();
    let text = if arrow.is_empty() {
        title
    } else {
        format!("{title}{arrow}")
    };
    let size = theme.table_header_font_size();
    let rich = egui::RichText::new(text)
        .color(if is_active {
            // active 정렬 컬럼 강조색 — 대응 component 토큰 부재로 text_primary() 로 alias.
            egui::Color32::from(theme.text_primary())
        } else {
            egui::Color32::from(theme.table_header_fg())
        })
        .size(size.value())
        .extra_letter_spacing(theme.table_header_tracking(size).value())
        .strong();

    let clickable = col.sort_id.is_some();
    let do_cell = move |ui: &mut egui::Ui| -> bool {
        if pad_x.value() > 0.0 {
            ui.add_space(pad_x.value());
        }
        if clickable {
            ui.add(egui::Label::new(rich).sense(egui::Sense::click()))
                .clicked()
        } else {
            ui.label(rich);
            false
        }
    };

    match col.align {
        TableAlign::Left => do_cell(ui),
        TableAlign::Right => {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if pad_right.value() > 0.0 {
                    ui.add_space(pad_right.value());
                }
                do_cell(ui)
            })
            .inner
        }
    }
}

/// 컬럼 고정폭의 합(+컬럼 사이 item_spacing.x). 가로 스크롤 모드에서 sticky 헤더
/// 띠 폭과 ScrollArea 컨텐츠 최소폭을 잡는 데 쓴다. `Remainder` 는 floor(`at_least`)
/// 기준으로 더한다(스크롤 모드에선 호출자가 `Exact` 만 쓰도록 권장).
fn fixed_total_width<K>(columns: &[TableColumn<'_, K>], spacing_x: LogicalPx) -> LogicalPx {
    let sum = columns
        .iter()
        .map(|c| match c.width {
            TableColumnWidth::Exact(w) => w,
            TableColumnWidth::Initial { initial, .. } => initial,
            TableColumnWidth::Remainder { at_least, .. } => at_least,
        })
        .fold(LogicalPx(0.0), |acc, w| acc + w);
    sum + spacing_x * columns.len().saturating_sub(1) as f32
}

fn to_column(width: TableColumnWidth) -> Column {
    match width {
        TableColumnWidth::Exact(w) => Column::exact(w.value()),
        TableColumnWidth::Initial { initial, at_least } => {
            Column::initial(initial.value()).at_least(at_least.value())
        }
        TableColumnWidth::Remainder { at_least, clip } => {
            Column::remainder().at_least(at_least.value()).clip(clip)
        }
    }
}
