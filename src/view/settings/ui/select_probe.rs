//! 설정 화면을 헤드리스 egui 로 그려 Select 트리거를 찾고 클릭을 흉내 낸다. 탭별 시험이 함께 쓴다.

use crate::view::plugins::ui::text_probe::visible_text_rects;

pub const SCREEN: egui::Rect = egui::Rect {
    min: egui::Pos2::ZERO,
    max: egui::pos2(900.0, 900.0),
};

/// 입력 없이 한 프레임을 그린다.
pub fn frame(ctx: &egui::Context, draw: &mut dyn FnMut(&mut egui::Ui)) -> egui::FullOutput {
    run(ctx, Vec::new(), draw)
}

fn run(
    ctx: &egui::Context,
    events: Vec<egui::Event>,
    draw: &mut dyn FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    ctx.run(
        egui::RawInput {
            screen_rect: Some(SCREEN),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| draw(ui));
        },
    )
}

/// `pos` 를 누르는 프레임과 떼는 프레임을 그린 뒤, 새로 연 팝업이 보이도록 두 프레임을 더 그린다.
pub fn click(
    ctx: &egui::Context,
    pos: egui::Pos2,
    draw: &mut dyn FnMut(&mut egui::Ui),
) -> egui::FullOutput {
    let press = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    run(ctx, vec![egui::Event::PointerMoved(pos), press(true)], draw);
    run(ctx, vec![press(false)], draw);
    frame(ctx, draw);
    frame(ctx, draw)
}

pub fn text_rects(output: &egui::FullOutput, text: &str) -> Vec<egui::Rect> {
    visible_text_rects(output, SCREEN)
        .into_iter()
        .filter(|(s, _)| s == text)
        .map(|(_, r)| r)
        .collect()
}

/// `text` 를 담은 가장 작은 Select 트리거 상자(select-bg 채움). 없으면 `None`.
pub fn trigger_around(output: &egui::FullOutput, text: &str) -> Option<egui::Rect> {
    let th = crate::theme::theme();
    let fill = th.select_bg().to_egui();
    let label = *text_rects(output, text).first()?;
    let mut boxes = Vec::new();
    fn walk(shape: &egui::Shape, fill: egui::Color32, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| walk(s, fill, out)),
            egui::Shape::Rect(r) if r.fill == fill => out.push(r.rect),
            _ => {}
        }
    }
    for clipped in &output.shapes {
        walk(&clipped.shape, fill, &mut boxes);
    }
    boxes
        .into_iter()
        .filter(|b| b.contains(label.center()))
        .min_by(|a, b| a.area().total_cmp(&b.area()))
}

/// 트리거를 눌러 목록을 열고 `option` 행을 누른다. 끝난 뒤의 출력을 돌려준다.
pub fn pick(
    ctx: &egui::Context,
    draw: &mut dyn FnMut(&mut egui::Ui),
    trigger_text: &str,
    option: &str,
) -> egui::FullOutput {
    frame(ctx, draw);
    let out = frame(ctx, draw);
    let trigger = trigger_around(&out, trigger_text)
        .unwrap_or_else(|| panic!("no Select trigger around {trigger_text:?}"));
    let opened = click(ctx, trigger.center(), draw);
    let row = *text_rects(&opened, option)
        .iter()
        .find(|r| !trigger.contains(r.center()))
        .unwrap_or_else(|| panic!("option {option:?} not in the open list"));
    click(ctx, row.center(), draw)
}
