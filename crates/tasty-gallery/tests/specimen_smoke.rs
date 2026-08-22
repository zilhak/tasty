//! specimen headless smoke — GPU 없이 egui 프레임을 돌려 specimen draw 가
//! 패닉(RefCell 이중 borrow·레이아웃 위반) 없이 렌더되는지 회귀 격리한다.
//! 픽셀 판정은 하지 않는다 — 시각 정합은 갤러리 육안 몫.

use tasty_gallery::catalog::components::{dag, prim_drilldown, prim_listctrl};
use tasty_ui_widgets::{Button, ButtonVariant, ControlSize, DrillDown, DrillDownView};

fn run_frames(mut body: impl FnMut(&mut egui::Ui)) {
    let ctx = egui::Context::default();
    // 상태(thread_local)가 프레임을 넘어 유지되는 경로까지 몇 프레임 돌린다.
    for _ in 0..3 {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| body(ui));
        });
    }
}

#[test]
fn listctrl_specimen_은_헤드리스로_렌더된다() {
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| prim_listctrl::draw(ui, &theme));
}

#[test]
fn drilldown_specimen_은_헤드리스로_렌더된다() {
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| prim_drilldown::draw(ui, &theme));
}

#[test]
fn drilldown_detail_뷰는_backbar_와_본문을_렌더된다() {
    // specimen 초기 상태는 List 라 Detail 경로는 위젯 직접 호출로 커버한다.
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| {
        let apply = |ui: &mut egui::Ui, th: &tasty_type_appearance::theme::Theme| {
            Button::new("Apply")
                .variant(ButtonVariant::Primary)
                .size(ControlSize::Sm)
                .show(ui, th);
        };
        let out = DrillDown::new("smoke_detail")
            .view(DrillDownView::Detail)
            .title("Default")
            .back_label("Back")
            .height(theme.measure_sm.value())
            .show(
                ui,
                &theme,
                |_, _| {},
                |ui, _| {
                    ui.label("preview body");
                },
                Some(&apply),
            );
        assert!(!out.back_clicked);
    });
}

// ── Task DAG ────────────────────────────────────────────────────────
// 캔버스/서피스 specimen 은 레이아웃 엔진 호출 + 절대좌표 페인팅 + 중첩
// ScrollArea 가 한 프레임에 겹친다. 폭이 좁을 때 음수 폭이 새지 않는지까지
// 여기서 잡는다.

#[test]
fn dag_canvas_specimen_은_헤드리스로_렌더된다() {
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| dag::canvas::draw(ui, &theme));
}

#[test]
fn dag_node_specimen_3종은_헤드리스로_렌더된다() {
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| dag::node::draw_states(ui, &theme));
    run_frames(|ui| dag::node::draw_kinds(ui, &theme));
    run_frames(|ui| dag::node::draw_lod(ui, &theme));
}

#[test]
fn dag_edges_specimen_은_헤드리스로_렌더된다() {
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| dag::edges::draw(ui, &theme));
}

#[test]
fn dag_chrome_와_runner_specimen_은_헤드리스로_렌더된다() {
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| dag::chrome::draw(ui, &theme));
    run_frames(|ui| dag::runner::draw(ui, &theme));
}

#[test]
fn dag_detail_와_states_specimen_은_헤드리스로_렌더된다() {
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| dag::detail::draw(ui, &theme));
    run_frames(|ui| dag::states::draw(ui, &theme));
}

#[test]
fn dag_surface_specimen_은_헤드리스로_렌더된다() {
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| dag::surface::draw(ui, &theme));
}

#[test]
fn dag_rows_와_window_specimen_은_헤드리스로_렌더된다() {
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| dag::rows::draw(ui, &theme));
    // popup specimen 은 560 폭 창 두 개를 가로로 놓는다 — 무대보다 넓어 남는
    // 폭이 음수로 새기 쉬운 배치라 여기서 함께 잡는다.
    run_frames(|ui| dag::window::draw(ui, &theme));
}

/// 레이아웃 캐시 불변식의 갤러리 쪽 대응 — 좌표는 id + 의존 엣지 + config 만
/// 보고 나온다. 상태를 바꿔도 노드 좌표가 한 픽셀도 움직이지 않아야 0.5 초
/// 폴링이 그래프를 흔들지 않는다.
#[test]
fn dag_레이아웃은_task_상태에_영향받지_않는다() {
    let theme = tasty_themes::mocha_fallback();
    let before = dag::layout(
        &dag::build_dag(),
        &theme,
        tasty_dag_layout::Orientation::TopDown,
    );
    let mut mutated = dag::build_dag();
    for n in &mut mutated.nodes {
        n.status = dag::Status::Succeeded;
        n.dur = Some("999s".into());
    }
    let after = dag::layout(&mutated, &theme, tasty_dag_layout::Orientation::TopDown);
    assert_eq!(
        before.nodes, after.nodes,
        "task 상태가 DAG 레이아웃 좌표를 바꿨다"
    );
}
