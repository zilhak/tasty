//! 저장 원격 행 표지·레일 아바타 툴팁이 본체 row_tip처럼 그려지는지 hover 뒤 그린 글자로 잰다.

use super::*;

fn collect_text(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(text) => out.push(text.galley.text().to_owned()),
        egui::Shape::Vec(shapes) => shapes.iter().for_each(|s| collect_text(s, out)),
        _ => {}
    }
}

fn frame_text(out: &egui::FullOutput) -> String {
    let mut texts = Vec::new();
    for clipped in &out.shapes {
        collect_text(&clipped.shape, &mut texts);
    }
    texts.join("\n")
}

/// `draw`가 돌려준 rect 위에 포인터를 툴팁 지연보다 오래 두고, 그 전과 뒤에 그려진 글자를 돌려준다.
fn hover_text(
    draw: impl Fn(&mut egui::Ui, &Theme) -> Vec<egui::Rect>,
    at: usize,
) -> (String, String) {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    crate::fonts::install(&ctx);
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 800.0));
    let input = |time: f64, pointer: Option<egui::Pos2>| egui::RawInput {
        screen_rect: Some(screen),
        time: Some(time),
        events: pointer
            .map(|p| vec![egui::Event::PointerMoved(p)])
            .unwrap_or_default(),
        ..Default::default()
    };
    let mut target = Vec::new();
    let run = |raw: egui::RawInput, target: &mut Vec<egui::Rect>| {
        ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| *target = draw(ui, &theme));
        })
    };
    let before = frame_text(&run(input(0.0, None), &mut target));
    let rect = *target.get(at).expect("drawn target");
    let delay = theme.tooltip_delay().to_secs_f64();
    let mut last = run(input(0.1, Some(rect.center())), &mut target);
    for step in 1..4 {
        last = run(input(0.2 + delay + step as f64 * 0.1, None), &mut target);
    }
    (before, frame_text(&last))
}

/// 툴팁에 있어야 할 세 줄. 이유 문장의 인자는 프로필이 없어진 행은 옛 프로필 이름, 그 밖은 행 이름이다.
fn expected_lines(lang: &str, reason: RemoteRowReason) -> [String; 3] {
    let (profile, arg) = match reason {
        RemoteRowReason::ProfileMissing => ("host-old", "host-old"),
        _ => ("host", "tasty-core"),
    };
    [
        t_lang(lang, "attach.mirror_workspace").to_owned(),
        format!("{profile} \u{2192} tasty-core"),
        t_lang(lang, reason.key()).replacen("{}", arg, 1),
    ]
}

fn assert_tip(label: &str, before: &str, after: &str, lang: &str, reason: RemoteRowReason) {
    let lines = expected_lines(lang, reason);
    assert!(
        !before.contains(&lines[2]),
        "{label}: the reason shows before hover"
    );
    for line in &lines {
        assert!(
            after.contains(line.as_str()),
            "{label}: no {line:?} in {after:?}"
        );
    }
    assert!(
        !after.contains("{}"),
        "{label}: a template slot is left in {after:?}"
    );
}

#[test]
fn the_row_mark_tooltip_names_the_target_and_fills_the_reason() {
    let reasons: Vec<_> = ROWS
        .iter()
        .filter_map(|(_, s, r)| r.map(|r| (*s, r)))
        .collect();
    assert_eq!(reasons.len(), 2, "two reason rows");
    for lang in langs() {
        for &(state, reason) in &reasons {
            let (before, after) = hover_text(
                |ui, theme| {
                    row(ui, theme, lang, state, Some(reason), false)
                        .into_iter()
                        .collect()
                },
                0,
            );
            assert_tip(
                &format!("row {lang} {reason:?}"),
                &before,
                &after,
                lang,
                reason,
            );
        }
    }
}

#[test]
fn the_rail_avatar_tooltip_matches_the_row_mark() {
    let reasons: Vec<_> = ROWS.iter().filter_map(|(_, _, r)| *r).collect();
    for (i, reason) in reasons.into_iter().enumerate() {
        let (before, after) = hover_text(rail, i);
        assert_tip(&format!("rail {reason:?}"), &before, &after, "en", reason);
    }
}
