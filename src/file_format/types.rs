//! 식별 시스템 (`file_format`)의 공유 타입.
//!
//! `DetectorId` 와 `FileTarget` 은 `file_handler` 영역에서도 import 한다 (단방향).
//! `file_format` 은 `file_handler` 를 모른다.

use std::path::{Path, PathBuf};

/// 파일 식별 시스템의 입력. **path-confirmed 파일만** 받는다.
/// `file://` URI / `http://` URL 등의 scheme parsing 은 호출자 책임.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FileTarget(pub PathBuf);

impl FileTarget {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    pub fn as_path(&self) -> &Path {
        &self.0
    }

    pub fn display(&self) -> String {
        self.0.display().to_string()
    }

    pub fn is_directory(&self) -> bool {
        self.0.is_dir()
    }
}

/// 파일 형식 식별자.
///
/// 형식:
/// - 일반: `[a-z0-9-]{1,64}` (예: `markdown`, `image-png`).
/// - 예약: `$`로 시작. 호스트만 정의 가능. 1단계 예약 id: `$directory`.
///
/// "unknown" 은 detector 가 아니다. identify 가 매칭 실패하면 `None` 반환.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DetectorId(pub String);

impl DetectorId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_reserved(&self) -> bool {
        self.0.starts_with('$')
    }
}

impl std::fmt::Display for DetectorId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// detector id 형식 검증. `$`-시작은 호스트 한정.
pub fn is_valid_detector_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    if let Some(rest) = s.strip_prefix('$') {
        if rest.is_empty() {
            return false;
        }
        return rest
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// detector rule 의 출처. debug/표시용으로 보존.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuleOrigin {
    HostDefault,
    Plugin(String),
    User,
}

/// detector rule 종류. evaluator 가 이걸 보고 매칭 평가.
///
/// `Eq` 는 derive 하지 않는다 — `Unknown.raw: toml::Value` 가 `Eq` 를 구현하지 않기 때문.
#[derive(Debug, Clone, PartialEq)]
pub enum DetectorRuleKind {
    /// 확장자 매칭. 값은 `.` 제외 소문자.
    Extension { values: Vec<String> },
    /// 파일명 glob (예: `Dockerfile`, `*.config.json`).
    PathGlob { pattern: String },
    /// MIME 타입. Phase B 에서 본격 평가.
    Mime { types: Vec<String> },
    /// magic bytes 비교. Phase B 에서 본격 평가.
    Magic { offset: usize, bytes: Vec<u8> },
    /// path 가 디렉토리인지.
    IsDirectory,
    /// Lua 평가자 (Phase D). Phase A 는 schema 만.
    Lua { script_path: PathBuf },
    /// 구조 체크 (Phase D). Phase A 는 schema 만.
    StructureCheck { spec_path: PathBuf },
    /// 미지의 kind — payload 보존 (forward-compat).
    Unknown {
        kind_name: String,
        raw: toml::Value,
    },
}

/// detector 의 rule 항목.
#[derive(Debug, Clone)]
pub struct DetectorRule {
    pub kind: DetectorRuleKind,
    pub origin: RuleOrigin,
}

/// 등록된 detector. 같은 id 의 여러 출처가 rule union 으로 합쳐진 결과.
#[derive(Debug, Clone)]
pub struct FileFormatDetector {
    pub id: DetectorId,
    pub display_name_i18n_key: Option<String>,
    pub icon: Option<String>,
    pub rules: Vec<DetectorRule>,
    pub disabled: bool,
}

/// identify 평가 깊이.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectDepth {
    /// 확장자 / path glob / is-directory 만. hover/목록 표시에 사용.
    Cheap,
    /// + magic bytes + MIME + Lua/structure-check. Phase B 이후 활용.
    Deep,
}
