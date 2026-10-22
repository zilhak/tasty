//! Folder-mode specimens use the production footer and the received three-language copy.
use super::{CRUMB_MENU_H, FRAME_H, FRAME_W, FpState, Mode, Variant, card};
use crate::catalog::spec::{self, StageVariant};
use crate::i18n::t;
use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::FolderPickerFooter;

/// 상태 견본과 한국어·Latte 견본의 카드 크기 — 디자인 폴더 갈래의 480×360 칸. 대응 Theme 토큰이 없다.
const SHORT_W: LogicalPx = LogicalPx(480.0);
const SHORT_H: LogicalPx = LogicalPx(360.0);

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
                FRAME_W,
                FRAME_H,
                false,
            ),
            (
                "Selected subfolder · 640",
                FpState::Loaded,
                true,
                Language::En,
                FRAME_W,
                FRAME_H,
                false,
            ),
            (
                "Files are inert · hover a file for its tooltip",
                FpState::Loaded,
                true,
                Language::En,
                FRAME_W,
                FRAME_H,
                false,
            ),
            (
                "Loading · cannot confirm",
                FpState::Loading,
                false,
                Language::En,
                SHORT_W,
                SHORT_H,
                false,
            ),
            (
                "Empty · use current folder",
                FpState::Empty,
                false,
                Language::En,
                SHORT_W,
                SHORT_H,
                false,
            ),
            (
                "Permission error · cannot confirm",
                FpState::ErrorPerm,
                false,
                Language::En,
                SHORT_W,
                SHORT_H,
                false,
            ),
            (
                "English · long path",
                FpState::Loaded,
                true,
                Language::En,
                FRAME_W,
                FRAME_H,
                true,
            ),
            (
                "한국어 · 480×360",
                FpState::Loaded,
                false,
                Language::Ko,
                SHORT_W,
                SHORT_H,
                true,
            ),
            (
                "日本語 · 最小幅",
                FpState::Loaded,
                true,
                Language::Ja,
                theme.fp_popup_min_width(),
                CRUMB_MENU_H,
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
        v.w = SHORT_W;
        v.h = SHORT_H;
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
