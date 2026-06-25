//! `Table` — 디자인 `components/data/Table` 의 공용 위젯.
//!
//! `egui_extras::TableBuilder` 의 boilerplate(컬럼 정의·sticky 헤더·정렬 인디케이터·
//! 행 선택·내부 스크롤 cap)를 감싸 *선언적* API 로 노출한다. 색·폰트·간격은 모두
//! `Theme` 토큰에서 가져오며 하드코딩하지 않는다.
//!
//! 디자인 계약 (port_scanner 7컬럼 표가 첫 적용 사례):
//! - 헤더: caption 폰트 + strong, active 정렬 컬럼은 `text` 색 + ▲/▼, 비활성은
//!   `subtext0`. 헤더 행은 egui_extras 기본 동작으로 스크롤 시 상단 고정(sticky).
//! - 본문: 행 단위 클릭 선택(`selectable`), 선택 행은 egui `set_selected` 하이라이트.
//! - 컬럼 폭/정렬은 [`TableColumn`] 으로 컬럼마다 지정. 본문 셀 내용은 호출자가
//!   `cell` 클로저로 `(ui, theme, row, col_index)` 를 받아 직접 렌더한다.
//!
//! 본체(`tasty`)·갤러리 양쪽에서 동일 위젯을 호출 → 시각 100% 동기화.

use egui_extras::{Column, TableBuilder};
use tasty_type_appearance::theme::Theme;

/// 컬럼 폭 (egui_extras [`Column`] 매핑).
#[derive(Clone, Copy)]
pub enum TableColumnWidth {
    /// 고정 폭 (리사이즈 불가).
    Exact(f32),
    /// 초기 폭 + 최소 폭.
    Initial { initial: f32, at_least: f32 },
    /// 남은 폭 균등 분배. `at_least` 최소폭(없으면 0.0), `clip` true 면 말줄임.
    Remainder { at_least: f32, clip: bool },
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
}

/// 공용 Table 빌더.
pub struct Table<'a, K> {
    columns: Vec<TableColumn<'a, K>>,
    active_sort: Option<(K, TableSortDir)>,
    header_fill: Option<egui::Color32>,
    header_pad_x: f32,
    header_height: Option<f32>,
    row_height: Option<f32>,
    max_scroll_height: Option<f32>,
    id_salt: Option<egui::Id>,
    selectable: bool,
    striped: bool,
}

impl<'a, K> Table<'a, K> {
    /// 컬럼 정의 목록으로 표를 만든다.
    pub fn new(columns: Vec<TableColumn<'a, K>>) -> Self {
        Self {
            columns,
            active_sort: None,
            header_fill: None,
            header_pad_x: 0.0,
            header_height: None,
            row_height: None,
            max_scroll_height: None,
            id_salt: None,
            selectable: false,
            striped: false,
        }
    }

    /// 현재 활성 정렬 상태(컬럼 키 + 방향). 해당 컬럼 헤더에 ▲/▼ 를 그린다.
    pub fn active_sort(mut self, key: K, dir: TableSortDir) -> Self {
        self.active_sort = Some((key, dir));
        self
    }

    /// 인스턴스 ID salt. 한 화면에 표가 2개 이상이면 egui 자동 ID 가 충돌하므로
    /// (내부 `TableBuilder`/`ScrollArea` Id 가 동일 → "First use of ... ID" 경고),
    /// 인스턴스마다 서로 다른 salt 를 주어 ID 네임스페이스를 분리한다.
    /// 미지정 시 기존 동작(부모 ui 의 Id 네임스페이스 그대로) 유지.
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
    pub fn header_pad_x(mut self, pad: f32) -> Self {
        self.header_pad_x = pad;
        self
    }

    /// 헤더 행 높이. 미지정 시 `font_body + 10`.
    pub fn header_height(mut self, h: f32) -> Self {
        self.header_height = Some(h);
        self
    }

    /// 본문 행 높이. 미지정 시 `font_body + 14`.
    pub fn row_height(mut self, h: f32) -> Self {
        self.row_height = Some(h);
        self
    }

    /// 내부 ScrollArea 최대 높이(이 높이를 넘으면 본문이 스크롤된다).
    pub fn max_scroll_height(mut self, h: f32) -> Self {
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
        let body_f = theme.font_size_body.value();
        let header_h = self.header_height.unwrap_or(body_f + 10.0);
        let row_h = self.row_height.unwrap_or(body_f + 14.0);

        // sticky 헤더 배경: egui_extras 는 셀 배경 API 가 없어 painter 로 직접 칠한다.
        if let Some(fill) = self.header_fill {
            let rect = egui::Rect::from_min_size(
                egui::pos2(ui.max_rect().left(), ui.cursor().top()),
                egui::vec2(ui.max_rect().width(), header_h),
            );
            ui.painter().rect_filled(rect, 0.0, fill);
        }

        let mut clicked_sort: Option<K> = None;
        let mut clicked_row: Option<usize> = None;

        let columns = &self.columns;
        let active_sort = self.active_sort;
        let header_pad_x = self.header_pad_x;
        let selectable = self.selectable;
        let striped = self.striped;
        let max_scroll_height = self.max_scroll_height;

        // 표 본체(헤더 + ScrollArea/Body). `id_salt` 가 있으면 `push_id` 로 감싸
        // 인스턴스마다 Id 네임스페이스를 분리한다. push_id 는 동일 가용 영역을
        // 상속하는 투명 scope 라 시각엔 영향이 없다.
        let mut draw = |ui: &mut egui::Ui| {
            let mut builder = TableBuilder::new(ui)
                .striped(striped)
                .resizable(false)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
            if selectable {
                builder = builder.sense(egui::Sense::click());
            }
            if let Some(ms) = max_scroll_height {
                builder = builder.max_scroll_height(ms);
            }
            for col in columns {
                builder = builder.column(to_column(col.width));
            }

            builder
                .header(header_h, |mut header| {
                    for col in columns {
                        header.col(|ui| {
                            if header_cell(ui, theme, col, active_sort, header_pad_x) {
                                clicked_sort = col.sort_id;
                            }
                        });
                    }
                })
                .body(|mut body| {
                    for (i, row) in rows.iter().enumerate() {
                        body.row(row_h, |mut tr| {
                            tr.set_selected(is_selected(row));
                            for (c, col) in columns.iter().enumerate() {
                                tr.col(|ui| match col.align {
                                    TableAlign::Left => cell(ui, theme, row, c),
                                    TableAlign::Right => {
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| cell(ui, theme, row, c),
                                        );
                                    }
                                });
                            }
                            if tr.response().clicked() {
                                clicked_row = Some(i);
                            }
                        });
                    }
                });
        };

        match self.id_salt {
            Some(salt) => {
                ui.push_id(salt, draw);
            }
            None => draw(ui),
        }

        TableOutput {
            clicked_sort,
            clicked_row,
        }
    }
}

/// 헤더 셀 1칸 렌더. 정렬 가능 컬럼이면 클릭 시 `true`.
fn header_cell<K: Copy + PartialEq>(
    ui: &mut egui::Ui,
    theme: &Theme,
    col: &TableColumn<'_, K>,
    active_sort: Option<(K, TableSortDir)>,
    pad_x: f32,
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
    let text = if arrow.is_empty() {
        col.title.to_string()
    } else {
        format!("{}{arrow}", col.title)
    };
    let rich = egui::RichText::new(text)
        .color(if is_active {
            egui::Color32::from(theme.text)
        } else {
            egui::Color32::from(theme.subtext0)
        })
        .size(theme.font_size_caption.value())
        .strong();

    let clickable = col.sort_id.is_some();
    let do_cell = move |ui: &mut egui::Ui| -> bool {
        if pad_x > 0.0 {
            ui.add_space(pad_x);
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
        TableAlign::Right => ui
            .with_layout(egui::Layout::right_to_left(egui::Align::Center), do_cell)
            .inner,
    }
}

fn to_column(width: TableColumnWidth) -> Column {
    match width {
        TableColumnWidth::Exact(w) => Column::exact(w),
        TableColumnWidth::Initial { initial, at_least } => {
            Column::initial(initial).at_least(at_least)
        }
        TableColumnWidth::Remainder { at_least, clip } => {
            Column::remainder().at_least(at_least).clip(clip)
        }
    }
}
