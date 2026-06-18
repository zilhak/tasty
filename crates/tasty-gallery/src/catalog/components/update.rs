//! Update popup 데모 — 5 가지 상태 mock.
//!
//! 본체 `src/adapters/ui/popup/update.rs::draw_update_view` 와 동일한 시각
//! layout 을 로컬 mock 으로 재현한다. 본체 변경 시 시각 일치는 수동 검증
//! (Tier 3 패턴 한계 — gallery 가 binary crate `tasty` 에 의존 불가).
//!
//! 본체 의존: 0. 본체의 `UpdateProps` / `UpdateStatusView` / `UpdateAction`
//! 과 같은 형태를 로컬에 정의.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;

#[derive(Debug, Clone)]
enum UpdateStatusView {
    NeverChecked,
    Checking,
    UpToDate,
    Available {
        version: String,
        body: String,
        html_url: String,
    },
    Failed {
        reason: String,
    },
}

#[derive(Debug, Clone)]
struct UpdateProps {
    current_version: String,
    status: UpdateStatusView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UpdateAction {
    None,
    /// 본체 view 와의 API 동등성 유지를 위해 보존. 갤러리는 popup container
    /// 없이 단독 demo 라 Close action 을 trigger 할 경로는 없다.
    #[allow(dead_code)]
    Close,
    OpenReleasePage(String),
    CheckNow,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DemoStateKey {
    NeverChecked,
    Checking,
    UpToDate,
    Available,
    Failed,
}

impl DemoStateKey {
    fn label(self) -> &'static str {
        match self {
            DemoStateKey::NeverChecked => "Idle / Never checked",
            DemoStateKey::Checking => "Checking (in-flight)",
            DemoStateKey::UpToDate => "Up to date",
            DemoStateKey::Available => "Available (long changelog)",
            DemoStateKey::Failed => "Failed (long reason)",
        }
    }

    fn build(self) -> UpdateProps {
        let current_version = "0.6.3".to_string();
        let status = match self {
            DemoStateKey::NeverChecked => UpdateStatusView::NeverChecked,
            DemoStateKey::Checking => UpdateStatusView::Checking,
            DemoStateKey::UpToDate => UpdateStatusView::UpToDate,
            DemoStateKey::Available => UpdateStatusView::Available {
                version: "0.7.0".to_string(),
                body: long_changelog(),
                html_url: "https://github.com/zilhak/tasty/releases/tag/v0.7.0".to_string(),
            },
            DemoStateKey::Failed => UpdateStatusView::Failed {
                reason: "network: connection timed out after 10s while fetching \
                    https://api.github.com/repos/zilhak/tasty/releases/latest \
                    — check your internet connection or HTTPS proxy settings"
                    .to_string(),
            },
        };
        UpdateProps {
            current_version,
            status,
        }
    }
}

const ALL_KEYS: [DemoStateKey; 5] = [
    DemoStateKey::NeverChecked,
    DemoStateKey::Checking,
    DemoStateKey::UpToDate,
    DemoStateKey::Available,
    DemoStateKey::Failed,
];

fn long_changelog() -> String {
    "## What's new\n\n\
     - feat(popup): extract Tier 3 props from AppState\n\
     - feat(gallery): add update popup demo with mock states\n\
     - refactor(theme): unify popup color palette\n\
     - fix(popup): Escape key now closes update popup reliably\n\
     - fix(check): retry GitHub API on transient 502\n\n\
     ## Breaking changes\n\n\
     - `UpdateStatus.last_checked` is now `Option<Instant>` (was `Instant`)\n\n\
     ## Internals\n\n\
     - Bump egui to 0.31\n\
     - Bump winit to 0.30.5\n\
     - Drop dead `from_agent_*` intents (warnings)\n"
        .to_string()
}

thread_local! {
    static SELECTED: RefCell<DemoStateKey> = const { RefCell::new(DemoStateKey::Available) };
    static LAST_ACTION: RefCell<UpdateAction> = const { RefCell::new(UpdateAction::None) };
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    ui.label(
        egui::RichText::new(
            "Tier 3 — `draw_update_view(ui, theme, &UpdateProps) -> UpdateAction`. 본체 \
             AppState/CoreState 의존 0. 갤러리는 시각 layout 을 로컬 재현.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
    ui.add_space(8.0);

    // State 선택.
    ui.horizontal_wrapped(|ui| {
        for key in ALL_KEYS {
            let current = SELECTED.with(|s| *s.borrow());
            if ui.selectable_label(current == key, key.label()).clicked() {
                SELECTED.with(|s| *s.borrow_mut() = key);
                LAST_ACTION.with(|a| *a.borrow_mut() = UpdateAction::None);
            }
        }
    });
    ui.add_space(8.0);

    let props = SELECTED.with(|s| s.borrow().build());

    // Popup frame — 본체 PopupDef 의 440 × 360 default size.
    let frame_size = egui::vec2(440.0, 360.0);
    let (frame_rect, _) = ui.allocate_exact_size(frame_size, egui::Sense::hover());
    let painter = ui.painter_at(frame_rect);

    painter.rect_filled(
        frame_rect,
        theme.corner_radius.value(),
        egui::Color32::from(theme.surface0),
    );
    painter.rect_stroke(
        frame_rect,
        theme.corner_radius.value(),
        egui::Stroke::new(
            theme.border_width.value(),
            egui::Color32::from(theme.surface2),
        ),
        egui::StrokeKind::Inside,
    );

    let inner_rect = frame_rect.shrink(12.0);
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let action = draw_update_view_mock(&mut child_ui, theme, &props);
    if !matches!(action, UpdateAction::None) {
        LAST_ACTION.with(|a| *a.borrow_mut() = action);
    }

    ui.add_space(12.0);
    let last_action = LAST_ACTION.with(|a| a.borrow().clone());
    ui.label(
        egui::RichText::new(format!("Last UpdateAction: {:?}", last_action))
            .small()
            .color(egui::Color32::from(theme.subtext0)),
    );

    ui.add_space(4.0);
    ui.label(
        egui::RichText::new(
            "⚠ Visual mock — 시각 layout 은 본체와 일치하지만, 본체 `t()` 번역 키 \
             대신 영문 하드코딩. 본체 update.rs 갱신 시 시각 동등성은 수동 검증.",
        )
        .small()
        .color(egui::Color32::from(theme.subtext0)),
    );
}

/// 본체 `draw_update_view` 의 시각 layout 을 로컬 재현 (번역 키는 영문 하드코딩).
fn draw_update_view_mock(ui: &mut egui::Ui, theme: &Theme, props: &UpdateProps) -> UpdateAction {
    let mut action = UpdateAction::None;

    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;

        ui.label(
            egui::RichText::new("Check for updates")
                .color(egui::Color32::from(theme.text))
                .size(13.0),
        );
        ui.separator();

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Current version:").color(egui::Color32::from(theme.subtext0)),
            );
            ui.label(
                egui::RichText::new(&props.current_version)
                    .color(egui::Color32::from(theme.text))
                    .strong(),
            );
        });

        match &props.status {
            UpdateStatusView::Available {
                version,
                body,
                html_url,
            } => {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("Latest version:")
                            .color(egui::Color32::from(theme.subtext0)),
                    );
                    ui.label(
                        egui::RichText::new(version)
                            .color(egui::Color32::from(theme.accent_success()))
                            .strong(),
                    );
                });
                ui.separator();
                ui.label(
                    egui::RichText::new("Release notes")
                        .color(egui::Color32::from(theme.subtext0))
                        .size(12.0),
                );
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(body)
                                    .color(egui::Color32::from(theme.text))
                                    .size(12.0),
                            )
                            .wrap(),
                        );
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Open release page").clicked() {
                        action = UpdateAction::OpenReleasePage(html_url.clone());
                    }
                    if ui.button("Check now").clicked() {
                        action = UpdateAction::CheckNow;
                    }
                });
            }
            other => {
                match other {
                    UpdateStatusView::Failed { reason } => {
                        ui.label(
                            egui::RichText::new(format!("Error: {reason}"))
                                .color(egui::Color32::from(theme.accent_danger()))
                                .size(12.0),
                        );
                    }
                    UpdateStatusView::NeverChecked => {
                        ui.label(
                            egui::RichText::new("Not checked yet.")
                                .color(egui::Color32::from(theme.subtext0)),
                        );
                    }
                    UpdateStatusView::UpToDate => {
                        ui.label(
                            egui::RichText::new("You're up to date.")
                                .color(egui::Color32::from(theme.accent_success())),
                        );
                    }
                    UpdateStatusView::Checking => {
                        // 아래 in-flight 라벨이 담당.
                    }
                    UpdateStatusView::Available { .. } => unreachable!(),
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Check now").clicked() {
                        action = UpdateAction::CheckNow;
                    }
                    if matches!(other, UpdateStatusView::Checking) {
                        ui.label(
                            egui::RichText::new("Checking…")
                                .color(egui::Color32::from(theme.subtext0))
                                .italics(),
                        );
                    }
                });
            }
        }
    });

    action
}
