//! 캔버스를 둘러싼 것들 — 헤더 · 러너 배지 · 줌 클러스터 · 미니맵 · 사이클 배너 ·
//! LOD 칩 · 빈 상태.
//!
//! 좁은 폭에서는 **정보 밀도가 아니라 우선순위** 로 접는다. 러너 배지(그래프가
//! 진행 중인가)와 사이클 배너(그래프가 애초에 돌 수 없는가)는 마지막까지 남고,
//! 줌 퍼센트 숫자와 미니맵처럼 없어도 조작이 가능한 것부터 사라진다.
//!
//! # 헤더와 크롬은 다른 레이어다
//!
//! 헤더 띠는 **정체성**(어떤 DAG 를, 얼마나, 누가 돌리고 있나)만 싣는다. 줌·fit·
//! 방향처럼 캔버스를 직접 조작하는 것들은 헤더가 아니라 캔버스 위에 뜨는
//! 오버레이(우하단 미니맵 + 그 아래 줌 클러스터)다 — 조작 대상 옆에 붙어 있어야
//! 손이 왕복하지 않고, `dag-chrome-bg`/`-border` 도 그 덩어리를 위한 토큰이다.

use tasty_dag_layout::GraphLayout;
use tasty_model::DagDirection;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{ControlSize, IconButton, IconButtonVariant, hspace, margin_sym};

use super::model::{DagData, DagGraphData, RunnerBadgeData};
use super::view::{DagGraphView, Lod, ZOOM_MAX, ZOOM_MIN};
use crate::adapters::ui::icons;
use crate::i18n::{t, t_fmt, t_fmt2};

/// 헤더·크롬에서 나온 조작.
pub enum ChromeAction {
    SelectDag(String),
    ToggleDirection,
    Fit,
    /// 줌 단계 변경. 앵커는 **캔버스 중심**이어야 하므로 여기서 적용하지 않고
    /// 캔버스 크기를 아는 호출자에게 넘긴다.
    Zoom(f32),
    /// 폴링 주기를 기다리지 않고 지금 다시 읽는다.
    Refresh,
}

/// 상단 헤더 — DAG 선택 · 진척 · 러너 배지 · 새로고침.
///
/// `surface_width` 가 [`NARROW_DETAIL_SHEET`] 미만이면 **2 줄로 접는다**: 첫 줄은
/// 정체성(어떤 DAG 를, 얼마나), 둘째 줄은 러너 상태와 새로고침. 한 줄로 밀어 넣으면
/// 좁은 폭에서 배지가 picker 를 밀어내 DAG 이름이 먼저 잘리는데, 그건 이 화면에서
/// 제일 먼저 읽어야 하는 정보다.
pub fn draw_header(
    ui: &mut egui::Ui,
    theme: &Theme,
    data: &DagData,
    surface_width: f32,
) -> Option<ChromeAction> {
    let mut action = None;
    let stacked = surface_width < NARROW_DETAIL_SHEET.value();
    let row_h = theme.dag_chrome_height().value();

    // `dag-chrome-*` 는 **줌 클러스터 덩어리**의 토큰이다. 헤더 띠는 surface 의
    // 상단 띠이므로 sidebar 계열 배경 + `separator` 헤어라인을 쓴다 — 색 값은
    // 지금 같지만(별칭) 토큰이 갈리는 날 헤더가 클러스터를 따라가면 안 된다.
    egui::Frame::NONE
        .fill(theme.bg_sidebar().to_egui())
        .inner_margin(margin_sym(theme.spacing_sm, theme.spacing_xs))
        .show(ui, |ui| {
            ui.set_height(if stacked { row_h * 2.0 } else { row_h });
            if stacked {
                ui.vertical(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), row_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| identity_group(ui, theme, data, &mut action),
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), row_h),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            // 좁은 줄에서는 알약만 남긴다 — 재개 힌트는 캡션이라
                            // 잘리면 명령이 반쪽이 되어 오히려 위험하다.
                            runner_badge(ui, theme, &data.runner);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if refresh_button(ui, theme) {
                                        action = Some(ChromeAction::Refresh);
                                    }
                                },
                            );
                        },
                    );
                });
            } else {
                ui.horizontal_centered(|ui| {
                    identity_group(ui, theme, data, &mut action);
                    // 이 줄은 오른쪽부터 채운다 — 먼저 넣은 것이 더 오른쪽에 놓이므로
                    // 화면에서 읽히는 순서(알약 → 캡션 → 새로고침)의 **역순**으로 넣는다.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if refresh_button(ui, theme) {
                            action = Some(ChromeAction::Refresh);
                        }
                        resume_hint(ui, theme, &data.runner);
                        runner_badge(ui, theme, &data.runner);
                    });
                });
            }
        });

    // 헤더 ↔ 캔버스 구분선. `separator` 는 알파가 이미 곱해진 색이라
    // premultiplied 로 읽는다 — `to_egui()` 로 읽으면 한 번 더 곱해져 옅어진다.
    let (sep, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        sep.x_range(),
        sep.center().y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui_premultiplied(),
        ),
    );
    action
}

/// 헤더 좌측 그룹 — 어떤 DAG 를, 얼마나.
fn identity_group(
    ui: &mut egui::Ui,
    theme: &Theme,
    data: &DagData,
    action: &mut Option<ChromeAction>,
) {
    // 이 띠가 무엇에 대한 것인지 알리는 글리프. 탭 제목이 없는 popup 자리에서도
    // 같은 화면임을 알아보는 손잡이다.
    ui.add(icons::GIT_TREE.image(
        theme.icon_glyph_size_sm.value(),
        theme.dag_chrome_fg().to_egui(),
    ));
    hspace(ui, theme.spacing_xs);
    if let Some(picked) = dag_picker(ui, theme, data) {
        *action = Some(ChromeAction::SelectDag(picked));
    }
    if let Some(graph) = &data.current {
        hspace(ui, theme.spacing_sm);
        ui.label(
            egui::RichText::new(t_fmt2(
                "dag.header.progress",
                &graph.done.to_string(),
                &graph.total().to_string(),
            ))
            .size(theme.dag_row_count_font_size().value())
            .color(theme.dag_row_count_fg().to_egui()),
        );
    }
}

/// 폴링을 기다리지 않고 지금 다시 읽는다. 눌렸으면 `true`.
fn refresh_button(ui: &mut egui::Ui, theme: &Theme) -> bool {
    IconButton::new()
        .variant(IconButtonVariant::Ghost)
        .size(ControlSize::Sm)
        .show(ui, theme, &|ui, rect, c| {
            icons::REFRESH.image(rect.height(), c).paint_at(ui, rect)
        })
        .on_hover_text(t("dag.header.refresh"))
        .clicked()
}

/// DAG 선택 드롭다운. 목록이 하나뿐이면 그냥 이름만 보인다.
fn dag_picker(ui: &mut egui::Ui, theme: &Theme, data: &DagData) -> Option<String> {
    let current_name = data
        .current
        .as_ref()
        .map(|g| g.name.clone())
        .unwrap_or_else(|| t("dag.header.no_dag").to_string());

    if data.dags.len() <= 1 {
        ui.label(
            egui::RichText::new(current_name)
                .size(theme.font_size_body.value())
                .color(theme.dag_chrome_fg().to_egui()),
        );
        return None;
    }

    let mut picked = None;
    egui::ComboBox::from_id_salt("dag_picker")
        .selected_text(current_name)
        .show_ui(ui, |ui| {
            for entry in &data.dags {
                let selected = data.current.as_ref().is_some_and(|g| g.id == entry.id);
                let label = t_fmt2(
                    "dag.header.entry",
                    &entry.name,
                    &entry.task_count.to_string(),
                );
                let text = egui::RichText::new(format!("{} {}", entry.rollup.glyph(), label))
                    .size(theme.font_size_caption.value());
                if ui.selectable_label(selected, text).clicked() {
                    picked = Some(entry.id.clone());
                }
            }
        });
    picked
}

/// 러너 배지. `stalled`(할 일이 있는데 아무도 안 돌린다)는 1급 경고다 — 이 화면이
/// 없으면 사용자는 "왜 안 도는지" 를 CLI 로 파헤쳐야 한다.
///
/// 알약은 **StatusDot + 문구** 두 채널이다. 색만으로 생사를 표기하면 색각 이상
/// 사용자에게 통째로 사라지고, 반대로 점만 있으면 "몇 개가 대기 중인가" 가 빠진다.
/// 재개 힌트 캡션은 [`resume_hint`] 가 따로 그린다 — 줄이 오른쪽부터 채워지는 자리라
/// 캡션을 알약보다 **먼저** 넣어야 알약 오른쪽에 놓인다.
fn runner_badge(ui: &mut egui::Ui, theme: &Theme, runner: &RunnerBadgeData) {
    let stalled = runner.is_stalled();
    let (bg, border, fg, dot, text) = if runner.crashed {
        (
            theme.dag_runner_crashed_bg(),
            theme.dag_runner_crashed_border(),
            theme.dag_runner_crashed_fg(),
            theme.status_dot_danger(),
            t_fmt("dag.runner.crashed", &runner.ready.to_string()),
        )
    } else if stalled {
        (
            theme.dag_runner_stalled_bg(),
            theme.dag_runner_stalled_border(),
            theme.dag_runner_stalled_fg(),
            theme.status_dot_warning(),
            t_fmt("dag.runner.stalled", &runner.ready.to_string()),
        )
    } else if runner.running {
        (
            theme.dag_runner_bg(),
            theme.dag_runner_border(),
            theme.dag_runner_fg(),
            theme.status_dot_success(),
            t_fmt2(
                "dag.runner.running",
                &runner.running_count.to_string(),
                &runner.ready.to_string(),
            ),
        )
    } else {
        (
            theme.dag_runner_bg(),
            theme.dag_runner_border(),
            theme.dag_runner_idle_fg(),
            theme.status_dot_idle(),
            t("dag.runner.idle").to_string(),
        )
    };

    let resp = egui::Frame::NONE
        .fill(bg.to_egui())
        .stroke(egui::Stroke::new(
            theme.border_width.value(),
            border.to_egui(),
        ))
        .corner_radius(theme.dag_runner_radius().value())
        .inner_margin(margin_sym(theme.dag_runner_padding_x(), theme.spacing_xs))
        .show(ui, |ui| {
            // 점 + 문구를 **한 번의 exact 할당**으로 배치한다. 중첩 레이아웃을 쓰면
            // 오른쪽 정렬 줄 안에서 알약이 남은 폭을 통째로 삼켜 헤더를 밀어낸다.
            // mono 로 짠다 — running/ready 카운트가 0.5 초마다 바뀌는 자리라
            // 비례폭이면 숫자가 한 자리 늘 때마다 알약 폭이 출렁인다.
            let font = egui::FontId::monospace(theme.font_size_caption.value());
            let galley = ui.painter().layout_no_wrap(text, font, fg.to_egui());
            let d = theme.status_dot_size().value();
            let gap = theme.dag_runner_gap().value();
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(
                    d + gap + galley.size().x,
                    theme.dag_runner_height().value().max(galley.size().y),
                ),
                egui::Sense::hover(),
            );
            ui.painter().circle_filled(
                egui::pos2(rect.min.x + d / 2.0, rect.center().y),
                d / 2.0,
                dot.to_egui(),
            );
            ui.painter().galley(
                egui::pos2(
                    rect.min.x + d + gap,
                    rect.center().y - galley.size().y / 2.0,
                ),
                galley,
                fg.to_egui(),
            );
        })
        .response;

    if runner.crashed || stalled {
        // 실행 가능한 복구 수단을 그대로 적는다. 이 화면에는 러너를 켜는 버튼이
        // 없다 — 관찰 전용 surface 라 상태를 바꾸지 않는다.
        resp.on_hover_text(format!(
            "{} {}",
            t("dag.runner.resume_hint_lead"),
            t("dag.runner.resume_hint_command")
        ));
    }
}

/// 재개 명령 캡션. 툴팁 전용이면 호버하지 않는 사용자에게는 복구 수단이 존재하지
/// 않는 것과 같다 — 알약 옆에 **상시** 붙인다(좁은 헤더에서만 접는다).
fn resume_hint(ui: &mut egui::Ui, theme: &Theme, runner: &RunnerBadgeData) {
    if !runner.crashed && !runner.is_stalled() {
        return;
    }
    // 명령만 mono 로 갈라 그린다 — 셸에 그대로 붙여 넣는 문자열이라 문자 폭이
    // 고정돼야 인자 경계가 눈에 잡힌다(갤러리 specimen 이 전사한 형태).
    let caption = |ui: &mut egui::Ui, text: String, mono: bool| {
        let mut rich = egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui());
        if mono {
            rich = rich.monospace();
        }
        ui.label(rich);
    };
    // 이 줄은 오른쪽부터 채워지므로 읽는 순서(lead → 명령)의 **역순**으로 넣는다.
    // 둘 사이 간격은 줄의 기본 item_spacing(=spacing_sm) 이 그대로 맡는다 —
    // specimen 이 명시한 값과 같은 토큰이라 따로 벌리지 않는다.
    caption(ui, t("dag.runner.resume_hint_command").to_string(), true);
    caption(ui, t("dag.runner.resume_hint_lead").to_string(), false);
    hspace(ui, theme.spacing_sm);
}

/// 줌 퍼센트 판독창 폭.
const ZOOM_READOUT_WIDTH: LogicalPx = LogicalPx(46.0);
/// 이 폭 아래에서는 판독창을 접고 버튼만 남긴다.
///
/// 미니맵의 `dag-minimap-min-surface` 와 달리 이 값은 대응 디자인 토큰이 없다 —
/// 시안이 좁은 폭 축약을 미니맵에 대해서만 명시했다. 토큰이 생기면 `Theme` 에서
/// 가져오도록 바꾼다.
const NARROW_ZOOM_LABEL: LogicalPx = LogicalPx(400.0);
/// 상세를 우측 패널로 둘 수 있는 최소 surface 폭. 그 아래는 하단 시트다.
pub const NARROW_DETAIL_SHEET: LogicalPx = LogicalPx(640.0);
/// 빈 상태 글리프 크기. 24px 아이콘 토큰이 아직 없어 시안 값을 그대로 둔다.
const EMPTY_ICON_SIZE: LogicalPx = LogicalPx(24.0);

/// 줌 클러스터 전체 크기 — 버튼 4 개 + (판독창) + 1px 구분선.
fn zoom_cluster_size(theme: &Theme, compact: bool) -> egui::Vec2 {
    let h = theme.dag_chrome_height().value();
    let mut w = h * 4.0 + theme.border_width.value();
    if !compact {
        w += ZOOM_READOUT_WIDTH.value();
    }
    egui::vec2(w, h)
}

/// 캔버스 안에서 줌 클러스터가 차지하는 자리. 캔버스 인터랙션이 이 영역을 비켜가야
/// 하므로 그리기 전에 먼저 알아야 한다.
pub fn zoom_cluster_rect(theme: &Theme, canvas: egui::Rect) -> egui::Rect {
    let inset = theme.dag_chrome_inset().value();
    let size = zoom_cluster_size(theme, canvas.width() < NARROW_ZOOM_LABEL.value());
    egui::Rect::from_min_size(
        egui::pos2(canvas.max.x - inset - size.x, canvas.max.y - inset - size.y),
        size,
    )
}

/// 클러스터 셀 하나. 눌렸으면 `true`.
fn cluster_cell(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    salt: &'static str,
    icon: icons::Icon,
    enabled: bool,
    tooltip: &str,
) -> bool {
    let resp = ui.interact(
        rect,
        ui.id().with(("dag_zoom_cell", salt)),
        egui::Sense::click(),
    );
    if enabled && resp.hovered() {
        ui.painter().rect_filled(
            rect,
            theme.corner_radius_sm.value(),
            theme.overlay_hover().to_egui_premultiplied(),
        );
    }
    let color = if enabled {
        theme.dag_chrome_fg()
    } else {
        theme.text_disabled()
    };
    let side = theme.icon_glyph_size_sm.value();
    icon.image(side, color.to_egui()).paint_at(
        ui,
        egui::Rect::from_center_size(rect.center(), egui::vec2(side, side)),
    );
    enabled && resp.on_hover_text(tooltip).clicked()
}

/// `−  %  +  |  fit  dir` — 28px 한 줄, 1px 보더로 묶인 한 덩어리.
///
/// 캔버스 위 오버레이라 헤더가 아니라 여기서 그린다. 버튼을 헤더로 올리면 조작
/// 대상(그래프)과 손잡이가 화면 양 끝으로 갈라진다.
fn draw_zoom_cluster(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    view: &DagGraphView,
    direction: DagDirection,
) -> Option<ChromeAction> {
    let compact = rect.width() < zoom_cluster_size(theme, false).x;
    let radius = theme.corner_radius.value();
    let border = theme.dag_chrome_border().to_egui();
    ui.painter()
        .rect_filled(rect, radius, theme.dag_chrome_bg().to_egui());
    ui.painter().rect_stroke(
        rect,
        radius,
        egui::Stroke::new(theme.border_width.value(), border),
        egui::StrokeKind::Inside,
    );

    let side = theme.dag_chrome_height();
    let mut x = rect.min.x;
    // 폭을 `LogicalPx` 로 받는다. 호출 인자가 전부 Theme 값이거나 명명 길이 상수라
    // 여기서 받으면 그 값들이 벗겨지지 않은 채 들어오고, 벗기는 자리가 egui 로 나가는
    // 이 본문 안으로 모인다. 벗기기 총수는 거의 그대로다(이 파일 53 → 52) — 얻는 것은
    // 개수가 아니라 위치다.
    let mut cell = |w: LogicalPx| {
        let r = egui::Rect::from_min_size(
            egui::pos2(x, rect.min.y),
            egui::vec2(w.value(), side.value()),
        );
        x += w.value();
        r
    };

    let mut action = None;
    let minus = cell(side);
    if cluster_cell(
        ui,
        theme,
        minus,
        "out",
        icons::MINUS,
        view.zoom > ZOOM_MIN,
        t("dag.zoom.out"),
    ) {
        action = Some(ChromeAction::Zoom(-1.0));
    }
    if !compact {
        let readout = cell(ZOOM_READOUT_WIDTH);
        ui.painter().text(
            readout.center(),
            egui::Align2::CENTER_CENTER,
            format!("{}%", (view.zoom * 100.0).round() as i32),
            egui::FontId::monospace(theme.font_size_caption.value()),
            theme.dag_chrome_fg().to_egui(),
        );
    }
    let plus = cell(side);
    if cluster_cell(
        ui,
        theme,
        plus,
        "in",
        icons::PLUS,
        view.zoom < ZOOM_MAX,
        t("dag.zoom.in"),
    ) {
        action = Some(ChromeAction::Zoom(1.0));
    }

    let sep = cell(theme.border_width);
    ui.painter().rect_filled(sep, 0.0, border);

    if cluster_cell(
        ui,
        theme,
        cell(side),
        "fit",
        icons::FIT,
        true,
        t("dag.zoom.fit"),
    ) {
        action = Some(ChromeAction::Fit);
    }
    // 방향 글리프는 **지금** 어느 방향인지를 보여준다 — 버튼이 무엇으로 바뀌는지가
    // 아니라 현재 상태를 읽는 쪽이 그래프와 대조하기 쉽다.
    let dir_icon = match direction {
        DagDirection::LeftRight => icons::ARROW_RIGHT,
        DagDirection::TopDown => icons::ARROW_DOWN,
    };
    if cluster_cell(
        ui,
        theme,
        cell(side),
        "dir",
        dir_icon,
        true,
        t("dag.zoom.direction"),
    ) {
        action = Some(ChromeAction::ToggleDirection);
    }
    action
}

/// 캔버스 위 오버레이 전부 — 우하단 미니맵 + 그 아래 줌 클러스터, 좌하단 LOD 칩.
pub fn draw_canvas_chrome(
    ui: &mut egui::Ui,
    theme: &Theme,
    canvas: egui::Rect,
    view: &DagGraphView,
    layout: &GraphLayout,
    direction: DagDirection,
    lod: Lod,
) -> Option<ChromeAction> {
    let cluster = zoom_cluster_rect(theme, canvas);
    let graph_size = egui::vec2(layout.width.value(), layout.height.value());
    paint_minimap(ui, theme, canvas, cluster, view, layout, graph_size);
    let action = draw_zoom_cluster(ui, theme, cluster, view, direction);
    paint_lod_chip(&ui.painter_at(canvas), theme, canvas, lod);
    action
}

/// 사이클 배너. 사이클이면 그래프는 그리되 "이 그래프는 완주할 수 없다" 를 알린다.
pub fn draw_cycle_banner(ui: &mut egui::Ui, theme: &Theme, cycle: &[String]) {
    let fg = theme.dag_cycle_fg().to_egui();
    let dock = egui::Frame::NONE
        .fill(theme.dag_cycle_bg().to_egui())
        .inner_margin(margin_sym(theme.spacing_md, theme.spacing_xs))
        .show(ui, |ui| {
            ui.set_height(theme.dag_cycle_height().value());
            ui.horizontal_centered(|ui| {
                ui.add(icons::ALERT_TRIANGLE.image(theme.icon_glyph_size_sm.value(), fg));
                hspace(ui, theme.spacing_sm);
                ui.label(
                    egui::RichText::new(t_fmt("dag.cycle.lead", &cycle.len().to_string()))
                        .size(theme.font_size_caption.value())
                        .color(fg),
                );
                // 경로는 mono 로 갈라 읽는다. 마지막에 첫 id 를 다시 붙여 **닫힌
                // 고리**임을 눈으로 보여준다 — 열린 나열은 사이클로 안 읽힌다.
                let path = match cycle.first() {
                    Some(head) => format!("{} \u{2192} {head}", cycle.join(" \u{2192} ")),
                    None => String::new(),
                };
                hspace(ui, theme.spacing_xs);
                ui.label(
                    egui::RichText::new(path)
                        .monospace()
                        .size(theme.font_size_caption.value())
                        .color(theme.text_secondary().to_egui()),
                );
            });
        })
        .response
        .rect;

    // 아래 변에만 선을 긋는다 — 캔버스와의 경계지 떠 있는 상자가 아니다.
    ui.painter().hline(
        dock.x_range(),
        dock.max.y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.dag_cycle_border().to_egui(),
        ),
    );
}

/// 좌하단 LOD 칩 — 지금 카드가 왜 축약돼 보이는지 알린다.
pub fn paint_lod_chip(painter: &egui::Painter, theme: &Theme, rect: egui::Rect, lod: Lod) {
    if lod == Lod::Full {
        return;
    }
    let text = match lod {
        Lod::Compact => t("dag.lod.compact"),
        _ => t("dag.lod.block"),
    };
    let pad = theme.spacing_xs.value();
    let font = egui::FontId::proportional(theme.font_size_micro.value());
    let galley = painter.layout_no_wrap(text.to_string(), font, theme.dag_chrome_fg().to_egui());
    let size = galley.size() + egui::vec2(pad * 2.0, pad);
    let chip = egui::Rect::from_min_size(
        egui::pos2(rect.min.x + pad, rect.max.y - size.y - pad),
        size,
    );
    painter.rect_filled(
        chip,
        theme.dag_runner_radius().value(),
        theme.dag_chrome_bg().to_egui(),
    );
    painter.galley(
        chip.min + egui::vec2(pad, pad / 2.0),
        galley,
        theme.dag_chrome_fg().to_egui(),
    );
}

/// 우하단 미니맵. surface 가 좁으면 그리지 않는다 — 캔버스를 가리는 손해가 크다.
/// 자리는 줌 클러스터 **바로 위**다(두 오버레이가 한 기둥으로 읽힌다).
fn paint_minimap(
    ui: &egui::Ui,
    theme: &Theme,
    canvas: egui::Rect,
    cluster: egui::Rect,
    view: &DagGraphView,
    layout: &GraphLayout,
    graph_size: egui::Vec2,
) {
    if canvas.width() < theme.dag_minimap_min_surface().value()
        || graph_size.x <= 0.0
        || graph_size.y <= 0.0
    {
        return;
    }
    let painter = ui.painter_at(canvas);
    let gap = theme.spacing_sm.value();
    let size = egui::vec2(
        theme.dag_minimap_width().value(),
        theme.dag_minimap_height().value(),
    );
    let map = egui::Rect::from_min_size(
        egui::pos2(
            canvas.max.x - size.x - theme.dag_chrome_inset().value(),
            cluster.min.y - gap - size.y,
        ),
        size,
    );
    painter.rect_filled(
        map,
        theme.dag_node_radius().value(),
        theme.dag_minimap_bg().to_egui(),
    );

    let k = (size.x / graph_size.x).min(size.y / graph_size.y);
    let base = map.min + (size - graph_size * k) / 2.0;
    let node = theme.dag_minimap_node().to_egui();
    let (nw, nh) = (
        theme.dag_node_width().value() * k,
        theme.dag_node_height().value() * k,
    );
    for n in &layout.nodes {
        painter.rect_filled(
            egui::Rect::from_min_size(
                base + egui::vec2(n.x.value() * k, n.y.value() * k),
                egui::vec2(nw.max(1.0), nh.max(1.0)),
            ),
            0.0,
            node,
        );
    }

    // 현재 뷰포트가 그래프의 어디를 보고 있는지. 화면 좌표 → 그래프 좌표 역변환.
    let vp = egui::Rect::from_min_size(
        base + egui::vec2(
            -view.offset.x / view.zoom * k,
            -view.offset.y / view.zoom * k,
        ),
        egui::vec2(
            canvas.width() / view.zoom * k,
            canvas.height() / view.zoom * k,
        ),
    );
    painter.rect_stroke(
        vp.intersect(map),
        0.0,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.dag_minimap_viewport().to_egui(),
        ),
        egui::StrokeKind::Inside,
    );
}

/// 빈 상태 2 종 — "workspace 에 DAG 가 없다" 와 "지정한 DAG 가 사라졌다".
///
/// 글리프 → 제목 → 본문을 한 덩어리로 묶어 **세로 정중앙**에 놓는다. 1/3 지점에
/// 두면 아래가 휑하게 비어 화면이 로딩 중인 것처럼 읽힌다.
pub fn draw_empty(ui: &mut egui::Ui, theme: &Theme, data: &DagData, dag_id: Option<&str>) {
    let (icon, title, hint) = if data.target_missing {
        (
            icons::SEARCH,
            t_fmt("dag.empty.missing", dag_id.unwrap_or("")),
            t("dag.empty.missing_hint").to_string(),
        )
    } else {
        (
            icons::GIT_TREE,
            t("dag.empty.none").to_string(),
            t_fmt("dag.empty.none_hint", &data.workspace_id.to_string()),
        )
    };
    egui::Frame::NONE
        .fill(theme.dag_canvas_bg().to_egui())
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            let rect = ui.max_rect();
            let side = EMPTY_ICON_SIZE.value();
            let gap = theme.spacing_sm.value();
            let title_font = egui::FontId::proportional(theme.font_size_body.value());
            let title_h = ui.fonts(|f| f.row_height(&title_font));
            // 본문은 measure_sm 안에서만 접는다 — 넓은 화면에서 한 줄로 늘어지면
            // 눈이 되돌아올 지점을 잃는다.
            let measure = theme
                .measure_sm
                .value()
                .min(rect.width() - theme.spacing_xl.value() * 2.0)
                .max(theme.spacing_xl.value());
            let body = ui.painter().layout(
                hint,
                egui::FontId::proportional(theme.font_size_caption.value()),
                theme.text_muted().to_egui(),
                measure,
            );

            let total = side + gap + title_h + gap + body.size().y;
            let mut y = rect.center().y - total / 2.0;
            icon.image(side, theme.text_disabled().to_egui()).paint_at(
                ui,
                egui::Rect::from_min_size(
                    egui::pos2(rect.center().x - side / 2.0, y),
                    egui::vec2(side, side),
                ),
            );
            y += side + gap;
            ui.painter().text(
                egui::pos2(rect.center().x, y),
                egui::Align2::CENTER_TOP,
                title,
                title_font,
                theme.text_secondary().to_egui(),
            );
            y += title_h + gap;
            ui.painter().galley(
                egui::pos2(rect.center().x - body.size().x / 2.0, y),
                body,
                theme.text_muted().to_egui(),
            );
        });
}

/// 그래프 데이터가 비어 있는지(노드 0 개).
pub fn is_empty(graph: Option<&DagGraphData>) -> bool {
    graph.is_none_or(|g| g.nodes.is_empty())
}
