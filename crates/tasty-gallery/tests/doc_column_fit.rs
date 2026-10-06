//! 모든 Spec이 문서 칸(시안 `.g-page` 최대 1080 − 좌우 40 = 1000) 안에 들어가는지 검사한다.
//! GPU 없이 egui 프레임을 몇 번 돌려 Spec 하나가 차지한 폭을 잰다. 줄바꿈 줄이 직전 프레임의
//! 크기로 자리를 잡으므로 한 프레임만으로는 판정하지 않는다. 픽셀이나 세로 넘침은 보지 않는다.

// 렌더 결과(FullOutput)는 쓰지 않는다. 폭은 Ui에서 직접 읽는다.
#![allow(clippy::let_underscore_must_use)]

use tasty_gallery::catalog::pages;

/// 문서 칸 폭. 갤러리 `host_shell`의 본문 최대 폭 1080에서 좌우 여백 40을 뺀 값이다.
const DOC_COLUMN_W: f32 = 1000.0;
/// 반올림 오차 허용치.
const TOLERANCE: f32 = 0.5;
/// note·meta 가 줄바꿈 폭으로 읽는 temp-data 키(`spec::body_column_width_id`와 같은 값).
const BODY_COLUMN_KEY: &str = "g_body_column_width";

/// 문서 칸보다 넓게 남겨 둔 Spec과 그 이유.
const ALLOWED_WIDE: &[(&str, &str)] = &[];

#[test]
fn every_spec_fits_the_doc_column() {
    let theme = tasty_themes::mocha_fallback();
    let ctx = egui::Context::default();
    tasty_gallery::fonts::install(&ctx);
    let mut wide = Vec::new();
    let mut stale: Vec<&str> = ALLOWED_WIDE.iter().map(|(id, _)| *id).collect();
    let mut measured = 0usize;
    for page in pages() {
        for sec in &page.sections {
            for sp in &sec.specs {
                let mut width = 0.0f32;
                for _ in 0..3 {
                    let raw = egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(DOC_COLUMN_W * 4.0, DOC_COLUMN_W * 20.0),
                        )),
                        ..Default::default()
                    };
                    // 렌더 출력은 쓰지 않는다. 폭은 Ui에서 직접 읽는다.
                    let _ = ctx.run(raw, |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.vertical(|ui| {
                                ui.set_max_width(DOC_COLUMN_W);
                                ui.data_mut(|d| {
                                    d.insert_temp(egui::Id::new(BODY_COLUMN_KEY), DOC_COLUMN_W)
                                });
                                ui.push_id(sp.id, |ui| (sp.draw)(ui, &theme));
                                width = ui.min_rect().width();
                            });
                        });
                    });
                }
                measured += 1;
                let allowed = ALLOWED_WIDE.iter().any(|(id, _)| *id == sp.id);
                let is_wide = width > DOC_COLUMN_W + TOLERANCE;
                if is_wide && !allowed {
                    wide.push(format!("{} ({:.0}px) — {}", sp.id, width, sp.title));
                }
                if is_wide && allowed {
                    stale.retain(|id| *id != sp.id);
                }
            }
        }
    }
    assert!(measured > 0, "잰 Spec이 없다");
    assert!(
        wide.is_empty(),
        "문서 칸 {DOC_COLUMN_W}px 보다 넓은 Spec {}개. 시안 stage 처럼 줄바꿈(spec::cluster · \
         spec::wrap_item)하거나 시안 폭으로 맞춘다:\n{}",
        wide.len(),
        wide.join("\n")
    );
    // 예외 목록은 지금도 넓은 Spec만 가리킨다. 고쳐졌거나 사라진 Spec이 남으면 실패한다.
    assert!(
        stale.is_empty(),
        "예외 목록에 넓지 않거나 없는 Spec이 남았다: {stale:?}"
    );
}
