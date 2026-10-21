//! Folder-mode specimens use the production footer and the received three-language copy.
use super::{FpState, Mode, Variant, card};
use crate::catalog::spec::{self, StageVariant};
use crate::i18n::t;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::folder_picker::FolderPickerFooter;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Language {
    En,
    Ko,
    Ja,
}

pub(super) fn props(v: Variant) -> FolderPickerFooter<'static> {
    let Mode::Folder {
        selected,
        lang,
        long,
    } = v.mode
    else {
        unreachable!()
    };
    let (label, confirm, cancel, current, picked) = match lang {
        Language::En => (
            t("picker.folder.label"),
            t("picker.folder.confirm"),
            t("button.cancel"),
            t("picker.folder.hint.current"),
            t("picker.folder.hint.selected"),
        ),
        Language::Ko => (
            "폴더",
            "폴더 선택",
            "취소",
            "현재 폴더를 사용합니다. 하위 폴더를 고르면 그 폴더를 사용합니다.",
            "{name} 폴더를 열려면 더블클릭하세요.",
        ),
        Language::Ja => (
            "フォルダー",
            "フォルダーを選択",
            "キャンセル",
            "現在のフォルダーを使用します。サブフォルダーを選ぶとそのフォルダーを使用します。",
            "{name} を開くにはダブルクリックします。",
        ),
    };
    let name = "configs";
    FolderPickerFooter {
        label,
        confirm,
        cancel,
        hint: if selected { picked } else { current },
        selected: selected.then_some(name),
        path: if long {
            if selected {
                "/Users/maya/long-folder-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/current-folder-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb/configs"
            } else {
                "/Users/maya/long-folder-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/current-folder-bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
            }
        } else if selected {
            "/Users/maya/projects/configs"
        } else {
            "/Users/maya/projects"
        },
        ready: matches!(v.state, FpState::Loaded | FpState::Empty),
    }
}

pub fn draw_folder_mode(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        for (label, state, selected, lang, width, height, long) in [
            (
                "Current folder · 640",
                FpState::Loaded,
                false,
                Language::En,
                LogicalPx(640.0),
                LogicalPx(480.0),
                false,
            ),
            (
                "Selected subfolder · 640",
                FpState::Loaded,
                true,
                Language::En,
                LogicalPx(640.0),
                LogicalPx(480.0),
                false,
            ),
            (
                "Files are inert · hover a file for its tooltip",
                FpState::Loaded,
                true,
                Language::En,
                LogicalPx(640.0),
                LogicalPx(480.0),
                false,
            ),
            (
                "Loading · cannot confirm",
                FpState::Loading,
                false,
                Language::En,
                LogicalPx(480.0),
                LogicalPx(360.0),
                false,
            ),
            (
                "Empty · use current folder",
                FpState::Empty,
                false,
                Language::En,
                LogicalPx(480.0),
                LogicalPx(360.0),
                false,
            ),
            (
                "Permission error · cannot confirm",
                FpState::ErrorPerm,
                false,
                Language::En,
                LogicalPx(480.0),
                LogicalPx(360.0),
                false,
            ),
            (
                "English · long path",
                FpState::Loaded,
                true,
                Language::En,
                LogicalPx(640.0),
                LogicalPx(480.0),
                true,
            ),
            (
                "한국어 · 480×360",
                FpState::Loaded,
                false,
                Language::Ko,
                LogicalPx(480.0),
                LogicalPx(360.0),
                true,
            ),
            (
                "日本語 · 最小幅",
                FpState::Loaded,
                true,
                Language::Ja,
                LogicalPx(320.0),
                LogicalPx(420.0),
                true,
            ),
        ] {
            spec::cluster(ui, theme, label, |ui| {
                let mut v = Variant::open(state, false, false);
                v.mode = Mode::Folder {
                    selected,
                    lang,
                    long,
                };
                v.w = width;
                v.h = height;
                if long {
                    v.path = super::path_bar::PathKind::LongTwo;
                }
                card(ui, theme, v);
            });
        }
    });
    let latte = crate::host_shell::latte_theme();
    spec::stage(ui, &latte, StageVariant::Wrap, |ui| {
        let mut v = Variant::open(FpState::Loaded, false, false);
        v.mode = Mode::Folder {
            selected: true,
            lang: Language::En,
            long: false,
        };
        v.w = LogicalPx(480.0);
        v.h = LogicalPx(360.0);
        card(ui, &latte, v);
    });
    spec::note(
        ui,
        theme,
        "Enter confirms the selected folder or current folder. Double-click and Right enter a subfolder. Files retain their layout but use disabled ink and are skipped by keyboard navigation. Clicking the selected folder or empty area clears selection.",
    );
}

pub(super) fn file_tooltip(v: Variant) -> &'static str {
    match v.mode {
        Mode::Folder {
            lang: Language::Ko, ..
        } => "폴더만 선택할 수 있습니다.",
        Mode::Folder {
            lang: Language::Ja, ..
        } => "選択できるのはフォルダーだけです。",
        _ => t("picker.folder.file.tooltip"),
    }
}
pub(super) fn empty(v: Variant) -> &'static str {
    match v.mode {
        Mode::Folder {
            lang: Language::Ko, ..
        } => "하위 폴더가 없습니다",
        Mode::Folder {
            lang: Language::Ja, ..
        } => "サブフォルダーはありません",
        _ => t("picker.folder.empty"),
    }
}
