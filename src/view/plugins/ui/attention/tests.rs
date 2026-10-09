use super::*;
use crate::view::plugins::ui::text_probe::visible_text_rects;

fn entry(kind: AttentionKind) -> AttentionEntry {
    AttentionEntry {
        id: "com.example.log-tailer".into(),
        name: "Log tailer".into(),
        version: "0.1.4".into(),
        authors: vec!["example".into()],
        description: "Tails log files.".into(),
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

/// 상세는 정체 블록 · 설명 · 사유 배너 · 사유 detail 을 `space-lg` 간격으로 쌓는다. 배너 안 글은
/// 테두리와 `space-md` 여백 안쪽에 있으므로 글 사이 거리에서 그 몫을 뺀다. 메타 줄은 `작성자 · id`
/// 뿐이다.
#[test]
fn the_attention_detail_stacks_its_blocks_a_large_space_apart() {
    crate::i18n::init("en");
    let th = theme::theme();
    let texts = draw(entry(AttentionKind::PermissionsChanged));
    // 목록 행에도 사유 문구가 있으므로 설명과 같은 열(상세 열)에서 시작하는 글자만 본다.
    let detail_x = texts
        .iter()
        .find(|(t, _)| t == "Tails log files.")
        .map(|(_, r)| r.min.x)
        .expect("description is drawn");
    let rect = |label: &str| {
        texts
            .iter()
            .find(|(t, r)| t.eq_ignore_ascii_case(label) && r.min.x >= detail_x - 0.5)
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("{label:?} is not drawn"))
    };
    let inset = th.border_width.value() + th.spacing_md.value();
    let lg = th.spacing_lg.value();
    let description = rect("Tails log files.");
    let title = rect(t("plugins.attn_perm_changed_label"));
    let blurb = rect(t("plugins.attn_perm_changed_blurb"));
    let header = rect(t("plugins.attn_permission_changes"));
    for (what, gap, want) in [
        (
            "description → banner",
            title.min.y - description.max.y,
            lg + inset,
        ),
        ("banner → detail", header.min.y - blurb.max.y, inset + lg),
    ] {
        assert!((gap - want).abs() < 0.5, "{what}: {gap} != {want}");
    }
    assert!(
        !texts.iter().any(|(t, _)| t.contains("https://")),
        "the attention meta line has no homepage"
    );
}

/// 서명이 깨진 번들의 매니페스트 글은 보이지 않는다. 나머지 사유는 보인다.
#[test]
fn only_a_broken_signature_hides_the_manifest_text() {
    assert!(!AttentionKind::SignatureInvalid.shows_manifest_text());
    for kind in [
        AttentionKind::UnknownKey,
        AttentionKind::PermissionsChanged,
        AttentionKind::HealthError,
    ] {
        assert!(kind.shows_manifest_text(), "{kind:?}");
    }
}
