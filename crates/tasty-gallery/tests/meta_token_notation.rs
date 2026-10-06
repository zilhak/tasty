//! 갤러리가 그린 글자에 CSS 변수 접두 `--tasty-`가 남지 않았는지 검사한다.
//! 갤러리 UI 글꼴은 하이픈 두 개를 붙여 그려 `--tasty-bg-app`이 `-tasty-bg-app`처럼 읽힌다.
//! 그래서 Meta·본문의 토큰 이름은 접두를 뗀 이름(`bg-app`)으로 쓴다
//! (docs/design/systems/design-gallery-mapping.md의 "토큰 이름 표기" 절).
//! 모든 Spec을 GPU 없이 한 프레임 그리고 출력 shape의 글자만 본다. 소스 주석은 보지 않는다.

use tasty_gallery::catalog::pages;

/// 그려진 글자에 나오면 안 되는 접두.
const FORBIDDEN_PREFIX: &str = "--tasty-";

fn collect_text(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
        egui::Shape::Vec(shapes) => {
            for inner in shapes {
                collect_text(inner, out);
            }
        }
        _ => {}
    }
}

#[test]
fn rendered_gallery_text_has_no_css_variable_prefix() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    tasty_gallery::fonts::install(&ctx);
    let mut hits = Vec::new();
    let mut drawn_specs = 0usize;
    let mut drawn_texts = 0usize;
    for page in pages() {
        for sec in &page.sections {
            if sec.title.contains(FORBIDDEN_PREFIX) {
                hits.push(format!("section {} title: {}", sec.id, sec.title));
            }
            for sp in &sec.specs {
                for label in std::iter::once(sp.title).chain(sp.when) {
                    if label.contains(FORBIDDEN_PREFIX) {
                        hits.push(format!("{} heading: {label}", sp.id));
                    }
                }
                let raw = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(4000.0, 20000.0),
                    )),
                    ..Default::default()
                };
                let output = ctx.run(raw, |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.push_id(sp.id, |ui| (sp.draw)(ui, &theme));
                    });
                });
                let mut texts = Vec::new();
                for clipped in &output.shapes {
                    collect_text(&clipped.shape, &mut texts);
                }
                drawn_specs += 1;
                drawn_texts += texts.len();
                hits.extend(
                    texts
                        .into_iter()
                        .filter(|t| t.contains(FORBIDDEN_PREFIX))
                        .map(|t| format!("{}: {t}", sp.id)),
                );
            }
        }
    }
    assert!(drawn_specs > 0, "그린 Spec이 없다");
    assert!(drawn_texts > 0, "그려진 글자를 하나도 모으지 못했다");
    assert!(
        hits.is_empty(),
        "그려진 글자 {}곳에 `{FORBIDDEN_PREFIX}` 접두가 남았다. 접두를 뗀 토큰 이름으로 쓴다:\n{}",
        hits.len(),
        hits.join("\n")
    );
}
