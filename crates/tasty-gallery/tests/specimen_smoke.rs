//! specimen headless smoke — GPU 없이 egui 프레임을 돌려 specimen draw 가
//! 패닉(RefCell 이중 borrow·레이아웃 위반) 없이 렌더되는지 회귀 격리한다.
//! 픽셀 판정은 하지 않는다 — 시각 정합은 갤러리 육안 몫.

use tasty_gallery::catalog::chrome_loading;
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

/// egui id 충돌 마커를 **헤드리스로** 잡는다. `check_for_id_clash` 는 패닉하지 않고
/// debug 레이어에 "First/Double use of … ID …" 텍스트만 그린다(`context.rs`) — 그래서
/// `let _ = ctx.run(...)` 로 `FullOutput` 을 버리는 `run_frames` 는 못 잡는다(값을 버리는
/// 것이 검증을 무력화한 실례: `docs/dev-guide/error-handling.md`). 여기서는 `FullOutput`
/// 을 받아 그려진 모든 텍스트 shape 를 훑어 그 마커 문구가 있으면 실패시킨다.
///
/// 이 가드가 잡는 것은 **id 충돌뿐**이다 — "상자 밖 렌더"(레이아웃 깨짐)는 마커 없이도
/// 일어나므로(리뷰 변이 B) 여기서 안 잡힌다. 그건 별도 rect-포함 단언이 필요하다.
fn assert_no_id_clash(label: &str, mut body: impl FnMut(&mut egui::Ui)) {
    let ctx = egui::Context::default();
    // release 로 테스트를 돌려도(그땐 기본이 꺼짐) 마커가 그려지도록 강제 — 안 켜면
    // "0 건" 이 프로파일 의존 동어반복이 된다(리뷰 §5-c1).
    ctx.options_mut(|o| o.warn_on_id_clash = true);
    let mut found: Vec<String> = Vec::new();
    for _ in 0..3 {
        let output = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| body(ui));
        });
        for cs in &output.shapes {
            collect_id_clash_text(&cs.shape, &mut found);
        }
    }
    assert!(
        found.is_empty(),
        "{label}: egui id 충돌 마커가 렌더됐다 — 같은 자리에서 auto-id 가 재사용된다\
         (push_id 로 갈라야 한다). 마커: {found:?}"
    );
}

fn collect_id_clash_text(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
    match shape {
        egui::epaint::Shape::Text(t) => {
            let s = t.galley.text();
            // "First use of … ID …" / "Double use of … ID …" (egui context.rs).
            if s.contains("use of") && s.contains("ID") {
                out.push(s.to_string());
            }
        }
        egui::epaint::Shape::Vec(v) => {
            for s in v {
                collect_id_clash_text(s, out);
            }
        }
        _ => {}
    }
}

// ── id 충돌 회귀 가드 (galfix ⑶) ────────────────────────────────────
// modifier-hint 패널은 같은 소스 위치에서 4번 호출되는 panel() 의 auto-id 가 재사용돼
// id 가 충돌했고, tutorial 의 topic popup 도 같은 계열의 ScrollArea id 충돌이 있었다.
// push_id 로 갈라 고쳤다 — 되돌리면(push_id 제거) 이 가드가 마커를 잡아 FAIL 한다.

#[test]
fn modifier_hint_specimen_은_id_충돌_없이_렌더된다() {
    use tasty_gallery::catalog::components::modifier_hint;
    let theme = tasty_themes::mocha_fallback();
    assert_no_id_clash("modifier_hint::draw", |ui| modifier_hint::draw(ui, &theme));
}

#[test]
fn tutorial_specimen_4종은_id_충돌_없이_렌더된다() {
    use tasty_gallery::catalog::widgets::tutorial;
    let theme = tasty_themes::mocha_fallback();
    assert_no_id_clash("tutorial::draw_marker", |ui| {
        tutorial::draw_marker(ui, &theme)
    });
    assert_no_id_clash("tutorial::draw_callout", |ui| {
        tutorial::draw_callout(ui, &theme)
    });
    assert_no_id_clash("tutorial::draw_topics", |ui| {
        tutorial::draw_topics(ui, &theme)
    });
    assert_no_id_clash("tutorial::draw_composite", |ui| {
        tutorial::draw_composite(ui, &theme)
    });
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
/// 부팅/종료 로딩 specimen — 두 화면은 같은 `draw_frame` 을 공유하므로 한 테스트로
/// 함께 잡는다. 중앙 스택은 `top_pad` 를 음수로 클램프하는 계산에 의존해서, 무대가
/// 스택보다 낮은 프레임에서 레이아웃이 새기 쉬운 자리다.
#[test]
fn loading_specimen_은_헤드리스로_렌더된다() {
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| chrome_loading::draw_default(ui, &theme));
    run_frames(|ui| chrome_loading::draw_min(ui, &theme));
    run_frames(|ui| chrome_loading::draw_phases(ui, &theme));
    run_frames(|ui| chrome_loading::draw_no_text(ui, &theme));
    run_frames(|ui| chrome_loading::draw_latte(ui, &theme));
    run_frames(|ui| chrome_loading::draw_shutdown_default(ui, &theme));
    run_frames(|ui| chrome_loading::draw_shutdown_phases(ui, &theme));
}

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

// ── 신규 specimen (modal / popup / chrome) ──────────────────────────

#[test]
fn 신규_오버레이_specimen_은_헤드리스로_렌더된다() {
    use tasty_gallery::catalog::components::{
        drop_overlay, info_modal, notification_panel, quit_modal, script_confirm,
    };
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| {
        notification_panel::draw(ui, &theme);
        info_modal::draw(ui, &theme);
        script_confirm::draw(ui, &theme);
        quit_modal::draw(ui, &theme);
        drop_overlay::draw(ui, &theme);
    });
}

#[test]
fn 신규_크롬_specimen_은_헤드리스로_렌더된다() {
    use tasty_gallery::catalog::components::{empty_surface, plugins_window, titlebar};
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| {
        titlebar::draw(ui, &theme);
        empty_surface::draw(ui, &theme);
        plugins_window::draw(ui, &theme);
    });
}

#[test]
fn layout_shell_specimen_은_헤드리스로_렌더된다() {
    // 공용 위젯(two_depth_layout 계열)을 직접 호출하는 경로 — thread_local 상태가
    // 프레임을 넘어 유지되므로 run_frames 의 다중 프레임이 이중 borrow 를 잡는다.
    use tasty_gallery::catalog::components::prim_layout_shell;
    let theme = tasty_themes::mocha_fallback();
    run_frames(|ui| prim_layout_shell::draw(ui, &theme));
}
