//! 단축키 서브탭의 라벨 열이 설정 행과 같은 규칙(가장 긴 라벨을 150 … 240 으로 clamp)을 따르고
//! quick-switch 행 라벨까지 함께 재는지 확인한다.
//! apply_theme_to_egui를 호출해야 egui 기본값이 아닌 제품의 Body 크기로 측정한다.

use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::SettingsRow;

use super::{
    KeybindingsSubTab, entries_for, quick_switch, quick_switch_kinds, subtab_label_column,
};

const ENTRY_SUBTABS: &[KeybindingsSubTab] = &[
    KeybindingsSubTab::General,
    KeybindingsSubTab::Workspace,
    KeybindingsSubTab::Pane,
    KeybindingsSubTab::Tab,
    KeybindingsSubTab::Surface,
    KeybindingsSubTab::Clipboard,
    KeybindingsSubTab::Zoom,
    KeybindingsSubTab::Explorer,
];

/// 한 프레임 안에서 `f` 를 부른다.
fn with_ui(f: impl FnOnce(&egui::Ui)) {
    let ctx = egui::Context::default();
    tasty_egui_theme::install_cjk_fallback(&ctx);
    tasty_egui_theme::apply_theme_to_egui(&crate::theme::theme(), &ctx);
    let mut f = Some(f);
    // 출력은 쓰지 않는다. 측정은 `f` 안에서 끝난다.
    drop(ctx.run(Default::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(f) = f.take() {
                f(ui);
            }
        });
    }));
}

#[test]
fn every_entry_subtab_column_is_clamped_like_settings_rows() {
    let th = crate::theme::theme();
    with_ui(|ui| {
        for &sub in ENTRY_SUBTABS {
            let col = subtab_label_column(ui, &th, &entries_for(sub), quick_switch_kinds(sub));
            assert!(
                col >= th.settings_label_width() && col <= th.settings_label_max_width(),
                "{sub:?} column {col:?} is outside the settings label clamp"
            );
        }
    });
}

/// quick-switch 행 라벨도 열 폭을 정한다. 빠지면 그 라벨이 엔트리보다 긴 언어에서 슬롯 행 라벨이
/// 줄을 바꾼다. en 은 엔트리 라벨이 더 길어 실제 목록으로는 이 누락이 드러나지 않으므로 엔트리
/// 없이 잰다.
#[test]
fn quick_switch_labels_widen_the_column_they_share() {
    let th = crate::theme::theme();
    with_ui(|ui| {
        for &sub in ENTRY_SUBTABS {
            let kinds = quick_switch_kinds(sub);
            let col = subtab_label_column(ui, &th, &[], kinds);
            for &kind in kinds {
                for label in quick_switch::row_labels(kind) {
                    let natural = SettingsRow::new(&label).natural_label_width(ui, &th);
                    let want =
                        natural.clamp(th.settings_label_width(), th.settings_label_max_width());
                    assert!(
                        col >= want - LogicalPx(0.01),
                        "{sub:?} column {col:?} is narrower than quick-switch label '{label}' ({want:?})"
                    );
                }
            }
        }
    });
}
