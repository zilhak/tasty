//! `surfaces` specimen — Surface focus states (research §2.5 Layouts).
//!
//! 터미널 surface 가 포커스 상태에 따라 어떻게 보이는지. 디자인 3 상태:
//! - **focused**: bg = `surface("terminal").focused_bg`(#000), 선명.
//! - **unfocused**: bg = `unfocused_bg`, opacity 0.92 로 살짝 가라앉음.
//! - **agent**: focused 와 같은 bg + pulsing `accent-agent` dot 으로 에이전트 점유 표시.
//!
//! fakePane = 헤더(StatusDot + Tag) + 프롬프트(mono + blink 커서 8×15).
//! 본체 view 변경 시 시각 동기화는 수동 (gallery 는 binary 미의존).
//!
//! **focus 와 attention 의 관계** (완료/응답대기 테두리는
//! [`occupancy_borders`](super::occupancy_borders) 참고): surface 가 여기서 그리는
//! `focused` 상태를 **실제로** 얻으면(에이전트 주입이 아닌 실 사용자 포커스, `gpu.rs`)
//! `AttentionStore` 의 그 surface 레코드가 kind 무관하게 clear 된다 — `Completion` 이든
//! `NeedsInput` 이든 이 해제 경로는 kind 를 구분하지 않는 단일 규칙이다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Focused,
    Unfocused,
    Agent,
}

/// 디자인 fakePane — 헤더(dot+tag) + 프롬프트 한 줄 + blink 커서.
fn fake_pane(ui: &mut egui::Ui, theme: &Theme, state: State) {
    let term = theme.surface("terminal");
    let w = theme.field_width_lg.value(); // 200
    let h = theme.spacing_xl.value() * 6.0; // 144
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);

    // 배경: focused/agent = focused_bg(#000), unfocused = unfocused_bg + 0.92 dim.
    // unfocused 표면 배경 디밍. 대응 토큰 없음.
    const UNFOCUSED_DIM_OPACITY: f32 = 0.92;
    let bg = match state {
        State::Focused | State::Agent => egui::Color32::from(term.focused_bg),
        State::Unfocused => {
            egui::Color32::from(term.unfocused_bg).gamma_multiply(UNFOCUSED_DIM_OPACITY)
        }
    };
    let fg = match state {
        State::Unfocused => egui::Color32::from(term.unfocused_fg),
        _ => egui::Color32::from(term.focused_fg),
    };
    p.rect_filled(rect, theme.corner_radius.value(), bg);
    p.rect_stroke(
        rect,
        theme.corner_radius.value(),
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.border_default()),
        ),
        egui::StrokeKind::Inside,
    );

    let pad = theme.spacing_sm.value();
    // 헤더: status dot + tag.
    let dot_r = theme.status_dot_size.value() * 0.5;
    let dot_c = egui::pos2(rect.min.x + pad + dot_r, rect.min.y + pad + dot_r);
    let (dot, tag) = match state {
        State::Focused => (egui::Color32::from(theme.accent_success()), "focused"),
        State::Unfocused => (egui::Color32::from(theme.text_muted()), "idle"),
        State::Agent => (egui::Color32::from(theme.accent_agent()), "agent"),
    };
    p.circle_filled(dot_c, dot_r, dot);
    p.text(
        egui::pos2(dot_c.x + dot_r + theme.spacing_xs.value(), rect.min.y + pad),
        egui::Align2::LEFT_TOP,
        tag,
        egui::FontId::proportional(theme.font_size_micro.value()),
        match state {
            State::Unfocused => egui::Color32::from(theme.text_muted()),
            _ => fg,
        },
    );

    // 프롬프트: mono 한 줄 + blink 커서 블록 8×15.
    let prompt_y = rect.min.y + pad + theme.status_dot_size.value() + theme.spacing_md.value();
    let prompt = "$ cargo build";
    let font = egui::FontId::monospace(theme.font_size_term_sm.value());
    let galley = p.layout_no_wrap(prompt.to_string(), font.clone(), fg);
    p.galley(egui::pos2(rect.min.x + pad, prompt_y), galley.clone(), fg);

    // 커서 블록: 8×15 (status_dot_size × spacing_lg 근사).
    let cur_x = rect.min.x + pad + galley.size().x + theme.spacing_xs.value();
    let cur_rect = egui::Rect::from_min_size(
        egui::pos2(cur_x, prompt_y),
        egui::vec2(theme.status_dot_size.value(), theme.spacing_lg.value()),
    );
    match state {
        State::Unfocused => {
            // 비포커스: 빈 커서(테두리만).
            p.rect_stroke(
                cur_rect,
                0.0,
                egui::Stroke::new(theme.border_width.value(), fg),
                egui::StrokeKind::Inside,
            );
        }
        State::Agent => {
            p.rect_filled(cur_rect, 0.0, egui::Color32::from(theme.accent_agent()));
        }
        State::Focused => {
            p.rect_filled(cur_rect, 0.0, fg);
        }
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "focused", |ui| {
            fake_pane(ui, theme, State::Focused)
        });
        spec::cluster(ui, theme, "unfocused · 0.92", |ui| {
            fake_pane(ui, theme, State::Unfocused)
        });
        spec::cluster(ui, theme, "agent", |ui| fake_pane(ui, theme, State::Agent));
    });

    spec::meta(
        ui,
        theme,
        &[
            ("focused bg", "#000 (terminal.focused_bg)"),
            ("unfocused bg", "unfocused_bg · opacity 0.92"),
            ("agent", "pulsing accent-agent dot"),
            ("header", "StatusDot + Tag"),
            ("cursor", "blink block 8×15"),
            (
                "attention on focus",
                "clear_attention — kind(Completion/NeedsInput) 무관 단일 해제 규칙",
            ),
        ],
        &[
            TokenChip::new(
                "terminal.focused-bg",
                "focused fill",
                theme.surface("terminal").focused_bg.into(),
            ),
            TokenChip::new(
                "terminal.unfocused-bg",
                "unfocused fill",
                theme.surface("terminal").unfocused_bg.into(),
            ),
            TokenChip::new("accent-agent", "agent dot", theme.accent_agent().into()),
            TokenChip::new(
                "accent-success",
                "focused dot",
                theme.accent_success().into(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "포커스된 surface 만 순흑(#000) — 나머지는 0.92 로 살짝 가라앉혀 \
         '지금 어디에 입력되는가' 를 한눈에 구분한다. agent 점유는 별도 색 dot 으로. \
         이 focused 전환은 완료/응답대기 테두리(occupancy_borders specimen 의 \
         completed·needs-input 클러스터) 해제와도 맞물린다 — surface 가 실 포커스를 \
         얻으면 그 attention 레코드가 kind(Completion·NeedsInput) 와 무관하게 지워진다.",
    );
}
