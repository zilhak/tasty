//! Workspace category(사이드바 폴더) — 워크스페이스를 그룹으로 묶는 계층.
//!
//! 핵심 불변식(`docs/concepts/ubiquitous-language.md` Workspace Category):
//! - `normal` 카테고리는 항상 존재하고 id 가 [`NORMAL_CATEGORY_ID`](tasty_utils::id::NORMAL_CATEGORY_ID) (`0`) 로 고정.
//! - `categories[0]` 는 항상 `normal` (위치 고정, reorder 대상 아님).
//! - 카테고리는 비어 있을 수 있다(워크스페이스 0개 허용, 자동 삭제 안 함).
//! - 카테고리 이름은 trim 후 비교하며 대소문자 무시 중복/예약어를 거부한다.

use tasty_utils::id::{NORMAL_CATEGORY_ID, WorkspaceCategoryId};

/// 예약된 normal 카테고리의 정규 이름.
pub const NORMAL_CATEGORY_NAME: &str = "normal";

/// Workspace category — 사이드바에서 워크스페이스를 묶는 폴더 한 칸.
///
/// `Vec<WorkspaceCategory>` 의 순서 = 사이드바 섹션 표시 순서. `collapsed` 는
/// 사이드바 접힘 UI 상태(영속 대상이지만 사용자 UI 상태이므로 IPC 노출 안 함).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceCategory {
    pub id: WorkspaceCategoryId,
    pub name: String,
    /// 사이드바에서 이 카테고리 섹션이 접혀 있는지. layout.json 으로 영속.
    pub collapsed: bool,
}

impl WorkspaceCategory {
    /// 예약된 normal 카테고리 인스턴스(id 0, 이름 "normal").
    pub fn normal() -> Self {
        Self {
            id: NORMAL_CATEGORY_ID,
            name: NORMAL_CATEGORY_NAME.to_string(),
            collapsed: false,
        }
    }

    /// 일반(사용자) 카테고리.
    pub fn new(id: WorkspaceCategoryId, name: String) -> Self {
        Self {
            id,
            name,
            collapsed: false,
        }
    }

    /// id 가 예약된 normal 인지.
    pub fn is_normal(&self) -> bool {
        self.id == NORMAL_CATEGORY_ID
    }
}

/// 카테고리 이름 검증 에러. IPC/CLI/GUI 생성·이름변경 경로가 공통으로 사용한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CategoryNameError {
    /// trim 후 빈 이름.
    Empty,
    /// 예약어(`normal`, 대소문자 무시).
    Reserved,
    /// 기존 카테고리명과 중복(대소문자 무시).
    Duplicate,
}

impl CategoryNameError {
    /// i18n 키 — 호출자가 `t()` 로 사용자 메시지를 만든다.
    pub fn i18n_key(&self) -> &'static str {
        match self {
            CategoryNameError::Empty => "workspace_category.error.empty",
            CategoryNameError::Reserved => "workspace_category.error.reserved",
            CategoryNameError::Duplicate => "workspace_category.error.duplicate",
        }
    }
}

impl std::fmt::Display for CategoryNameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let msg = match self {
            CategoryNameError::Empty => "category name must not be empty",
            CategoryNameError::Reserved => "'normal' is a reserved category name",
            CategoryNameError::Duplicate => "a category with this name already exists",
        };
        f.write_str(msg)
    }
}

impl std::error::Error for CategoryNameError {}

/// 카테고리 이름 정규화 — 앞뒤 공백 제거.
pub fn normalize_category_name(raw: &str) -> String {
    raw.trim().to_string()
}

/// 정규화 후 예약어 `normal` 인지(대소문자 무시).
pub fn is_reserved_normal(name: &str) -> bool {
    normalize_category_name(name).eq_ignore_ascii_case(NORMAL_CATEGORY_NAME)
}

/// 새 카테고리 이름 검증. `existing` 은 기존 카테고리 이름들(normal 포함).
///
/// 1) trim 후 빈 이름 거부, 2) 예약어 `normal` 거부, 3) 기존 이름과 대소문자
/// 무시 중복 거부. 성공 시 정규화된 이름을 반환한다.
pub fn validate_new_category_name<'a>(
    raw: &str,
    existing: impl IntoIterator<Item = &'a str>,
) -> Result<String, CategoryNameError> {
    let name = normalize_category_name(raw);
    if name.is_empty() {
        return Err(CategoryNameError::Empty);
    }
    if is_reserved_normal(&name) {
        return Err(CategoryNameError::Reserved);
    }
    if existing
        .into_iter()
        .any(|e| e.eq_ignore_ascii_case(&name))
    {
        return Err(CategoryNameError::Duplicate);
    }
    Ok(name)
}

/// rename 시 이름 검증. 자기 자신의 현재 이름과 같은(대소문자만 다른) 경우는 허용.
/// `existing` 은 **대상 외** 카테고리 이름들이어야 한다(호출자가 자기 자신 제외).
pub fn validate_rename_category_name<'a>(
    raw: &str,
    existing: impl IntoIterator<Item = &'a str>,
) -> Result<String, CategoryNameError> {
    // 규칙은 신규 생성과 동일 — 호출자가 existing 에서 대상 자신을 제외해 전달한다.
    validate_new_category_name(raw, existing)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_has_reserved_id() {
        let n = WorkspaceCategory::normal();
        assert_eq!(n.id, NORMAL_CATEGORY_ID);
        assert!(n.is_normal());
        assert_eq!(n.name, NORMAL_CATEGORY_NAME);
    }

    #[test]
    fn reserved_normal_case_insensitive() {
        assert!(is_reserved_normal("normal"));
        assert!(is_reserved_normal("Normal"));
        assert!(is_reserved_normal("  NORMAL  "));
        assert!(!is_reserved_normal("normal-2"));
    }

    #[test]
    fn validate_rejects_empty_reserved_duplicate() {
        let existing = vec!["work", "play"];
        assert_eq!(
            validate_new_category_name("   ", existing.iter().copied()),
            Err(CategoryNameError::Empty)
        );
        assert_eq!(
            validate_new_category_name("Normal", existing.iter().copied()),
            Err(CategoryNameError::Reserved)
        );
        assert_eq!(
            validate_new_category_name("WORK", existing.iter().copied()),
            Err(CategoryNameError::Duplicate)
        );
        assert_eq!(
            validate_new_category_name("  Study  ", existing.iter().copied()),
            Ok("Study".to_string())
        );
    }
}
