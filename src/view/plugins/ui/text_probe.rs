//! 헤드리스 egui 로 그린 창에서 사용자에게 보이는 글자를 읽는다. 배치 시험이 함께 쓴다.

/// 창 안에 그려진 글자 사각형들. 클립 밖으로 나간 글자는 사용자에게 보이지 않으므로 뺀다.
pub(super) fn visible_text_rects(
    output: &egui::FullOutput,
    screen: egui::Rect,
) -> Vec<(String, egui::Rect)> {
    fn walk(
        shape: &egui::Shape,
        clip: egui::Rect,
        screen: egui::Rect,
        out: &mut Vec<(String, egui::Rect)>,
    ) {
        match shape {
            egui::Shape::Vec(shapes) => {
                for s in shapes {
                    walk(s, clip, screen, out);
                }
            }
            egui::Shape::Text(text) => {
                let rect = text.galley.rect.translate(text.pos.to_vec2());
                if clip.contains_rect(rect) && screen.contains_rect(rect) {
                    out.push((text.galley.text().to_string(), rect));
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, clipped.clip_rect, screen, &mut out);
    }
    out
}
