//! 터미널의 포커스·비포커스·에이전트 상태를 정적으로 비교한다.
//! 본체에서 실제 사용자 포커스를 받으면 Completion과 NeedsInput을 모두 해제한다.
//! 이 예제 자체는 포커스 전환이나 알림 상태 변경을 실행하지 않는다.

use tasty_type_appearance::theme::Theme;

use crate::catalog::spec::{self, StageVariant, TokenChip};

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Focused,
    Unfocused,
    Agent,
}

fn fake_pane(ui: &mut egui::Ui, theme: &Theme, state: State) {
    let term = theme.surface("terminal");
    let w = theme.field_width_lg.value(); // 200
    let h = theme.spacing_xl.value() * 6.0; // 144
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    let p = ui.painter_at(rect);

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

    let prompt_y = rect.min.y + pad + theme.status_dot_size.value() + theme.spacing_md.value();
    let prompt = "$ cargo build";
    let font = egui::FontId::monospace(theme.font_size_term_sm.value());
    let galley = p.layout_no_wrap(prompt.to_string(), font.clone(), fg);
    p.galley(egui::pos2(rect.min.x + pad, prompt_y), galley.clone(), fg);

    let cur_x = rect.min.x + pad + galley.size().x + theme.spacing_xs.value();
    let cur_rect = egui::Rect::from_min_size(
        egui::pos2(cur_x, prompt_y),
        egui::vec2(theme.status_dot_size.value(), theme.spacing_lg.value()),
    );
    match state {
        State::Unfocused => {
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
            ("agent", "static accent-agent dot in this example"),
            ("header", "painted dot + state label"),
            ("cursor", "static block from status-dot-size × spacing-lg"),
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
        "이 예제는 테마의 포커스·비포커스 터미널 배경을 사용하고 비포커스 배경을 0.92로 흐리게 한다. 에이전트 상태는 별도 색의 점으로 표시한다. 본체의 실제 사용자 포커스 전환은 해당 서피스의 완료·응답 대기 알림도 해제한다.",
    );
}
