use super::*;
use crate::view::plugins::ui::text_probe::visible_text_rects;

fn entry(kind: AttentionKind) -> AttentionEntry {
    AttentionEntry {
        id: "com.example.log-tailer".into(),
        name: "Log tailer".into(),
        version: "0.1.4".into(),
        authors: vec!["example".into()],
        builtin: false,
        kind,
        fingerprint: None,
        permissions_added: (0..40).map(|i| format!("perm:{i}")).collect(),
        permissions_removed: Vec::new(),
        health_detail: None,
        cause: None,
    }
}

fn draw(entry: AttentionEntry) -> Vec<(String, egui::Rect)> {
    let snapshot = PluginsSnapshot {
        attention: vec![entry],
        ..Default::default()
    };
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(720.0, 480.0));
    let ctx = egui::Context::default();
    let mut ui_state = PluginsUiState::default();
    let mut actions = Vec::new();
    let mut output = None;
    for _ in 0..2 {
        let raw = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        output = Some(ctx.run(raw, |ctx| {
            draw_attention_tab(ctx, &snapshot, &mut ui_state, &mut actions);
        }));
    }
    visible_text_rects(&output.expect("drawn"), screen)
}

/// Attention 의 액션 바도 Installed 처럼 본문 스크롤 밖 열 바닥에 붙어, 상세가 길어도 상태 문구와
/// 사유별 버튼이 창 안에 보인다. 정체 블록의 메타 줄은 `작성자 · id` 다.
#[test]
fn the_attention_bar_stays_inside_the_window_below_a_long_detail() {
    crate::i18n::init("en");
    let texts = draw(entry(AttentionKind::PermissionsChanged));
    let has = |label: &str| texts.iter().any(|(t, _)| t == label);
    for label in [
        t("plugins.attn_needs_review"),
        t("plugins.attn_reapprove"),
        "example",
        "com.example.log-tailer",
    ] {
        assert!(
            has(label),
            "{label:?} is not wholly visible inside the window"
        );
    }
}
