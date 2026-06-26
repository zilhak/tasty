//! `Status resolution` specimen — 디자인(4) `components/feedback/Status resolution` 카드.
//!
//! 한 surface 의 *소유(owner)* 상태와 *활동(activity)* 상태가 충돌할 때 어떤 점 하나로
//! 귀결되는지를 보이는 우선순위 표. 규칙: error › waiting › running › agent › idle,
//! 동순위는 live(activity) 우선. 항상 점 1개로 해소된다(`resolveStatus`).

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{StatusKind, status_dot};

use crate::catalog::spec::{StageVariant, TokenChip, meta, note, stage};

/// 우선순위 인덱스 (작을수록 우세).
fn priority(k: StatusKind) -> u8 {
    match k {
        StatusKind::Error => 0,
        StatusKind::Waiting => 1,
        StatusKind::Running => 2,
        StatusKind::Agent => 3,
        StatusKind::Idle => 4,
    }
}

/// owner × activity → 해소된 단일 상태. 동순위는 activity(live) 우선.
fn resolve(owner: StatusKind, activity: StatusKind) -> StatusKind {
    if priority(activity) <= priority(owner) {
        activity
    } else {
        owner
    }
}

fn label(k: StatusKind) -> &'static str {
    match k {
        StatusKind::Running => "running",
        StatusKind::Idle => "idle",
        StatusKind::Agent => "agent",
        StatusKind::Waiting => "waiting",
        StatusKind::Error => "error",
    }
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    // (owner 표시 라벨, owner dot 종류, activity). 디자인 owner 어휘는 user / agent —
    // StatusKind 엔 User 가 없어 user 는 Idle dot 으로 표현하고 라벨만 "user".
    let cases: [(&str, StatusKind, StatusKind); 6] = [
        ("agent", StatusKind::Agent, StatusKind::Running),
        ("agent", StatusKind::Agent, StatusKind::Idle),
        ("agent", StatusKind::Agent, StatusKind::Waiting),
        ("user", StatusKind::Idle, StatusKind::Running),
        ("user", StatusKind::Idle, StatusKind::Idle),
        ("agent", StatusKind::Agent, StatusKind::Error),
    ];

    let cw = theme.field_width_color.value();
    stage(ui, theme, StageVariant::Column, |ui| {
        // 헤더 행 — mono micro.
        ui.horizontal(|ui| {
            header_cell(ui, theme, cw, "owner");
            header_cell(ui, theme, cw, "activity");
            header_cell(ui, theme, cw, "=");
            header_cell(ui, theme, cw, "resolved dot");
        });
        for (owner_label, owner, activity) in cases {
            let resolved = resolve(owner, activity);
            ui.horizontal(|ui| {
                cell(ui, cw, |ui| {
                    status_dot(ui, theme, owner, owner_label, false, true);
                });
                cell(ui, cw, |ui| {
                    status_dot(ui, theme, activity, label(activity), false, true);
                });
                cell(ui, cw, |ui| {
                    ui.label(
                        egui::RichText::new("→")
                            .size(theme.font_size_body.value())
                            .color(egui::Color32::from(theme.text_muted())),
                    );
                });
                cell(ui, cw, |ui| {
                    status_dot(ui, theme, resolved, label(resolved), false, true);
                });
            });
        }
    });

    note(
        ui,
        theme,
        "Priority error › waiting › running › agent › idle. Live activity beats ownership on \
         ties — a row always resolves to exactly one dot.",
    );

    meta(
        ui,
        theme,
        &[
            ("priority", "error › waiting › running › agent › idle"),
            ("tie-break", "live beats ownership"),
            ("result", "always 1 dot"),
            ("helper", "resolveStatus()"),
        ],
        &[
            TokenChip::new(
                "accent-success",
                "running",
                egui::Color32::from(theme.accent_success()),
            ),
            TokenChip::new(
                "accent-agent",
                "agent",
                egui::Color32::from(theme.accent_agent()),
            ),
            TokenChip::new(
                "accent-danger",
                "error",
                egui::Color32::from(theme.accent_danger()),
            ),
        ],
    );
}

fn header_cell(ui: &mut egui::Ui, theme: &Theme, width: f32, text: &str) {
    cell(ui, width, |ui| {
        ui.label(
            egui::RichText::new(text.to_uppercase())
                .size(theme.font_size_micro.value())
                .color(egui::Color32::from(theme.text_muted())),
        );
    });
}

/// 고정폭 셀 — 4컬럼 정렬용 (field-width-color).
fn cell(ui: &mut egui::Ui, width: f32, add: impl FnOnce(&mut egui::Ui)) {
    ui.scope(|ui| {
        ui.set_width(width);
        add(ui);
    });
}
