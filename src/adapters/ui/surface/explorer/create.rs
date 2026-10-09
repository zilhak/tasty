//! 새 폴더·새 파일. 목록 맨 위의 인라인 이름 입력과 이름 검사.
//! 입력 중에는 정렬 자리로 옮기지 않아 줄이 튀지 않는다. 확정하면 `ExplorerAction::Create`
//! 를 내고, 만든 뒤 다시 읽은 목록에서 정렬 자리로 간다.

use std::path::{Path, PathBuf};

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{
    ExplorerNameEdit, ExplorerNameEvent, ExplorerNameLayout, explorer_name_error, explorer_name_row,
};

use super::ExplorerAction;
use super::view::ExplorerView;
use crate::i18n::{t, t_fmt};

/// 이름을 받고 있는 새 항목. 대상 폴더는 명령을 시작할 때 정한다.
#[derive(Clone, Debug)]
pub(crate) struct CreateEdit {
    pub(crate) dir: PathBuf,
    pub(crate) folder: bool,
    pub(crate) edit: ExplorerNameEdit,
    /// 대상 폴더에 이미 있는 이름. 명령을 시작할 때 알던 목록이다.
    taken: Vec<String>,
    /// Enter 에서 이미 있다고 확인한 이름. 글자를 바꾸면 이 오류는 사라진다.
    exists: Option<String>,
}

/// 이름 오류.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum NameError {
    Empty,
    InvalidChar(char),
    /// 앞이나 뒤에 공백이 있다. 조용히 떼지 않고 거절한다.
    EdgeSpace,
    Reserved,
    Exists,
}

/// Windows 가 파일 이름에 받지 않는 글자. 제어 문자도 받지 않는다.
const WINDOWS_INVALID: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// 입력하는 동안 보는 검사. 이미 있는지는 Enter 에서 따로 본다.
/// 공백만인 이름은 빈 이름이다. 앞뒤 공백은 모든 OS 에서 거절한다. 사용자가 일부러 넣었을 수도 있어
/// 떼어 내면 입력한 이름과 달라지고, 남겨 두면 눈에 보이지 않는 차이가 생기기 때문이다.
pub(crate) fn check_name(name: &str, windows: bool) -> Result<(), NameError> {
    if name.trim().is_empty() {
        return Err(NameError::Empty);
    }
    let invalid = |c: char| {
        c == '/' || c == '\0' || (windows && (WINDOWS_INVALID.contains(&c) || (c as u32) < 0x20))
    };
    if let Some(c) = name.chars().find(|c| invalid(*c)) {
        return Err(NameError::InvalidChar(c));
    }
    if name.starts_with(char::is_whitespace) || name.ends_with(char::is_whitespace) {
        return Err(NameError::EdgeSpace);
    }
    // Windows 는 끝의 점을 떼고 만들어 다른 이름이 되므로 예약 이름처럼 막는다.
    let trailing = windows && name.ends_with('.');
    if name == "." || name == ".." || trailing || (windows && reserved_on_windows(name)) {
        return Err(NameError::Reserved);
    }
    Ok(())
}

/// CON·PRN·AUX·NUL·COM1~9·LPT1~9 는 확장자가 붙어도 장치 이름이다.
fn reserved_on_windows(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or(name).trim_end();
    let upper = stem.to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ((upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.len() == 4
            && upper.as_bytes()[3].is_ascii_digit()
            && upper.as_bytes()[3] != b'0')
}

/// 이 OS 의 검사.
fn check_here(name: &str) -> Result<(), NameError> {
    check_name(name, cfg!(windows))
}

impl NameError {
    pub(crate) fn message(&self, name: &str) -> String {
        match self {
            Self::Empty => t("explorer.name.empty").to_string(),
            Self::InvalidChar(c) => t_fmt("explorer.name.invalid_char", &c.to_string()),
            Self::EdgeSpace => t("explorer.name.edge_space").to_string(),
            Self::Reserved => t("explorer.name.reserved").to_string(),
            Self::Exists => t_fmt("explorer.name.exists", name),
        }
    }
}

/// `base` 가 이미 있으면 "{stem} 2{.ext}", "{stem} 3{.ext}" 순으로 비는 이름을 찾는다.
pub(crate) fn default_name(base: &str, taken: &[String]) -> String {
    let is_taken = |n: &str| taken.iter().any(|t| t == n);
    if !is_taken(base) {
        return base.to_string();
    }
    let (stem, ext) = split_ext(base);
    (2u32..)
        .map(|i| format!("{stem} {i}{ext}"))
        .find(|n| !is_taken(n))
        .expect("an unused name exists")
}

/// 마지막 점부터를 확장자로 본다. 맨 앞의 점(숨김 파일)은 확장자가 아니다.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    }
}

impl CreateEdit {
    fn new(dir: PathBuf, folder: bool, taken: Vec<String>) -> Self {
        let base = if folder {
            t("explorer.new.folder_default")
        } else {
            t("explorer.new.file_default")
        };
        let name = default_name(base, &taken);
        // 폴더는 이름 전체를, 파일은 확장자 앞까지 고른다.
        let selected = if folder {
            name.chars().count()
        } else {
            split_ext(&name).0.chars().count()
        };
        Self {
            dir,
            folder,
            edit: ExplorerNameEdit::new(name, 0..selected),
            taken,
            exists: None,
        }
    }

    /// 지금 보일 오류. 입력하는 동안의 검사와 Enter 에서 확인한 "이미 있음" 이다.
    fn error(&self) -> Option<NameError> {
        let name = self.edit.buf.as_str();
        check_here(name)
            .err()
            .or_else(|| (self.exists.as_deref() == Some(name)).then_some(NameError::Exists))
    }

    /// 확정할 수 있는 이름이면 그대로, 아니면 오류.
    fn confirmable(&self) -> Result<(), NameError> {
        check_here(&self.edit.buf)?;
        if self.taken.iter().any(|n| n == &self.edit.buf) {
            return Err(NameError::Exists);
        }
        Ok(())
    }
}

impl ExplorerView {
    /// 새 항목 이름 입력을 연다. 이미 열려 있으면 새로 시작한다.
    /// 대상 폴더의 이름 목록은 지금 보는 목록이나 사이드바 트리에서 읽은 것만 쓴다.
    /// 화면 스레드는 파일시스템을 읽지 않으므로, 모르는 이름과 겹치면 만들 때 실패로 알린다.
    pub(crate) fn start_create(&mut self, dir: PathBuf, folder: bool) {
        let taken: Vec<String> = if self.shown_dir() == Some(dir.as_path()) {
            self.entries.iter().map(|e| e.name.clone()).collect()
        } else {
            self.tree_children
                .get(&dir)
                .map(|children| children.iter().map(|e| e.name.clone()).collect())
                .unwrap_or_default()
        };
        self.create = Some(CreateEdit::new(dir, folder, taken));
    }
}

/// 보기 모드별 편집 줄 배치.
pub(super) enum Slot {
    /// 상세 표가 비워 둔 행 위에 겹쳐 그린다. 표 전체 폭과 이름 칸 rect.
    Detail {
        row: egui::Rect,
        name_cell: egui::Rect,
    },
    List,
    Grid,
}

/// 상세 표의 가짜 행 이름 칸 위치를 받아 편집 줄을 겹쳐 그린다.
pub(super) fn detail_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &mut ExplorerView,
    name_cell: Option<egui::Rect>,
    action: &mut Option<ExplorerAction>,
) {
    let Some(name_cell) = name_cell else {
        return;
    };
    let row = egui::Rect::from_x_y_ranges(
        ui.max_rect().x_range(),
        egui::Rangef::point(name_cell.center().y).expand(theme.table_cell_height().value() * 0.5),
    );
    name_row(ui, theme, view, Slot::Detail { row, name_cell }, action);
}

/// 편집 줄을 그리고 입력을 처리한다. 확정하면 `Create` 를 낸다.
pub(super) fn name_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    view: &mut ExplorerView,
    slot: Slot,
    action: &mut Option<ExplorerAction>,
) {
    let Some(create) = view.create.as_mut() else {
        return;
    };
    let glyph = if create.folder {
        crate::adapters::ui::icons::FOLDER
    } else {
        crate::adapters::ui::icons::FILE
    };
    let before = create.edit.buf.clone();
    let first_frame = create.edit.initial_selection.is_some();
    let error = create.error();
    let (field, event) = match slot {
        Slot::Detail { row, name_cell } => {
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(row));
            let layout = ExplorerNameLayout::Detail {
                inset: name_cell.left() - row.left(),
                name_width: name_cell.right() - row.left(),
            };
            let (_, field, event) = explorer_name_row(
                &mut child,
                theme,
                layout,
                glyph,
                &mut create.edit,
                error.is_some(),
            );
            (field, event)
        }
        Slot::List => {
            let (_, field, event) = explorer_name_row(
                ui,
                theme,
                ExplorerNameLayout::List,
                glyph,
                &mut create.edit,
                error.is_some(),
            );
            (field, event)
        }
        Slot::Grid => {
            // 다른 Grid 칸의 글리프 자리와 같은 높이여야 글리프가 한 줄에 선다.
            let slot = theme.icon_glyph_size_md.value();
            let pad = theme.spacing_sm.value();
            let cell = egui::vec2(
                super::CELL_W.value(),
                pad + slot + theme.spacing_xs.value() + theme.input_height().value() + pad,
            );
            let (_, field, event) = explorer_name_row(
                ui,
                theme,
                ExplorerNameLayout::Grid { cell, slot },
                glyph,
                &mut create.edit,
                error.is_some(),
            );
            (field, event)
        }
    };
    if first_frame {
        ui.scroll_to_rect(field, None);
    }
    if create.edit.buf != before {
        create.exists = None;
    }
    if let Some(error) = &error {
        explorer_name_error(ui, theme, field, &error.message(&create.edit.buf));
    }
    match event {
        ExplorerNameEvent::None => {}
        ExplorerNameEvent::Cancel => view.create = None,
        ExplorerNameEvent::Confirm => match create.confirmable() {
            Ok(()) => confirm(view, action),
            Err(error) => {
                if error == NameError::Exists {
                    create.exists = Some(create.edit.buf.clone());
                }
                // 입력은 열어 두고 글자를 유지한다. 커서는 끝으로 둔다.
                let end = create.edit.buf.chars().count();
                create.edit.initial_selection = Some(end..end);
            }
        },
        ExplorerNameEvent::Blur => {
            if create.confirmable().is_ok() {
                confirm(view, action);
            } else {
                view.create = None;
            }
        }
    }
}

fn confirm(view: &mut ExplorerView, action: &mut Option<ExplorerAction>) {
    let Some(create) = view.create.take() else {
        return;
    };
    if action.is_none() {
        *action = Some(ExplorerAction::Create {
            dir: create.dir,
            name: create.edit.buf,
            folder: create.folder,
        });
    }
}

/// 상세 표에서 편집 줄 자리를 비워 둘 가짜 행. 이름이 비어 있어 실제 항목과 겹치지 않는다.
pub(super) fn placeholder_row(dir: &Path) -> super::DirEntryInfo {
    super::DirEntryInfo {
        path: dir.to_path_buf(),
        name: String::new(),
        is_dir: false,
        size: 0,
        modified: None,
        ext: String::new(),
        link: crate::core::fs_list::EntryLink::NotALink,
    }
}

#[cfg(test)]
mod tests;
