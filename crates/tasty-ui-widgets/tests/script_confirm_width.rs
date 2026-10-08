//! 스크립트 확인 콘텐츠가 주어진 폭을 넘지 않는지 검사한다. 안내문이 줄바꿈하지 않으면
//! 버튼 행이 넓어진 폭의 오른쪽 끝에 붙어 popup clip 밖으로 밀린다.

use egui::{Pos2, RawInput, Rect, vec2};
use tasty_ui_widgets::{ScriptConfirmView, script_confirm};

#[test]
fn the_content_and_its_buttons_stay_within_the_given_width() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let width = 240.0;
    let mut drawn = Rect::NOTHING;
    // 첫 프레임은 폰트 준비 전이라 두 번 돌린다.
    for _ in 0..2 {
        drop(ctx.run(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let left = ui.cursor().left();
                    drawn = ui
                        .allocate_ui(vec2(width, 600.0), |ui| {
                            script_confirm(
                                ui,
                                &theme,
                                &ScriptConfirmView {
                                    title: "Script changed since registration",
                                    name: "deploy.lua",
                                    changed_tag: "changed",
                                    body: "Review it, then run the new version. Its recorded \
                                           hash will be updated.",
                                    run: "Run anyway",
                                    cancel: "Cancel",
                                },
                            );
                        })
                        .response
                        .rect;
                    drawn = drawn.translate(vec2(-left, 0.0));
                });
            },
        ));
    }
    assert!(
        drawn.max.x <= width,
        "the content reaches x = {} in a {width} wide area",
        drawn.max.x
    );
}
