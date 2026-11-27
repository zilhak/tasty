//! 갤러리 튜토리얼 표본이 본체 basics 주제와 같은 단계 수·순서를 보이는지 본체 소스를 읽어 대조한다.

use super::{BASICS_STEPS, COMPOSITE_NOTE, COMPOSITE_WHEN};

fn app_source(rel: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 을 읽지 못했다: {e}", path.display()))
}

const APP_CATALOG: &str = "src/adapters/ui/tutorial/catalog.rs";

/// `src[start..]`의 원소를 최상위 쉼표로 나눠 `(첫 글자 바이트, 주석을 뺀 원소 글)`로 돌려준다. 여는 괄호
/// 없이 시작해 짝 없는 닫는 `]`에서 멈춘다. 문자열 리터럴 안의 괄호·쉼표는 세지 않고, 줄 주석과
/// (중첩 포함) 블록 주석은 원소 글과 위치에서 뺀다. 위치는 주석 밖 첫 글자가 원문에 놓인 바이트다.
fn top_level_elements(src: &str, start: usize) -> Vec<(usize, String)> {
    let b = src.as_bytes();
    let (mut i, mut depth) = (start, 0usize);
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut first: Option<usize> = None;
    let flush = |cur: &mut String, first: &mut Option<usize>, out: &mut Vec<(usize, String)>| {
        if let Some(at) = first.take() {
            out.push((at, cur.trim().to_owned()));
        }
        cur.clear();
    };
    while i < b.len() {
        let c = b[i];
        if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
            continue;
        }
        if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let mut nest = 1;
            i += 2;
            while i < b.len() && nest > 0 {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    nest += 1;
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    nest -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            cur.push(' ');
            continue;
        }
        if c == b']' && depth == 0 {
            flush(&mut cur, &mut first, &mut out);
            return out;
        }
        if c == b',' && depth == 0 {
            flush(&mut cur, &mut first, &mut out);
            i += 1;
            continue;
        }
        let from = i;
        if c == b'"' {
            i += 1;
            while i < b.len() && b[i] != b'"' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i += 1;
        } else {
            match c {
                b'(' | b'[' | b'{' => depth += 1,
                b')' | b']' | b'}' => depth -= 1,
                _ => {}
            }
            i += 1;
        }
        while i < b.len() && !src.is_char_boundary(i) {
            i += 1;
        }
        let i_end = i.min(b.len());
        if first.is_none() && !c.is_ascii_whitespace() {
            first = Some(from);
        }
        cur.push_str(&src[from..i_end]);
    }
    panic!("{APP_CATALOG}: basics steps 목록이 닫히지 않았다");
}

/// 본체 catalog의 basics 주제에서 `step!("<id>", <Target>, …)` 원소를 순서대로 꺼낸다. 이 꼴이 아닌
/// 원소(구조체 리터럴, 다른 괄호의 매크로 등)가 하나라도 있으면 원소와 위치를 담아 실패한다.
fn app_basics_steps() -> Vec<(String, String)> {
    parse_basics_steps(&app_source(APP_CATALOG))
}

/// `src`(본체 catalog 꼴의 소스)에서 basics 단계를 꺼낸다. 규칙은 `app_basics_steps`와 같다.
fn parse_basics_steps(src: &str) -> Vec<(String, String)> {
    let topic = src
        .find("id: \"basics\"")
        .expect("본체 catalog에 basics 주제가 있다");
    let open =
        src[topic..].find("steps: &[").expect("basics steps 시작") + topic + "steps: &[".len();
    let mut steps = Vec::new();
    for (at, element) in top_level_elements(src, open) {
        let line = src[..at].matches('\n').count() + 1;
        let args = element
            .strip_prefix("step!(")
            .and_then(|e| e.strip_suffix(')'))
            .unwrap_or_else(|| {
                panic!("{APP_CATALOG}:{line}: basics 단계가 step!(…) 꼴이 아니다: {element}")
            });
        let mut parts = args.splitn(3, ',');
        let id = parts.next().unwrap_or("").trim();
        let target = parts.next().unwrap_or("").trim();
        let id = id
            .strip_prefix('"')
            .and_then(|i| i.strip_suffix('"'))
            .unwrap_or_else(|| {
                panic!("{APP_CATALOG}:{line}: 단계 id가 문자열 리터럴이 아니다: {element}")
            });
        assert!(
            !target.is_empty()
                && target
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "{APP_CATALOG}:{line}: 단계 대상이 MarkerTarget 이름이 아니다: {element}"
        );
        steps.push((id.to_owned(), target.to_owned()));
    }
    steps
}

#[test]
fn basics_steps_follow_the_app_topic() {
    let app = app_basics_steps();
    let ids: Vec<&str> = app.iter().map(|(id, _)| id.as_str()).collect();
    let gallery: Vec<&str> = BASICS_STEPS.iter().map(|s| s.id).collect();
    assert_eq!(
        gallery, ids,
        "갤러리 BASICS_STEPS가 본체 basics 순서와 다르다"
    );

    // 마지막 단계만 Summary 대상이고, 본체는 Summary에 마커 사각형을 두지 않는다.
    let summaries: Vec<usize> = app
        .iter()
        .enumerate()
        .filter(|(_, (_, target))| target == "Summary")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(summaries, vec![app.len() - 1]);
    assert!(
        app_source("src/adapters/ui/tutorial/mod.rs").contains("MarkerTarget::Summary => None"),
        "본체 summary 단계가 마커를 그리면 COMPOSITE_NOTE의 summary 서술을 고친다"
    );
}

#[test]
fn basics_step_keys_exist_in_app_lang() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lang/en.toml");
    let value: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut flat = std::collections::HashMap::new();
    tasty_i18n::flatten_catalog_toml("", &value, &mut flat);
    for step in &BASICS_STEPS {
        for key in [step.title_key, step.body_key] {
            assert!(flat.contains_key(key), "{key}");
        }
    }
}

#[test]
fn composite_texts_state_the_app_step_count_and_order() {
    let n = BASICS_STEPS.len();
    assert!(
        COMPOSITE_WHEN.starts_with(&format!("Step 1/{n} ")),
        "{COMPOSITE_WHEN}"
    );
    let middle: Vec<&str> = BASICS_STEPS[1..n - 1].iter().map(|s| s.id).collect();
    let order = middle.join(" → ");
    assert!(
        COMPOSITE_NOTE.contains(&format!("move the marker {order}:")),
        "{COMPOSITE_NOTE}"
    );
    // 탭 헤더(페인 상단 띠)와 서피스(탭 바 아래)는 서로를 담지 않는다. 마커는 옮겨 갈 뿐이다.
    assert!(!COMPOSITE_NOTE.contains("containment"), "{COMPOSITE_NOTE}");
    assert!(
        COMPOSITE_NOTE.contains(&format!("last step ({})", BASICS_STEPS[n - 1].id)),
        "{COMPOSITE_NOTE}"
    );
}

/// 합성 표본을 GPU 없이 한 프레임 그려 글자 모양을 모은다.
fn composite_texts() -> Vec<String> {
    fn collect(shape: &egui::epaint::Shape, out: &mut Vec<String>) {
        match shape {
            egui::epaint::Shape::Text(t) => out.push(t.galley.text().to_owned()),
            egui::epaint::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
            _ => {}
        }
    }
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let output = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| super::draw_composite(ui, &theme));
    });
    let mut out = Vec::new();
    for cs in &output.shapes {
        collect(&cs.shape, &mut out);
    }
    out
}

#[test]
fn composite_draws_the_first_basics_step_callout() {
    let texts = composite_texts();
    let progress = format!("1 / {}", BASICS_STEPS.len());
    assert!(texts.contains(&progress), "{progress} 없음: {texts:?}");
    let title = crate::i18n::t(BASICS_STEPS[0].title_key);
    assert!(texts.iter().any(|t| t == title), "{title} 없음: {texts:?}");
}

/// 갤러리 본문 열과 같은 꼴로 합성 표본을 한 프레임 그린다. host_shell의 main_ui처럼 열 폭 `col_w`를
/// 정하고 본문 열 폭으로 저장한 뒤, `widen`이면 그 앞에 주제 팝업 표본(고정 폭 무대)을 그려 열의 Ui를
/// 넓힌다. (표본 결과, 보이는 열의 오른쪽 끝 x, 그린 메시(페이드) 사각형들)을 돌려준다.
fn composite_frame(
    ctx: &egui::Context,
    theme: &tasty_type_appearance::theme::Theme,
    col_w: f32,
    widen: bool,
) -> (super::CompositeLayout, f32, Vec<egui::Rect>) {
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(col_w + 400.0, 3000.0),
        )),
        ..Default::default()
    };
    let mut got = None;
    let frame = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.set_max_width(col_w);
                ui.data_mut(|d| d.insert_temp(crate::catalog::spec::body_column_width_id(), col_w));
                let right = ui.max_rect().left() + col_w;
                if widen {
                    super::draw_topics(ui, theme);
                }
                got = Some((super::composite_specimen(ui, theme), right));
            });
        });
    });
    let (layout, right) = got.expect("합성 표본을 그렸다");
    (layout, right, mesh_bounds(&frame))
}

/// 한 프레임 출력에서 메시(페이드)의 사각형을 모은다.
fn mesh_bounds(frame: &egui::FullOutput) -> Vec<egui::Rect> {
    frame
        .shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::epaint::Shape::Mesh(m) => Some(m.calc_bounds()),
            _ => None,
        })
        .collect()
}

/// `composite_frame`을 그리고, `to_end`면 가로 스크롤을 끝으로 옮겨 한 번 더 그린다.
fn composite_in_column(
    col_w: f32,
    widen: bool,
    to_end: bool,
) -> (super::CompositeLayout, f32, Vec<egui::Rect>) {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    let first = composite_frame(&ctx, &theme, col_w, widen);
    if !to_end {
        return first;
    }
    let layout = &first.0;
    let mut state =
        egui::scroll_area::State::load(&ctx, layout.scroll_id).expect("가로 스크롤 상태");
    state.offset.x = (layout.content.x - layout.viewport.width()).max(0.0);
    state.store(&ctx, layout.scroll_id);
    composite_frame(&ctx, &theme, col_w, widen)
}

fn floor() -> f32 {
    tasty_themes::mocha_fallback()
        .tutorial_step_specimen_min_width()
        .value()
}

#[test]
fn composite_stage_keeps_its_floor_and_holds_the_callout() {
    // 바닥보다 좁은 열: 무대는 바닥 폭이고 말풍선과 꼬리가 무대 안에 든다.
    let (l, _, _) = composite_in_column(floor() / 2.0, false, false);
    assert_eq!(l.stage.width(), floor(), "{:?}", l.stage);
    assert!(
        l.stage.contains_rect(l.bubble),
        "{:?} ⊅ {:?}",
        l.stage,
        l.bubble
    );
    // 넓은 열: 무대는 최대 폭에서 멈춘다.
    let (l, _, _) = composite_in_column(2000.0, false, false);
    assert_eq!(
        l.stage.width(),
        super::COMPOSITE_STAGE_MAX_W.value(),
        "{:?}",
        l.stage
    );
    assert!(
        l.stage.contains_rect(l.bubble),
        "{:?} ⊅ {:?}",
        l.stage,
        l.bubble
    );
}

#[test]
fn composite_stage_scrolls_horizontally_in_a_narrow_column() {
    let (l, _, _) = composite_in_column(floor() / 2.0, false, false);
    assert!(
        l.stage.width() > l.viewport.width() + 0.5,
        "무대 {:?}가 보이는 영역 {:?}보다 넓어야 한다",
        l.stage,
        l.viewport
    );
    assert_eq!(l.content.x, floor(), "스크롤 내용 폭 {:?}", l.content);
}

#[test]
fn composite_tail_is_visible_after_scrolling_to_the_end_under_a_widening_specimen() {
    // 주제 팝업 표본이 열의 Ui를 넓힌 뒤에도 보이는 영역은 열 안에 있고, 끝까지 스크롤하면 꼬리 끝이 보인다.
    let col_w = 300.0;
    assert!(col_w < floor());
    let (l, right, _) = composite_in_column(col_w, true, true);
    assert!(
        l.viewport.right() <= right + 0.5,
        "보이는 영역 {:?}가 열 끝 {right}을 넘는다",
        l.viewport
    );
    assert!(
        l.bubble.right() <= l.viewport.right() + 0.5,
        "끝까지 스크롤한 말풍선 {:?}의 꼬리가 보이는 영역 {:?} 밖이다",
        l.bubble,
        l.viewport
    );
}

#[test]
fn composite_fades_reach_the_content_clip_edge() {
    // 첫 화면은 오른쪽, 끝까지 스크롤한 화면은 왼쪽에만 페이드가 있다. 페이드의 불투명 끝은 무대 내용이
    // 실제로 그려지는 클립 경계와 같거나 그 바깥이어서, 페이드 바깥에 스크롤된 무대 띠가 보이지 않는다.
    for to_end in [false, true] {
        let (l, _, fades) = composite_in_column(floor() / 2.0, false, to_end);
        assert_eq!(fades.len(), 1, "끝={to_end}: 페이드 {fades:?}");
        let f = fades[0];
        let reaches = if to_end {
            f.left() <= l.clip.left() + 0.01
        } else {
            f.right() >= l.clip.right() - 0.01
        };
        assert!(
            reaches,
            "끝={to_end}: 페이드 {f:?}가 내용 클립 {:?}의 가장자리까지 덮지 않는다",
            l.clip
        );
    }
}

/// 진단 메시지의 `catalog.rs:<줄>:`을 꺼내 줄 번호를 돌려준다.
fn rejected_line(src: &str) -> usize {
    let err = std::panic::catch_unwind(|| parse_basics_steps(src)).expect_err("거부해야 한다");
    let msg = err.downcast_ref::<String>().cloned().unwrap_or_default();
    let rest = msg
        .strip_prefix(&format!("{APP_CATALOG}:"))
        .unwrap_or_else(|| panic!("진단에 파일·줄이 없다: {msg}"));
    rest.split(':').next().unwrap().parse().unwrap()
}

const FIXTURE_HEAD: &str = "    Topic {\n        id: \"basics\",\n        steps: &[\n            step!(\"workspace\", ContentArea, Read, None),\n";

#[test]
fn block_comments_before_a_step_are_skipped() {
    let src = format!(
        "{FIXTURE_HEAD}            /* summary는 말풍선만, ( [ , 를 담아도 */\n            step!(\"summary\", Summary, Read, None),\n        ],\n"
    );
    let ids: Vec<String> = parse_basics_steps(&src)
        .into_iter()
        .map(|(id, _)| id)
        .collect();
    assert_eq!(ids, ["workspace", "summary"]);
}

#[test]
fn a_rejected_step_under_a_block_comment_reports_its_own_line() {
    // 5행: 블록 주석, 6행: 거부할 원소.
    let src = format!(
        "{FIXTURE_HEAD}            /* 여러 줄\n            블록 주석 */ step![\"extra\", Summary, Read, None],\n        ],\n"
    );
    assert_eq!(rejected_line(&src), 6);
    let src = format!(
        "{FIXTURE_HEAD}            /* 한 줄 블록 주석 */\n            step![\"extra\", Summary, Read, None],\n        ],\n"
    );
    assert_eq!(rejected_line(&src), 6);
}

#[test]
fn composite_edge_fades_stay_on_the_stage() {
    // 바닥보다 좁은 열의 첫 프레임: 오른쪽으로 더 스크롤할 수 있어 오른쪽 페이드가 있다. 페이드(메시)는
    // 무대의 세로 범위를 그대로 덮는다(무대 아래로 넘지도, 높이 0으로 줄지도 않는다). 세로로 남은 가용
    // 높이가 큰 첫 번째 경우와, 긴 갤러리 페이지처럼 남은 높이가 0인 두 번째 경우를 함께 본다.
    let theme = tasty_themes::mocha_fallback();
    let col_w = floor() / 2.0;
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(col_w + 400.0, 3000.0),
        )),
        ..Default::default()
    };
    for no_height_left in [false, true] {
        edge_fades_cover_the_stage(&theme, col_w, input.clone(), no_height_left);
    }
}

fn edge_fades_cover_the_stage(
    theme: &tasty_type_appearance::theme::Theme,
    col_w: f32,
    input: egui::RawInput,
    no_height_left: bool,
) {
    let ctx = egui::Context::default();
    let mut stage = None;
    let output = ctx.run(input, |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                ui.set_max_width(col_w);
                if no_height_left {
                    ui.add_space(ui.available_height());
                }
                ui.data_mut(|d| d.insert_temp(crate::catalog::spec::body_column_width_id(), col_w));
                stage = Some(super::composite_specimen(ui, theme).stage);
            });
        });
    });
    let stage = stage.expect("합성 표본을 그렸다");
    let meshes: Vec<egui::Rect> = output
        .shapes
        .iter()
        .filter_map(|cs| match &cs.shape {
            egui::epaint::Shape::Mesh(m) => Some(m.calc_bounds()),
            _ => None,
        })
        .collect();
    assert!(!meshes.is_empty(), "오른쪽 페이드가 없다");
    for m in meshes {
        assert!(
            (m.top() - stage.top()).abs() <= 0.5 && (m.bottom() - stage.bottom()).abs() <= 0.5,
            "남은 높이 0={no_height_left}: 페이드 {m:?}의 세로 범위가 무대 {stage:?}와 같아야 한다"
        );
    }
}

/// `lang/<code>.toml` 을 점 키로 펼친 표. 갤러리 번역표는 프로세스에 한 번만 정해지므로 언어마다 따로 읽는다.
fn lang_table(code: &str) -> std::collections::HashMap<String, String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../lang")
        .join(format!("{code}.toml"));
    let value: toml::Value = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut flat = std::collections::HashMap::new();
    tasty_i18n::flatten_catalog_toml("", &value, &mut flat);
    flat
}

/// 표본 말풍선의 버튼 글자가 네 언어 모두 본체 [tutorial] lang 값이고, Topics ghost가 raised 잉크인지
/// 그린 글자로 본다. 영어로는 같은 값을 가진 다른 키(예: explorer.nav.back)와 갈리지 않으므로 언어마다 그린다.
#[test]
fn callout_buttons_read_the_app_tutorial_keys_and_the_raised_ghost_ink() {
    use crate::catalog::components::painted::{mocha_latte, paint};
    for code in ["en", "ko", "ja", "zh-Hans"] {
        let table = lang_table(code);
        let tr = |key: &'static str| -> &str {
            table
                .get(key)
                .map(String::as_str)
                .unwrap_or_else(|| panic!("{code}: {key} 없음"))
        };
        let (skip, back, next, done) = (
            tr("tutorial.btn_skip"),
            tr("tutorial.btn_back"),
            tr("tutorial.btn_next"),
            tr("tutorial.btn_done"),
        );
        for (name, th) in mocha_latte() {
            // (단계, 전체, 첫 단계): 가운데 · 첫 단계 · 마지막 단계.
            for (step, total, first) in [(2, 5, false), (1, 5, true), (5, 5, false)] {
                let painted = paint(|ui| {
                    super::callout(ui, &th, super::Tail::Up, step, total, "T", "B", first, &tr);
                });
                let count = |s: &str| painted.texts.iter().filter(|p| p.text == s).count();
                let case = format!("{code} {name} {step}/{total}");
                assert_eq!(count(back), usize::from(!first), "{case} {back}");
                assert_eq!(count(next), usize::from(step != total), "{case} {next}");
                assert_eq!(count(done), usize::from(step == total), "{case} {done}");
                let topics: Vec<_> = painted.texts.iter().filter(|p| p.text == skip).collect();
                assert_eq!(topics.len(), 1, "{case} {skip}");
                assert_eq!(
                    topics[0].color,
                    th.button_ghost_raised_fg().to_egui(),
                    "{case} {skip}"
                );
            }
        }
    }
}

/// 갤러리 표본(줄바꿈 묶음 안)에서 말풍선 안이 세로로 쌓이는지 그린 도형으로 본다. 말풍선마다 같은 말풍선의
/// 버튼 줄(버튼 배경 Rect의 합)이 있고 그 줄이 본문 Rect와 겹치지 않는지를 먼저 보고, 그다음 제목이 본문 위에
/// 있는지 본다.
#[test]
fn callout_specimen_stacks_the_body_above_the_button_row() {
    use crate::catalog::components::painted::{mocha_latte, paint};
    use crate::i18n::t;
    let buttons = [
        t("tutorial.btn_skip"),
        t("tutorial.btn_back"),
        t("tutorial.btn_next"),
    ];
    for (name, th) in mocha_latte() {
        let painted = paint(|ui| super::draw_callout(ui, &th));
        let rect_of = |s: &str| -> Vec<egui::Rect> {
            painted
                .texts
                .iter()
                .filter(|p| p.text == s)
                .map(|p| p.rect)
                .collect()
        };
        // 그림자를 뺀 채움 사각형.
        let fills: Vec<(egui::Rect, egui::Color32)> = painted
            .shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Rect(r) if r.blur_width == 0.0 && r.brush.is_none() => {
                    Some((r.rect, r.fill))
                }
                _ => None,
            })
            .collect();
        let raised = egui::Color32::from(th.surface_raised());
        let button_bgs = [
            egui::Color32::from(th.button_primary_bg()),
            egui::Color32::from(th.button_secondary_bg()),
        ];
        let labels: Vec<egui::Rect> = buttons.iter().flat_map(|b| rect_of(b)).collect();
        let mut bodies = 0;
        for step in BASICS_STEPS.iter().filter(|s| s.id != "summary") {
            for body in rect_of(t(step.body_key)) {
                bodies += 1;
                // 이 본문을 품은 가장 작은 surface-raised 채움이 그 말풍선 프레임이다.
                let frame = fills
                    .iter()
                    .filter(|(r, c)| *c == raised && r.contains_rect(body))
                    .map(|(r, _)| *r)
                    .min_by(|a, b| a.area().total_cmp(&b.area()))
                    .unwrap_or_else(|| panic!("{name} {}: 말풍선 프레임 없음", step.id));
                // 같은 말풍선의 버튼 줄: 프레임 안에서 버튼 글자를 품은 버튼 배경 Rect의 합.
                let row = fills
                    .iter()
                    .filter(|(r, c)| button_bgs.contains(c) && *r != frame)
                    .filter(|(r, _)| frame.contains_rect(*r))
                    .filter(|(r, _)| labels.iter().any(|l| r.contains(l.center())))
                    .map(|(r, _)| *r)
                    .reduce(|a, b| a.union(b));
                let row = row.unwrap_or_else(|| {
                    panic!("{name} {}: 말풍선 {frame:?} 에 버튼 줄이 없다", step.id)
                });
                assert!(
                    !row.intersects(body),
                    "{name} {}: 버튼 줄 {row:?} 이 본문 {body:?} 과 겹친다",
                    step.id
                );
                for title in rect_of(t(step.title_key)) {
                    assert!(
                        title.bottom() <= body.top(),
                        "{name} {}: 제목 {title:?} 이 본문 {body:?} 위에 있지 않다",
                        step.id
                    );
                }
            }
        }
        assert_eq!(bodies, 4, "{name}: 표본 본문 수");
    }
}

/// 반투명 accent 의 알파. 정본 식은 accent 알파에 비율을 곱하므로, 불투명 accent 에서는 같은 값이 되는
/// 고정 알파 리터럴(40% = 102)도 이 테마에서는 갈린다.
const TRANSLUCENT_ACCENT_ALPHA: u8 = 128;

/// Mocha·Latte 와 accent 만 반투명인 Mocha.
fn marker_themes() -> Vec<(&'static str, tasty_type_appearance::theme::Theme)> {
    use crate::catalog::components::painted::mocha_latte;
    let mut colors = tasty_themes::mocha_fallback_colors();
    colors.blue = colors.blue.with_alpha(TRANSLUCENT_ACCENT_ALPHA);
    let translucent = tasty_type_appearance::theme::Theme::with_colors_and_zoom(colors, false, 1.0);
    assert_eq!(translucent.accent_primary().a(), TRANSLUCENT_ACCENT_ALPHA);
    let mut themes = mocha_latte().to_vec();
    themes.push(("translucent accent", translucent));
    themes
}

fn stroked_rects(shapes: &[egui::Shape]) -> Vec<&egui::epaint::RectShape> {
    shapes
        .iter()
        .filter_map(|s| match s {
            egui::Shape::Rect(r) if r.stroke.width > 0.0 => Some(r),
            _ => None,
        })
        .collect()
}

/// 마커 표본의 발광 두 겹이 정본 토큰 색인지 그린 도형으로 본다. 링(accent, 안쪽 stroke)을 기준으로 바깥에
/// 그린 stroke 가 발광이고, 링에서 더 멀리 벌린 겹이 glow-inner, 가까운 겹이 glow-outer 다(정본 box-shadow
/// 합성의 근사, `paint_marker`). 발광은 "glow + spotlight" 예제 하나에만 있다.
#[test]
fn marker_glow_layers_paint_the_design_glow_tokens() {
    use crate::catalog::components::painted::paint;
    for (name, th) in marker_themes() {
        let painted = paint(|ui| super::draw_marker(ui, &th));
        let strokes = stroked_rects(&painted.shapes);
        let accent = th.accent_primary().to_egui();
        let rings: Vec<egui::Rect> = strokes
            .iter()
            .filter(|r| r.stroke.color == accent && r.stroke_kind == egui::StrokeKind::Inside)
            .map(|r| r.rect)
            .collect();
        assert_eq!(rings.len(), 2, "{name}: 마커 링 수");
        // 링과 중심이 같고 더 큰 바깥 stroke 를 그 링의 발광 겹으로 본다. 벌린 폭은 0.1px 단위로 읽는다.
        let mut glows: Vec<(f32, egui::Color32)> = Vec::new();
        for s in strokes
            .iter()
            .filter(|r| r.stroke_kind == egui::StrokeKind::Outside)
        {
            for ring in &rings {
                let grow = ((s.rect.width() - ring.width()) / 2.0 * 10.0).round() / 10.0;
                let same_center = (s.rect.center() - ring.center()).length() < 0.01;
                let same_grow = ((s.rect.height() - ring.height()) / 2.0 - grow).abs() < 0.05;
                if same_center && same_grow && grow > 0.0 {
                    glows.push((grow, s.stroke.color));
                }
            }
        }
        glows.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(
            glows,
            vec![
                (2.5, th.tutorial_marker_glow_outer().to_egui()),
                (5.0, th.tutorial_marker_glow_inner().to_egui()),
            ],
            "{name}: 발광 겹(벌린 폭, 색)"
        );
    }
}

/// 선택한 주제 행(surface-active 채움)의 테두리가 정본 토큰 색인지 그린 도형으로 본다.
#[test]
fn selected_topic_row_border_paints_the_design_token() {
    use crate::catalog::components::painted::paint;
    for (name, th) in marker_themes() {
        let active = th.surface_active().to_egui();
        let selected = paint(|ui| super::topic_row(ui, &th, 1, "T", "D", true, false));
        let frames: Vec<_> = stroked_rects(&selected.shapes)
            .into_iter()
            .filter(|r| r.fill == active)
            .map(|r| r.stroke.color)
            .collect();
        assert_eq!(
            frames,
            vec![th.tutorial_topic_selected_border().to_egui()],
            "{name}: 선택 행 테두리"
        );
    }
}
