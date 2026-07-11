//! specimen headless smoke — GPU 없이 egui 프레임을 돌려 specimen draw 가
//! 패닉(RefCell 이중 borrow·레이아웃 위반) 없이 렌더되는지 회귀 격리한다.
//! 픽셀 판정은 하지 않는다 — 시각 정합은 갤러리 육안 몫.

use tasty_gallery::catalog::components::{prim_drilldown, prim_listctrl};
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
