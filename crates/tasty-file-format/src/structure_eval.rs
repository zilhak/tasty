//! `DetectorRuleKind::StructureCheck` 평가자.
//!
//! JSON Schema 로 target 파일의 구조를 검증. 현재 입력 포맷은 **JSON 만 지원** —
//! YAML/TOML 입력은 별도 deps (serde_yml 등) 도입 후 확장. JSON 이외의 입력은
//! `false` 반환.
//!
//! ## 제한
//! - 파일 크기 > 5MB 면 즉시 `false` (큰 binary/log 파일을 스키마 검증하지 않도록).
//! - schema 컴파일은 매 호출마다 수행 — 글로벌 schema cache 는 후속 작업. 평균 schema
//!   가 작아 cold cost 무시 가능.
//! - spec_path 는 절대 경로일 때만 신뢰. 상대 경로는 현재 호스트의 CWD 에 의존하므로
//!   plugin 매니페스트 dir 기준 해석은 install 단계에서 수행해야 함 (별도 작업).

use std::fs;
use std::path::Path;

use super::types::FileTarget;

/// 입력 파일을 메모리에 통째 읽는 cap. 이보다 큰 파일은 schema 검증 후보가 아니라고
/// 보고 즉시 `false`.
pub const STRUCTURE_FILE_CAP: u64 = 5 * 1024 * 1024;

pub fn evaluate_structure(spec_path: &Path, target: &FileTarget) -> bool {
    if !target_eligible(target) {
        return false;
    }
    let Some(target_value) = read_json(target.as_path()) else {
        return false;
    };
    let Some(schema_value) = read_and_parse_schema(spec_path) else {
        return false;
    };
    let Some(validator) = compile_validator(spec_path, &schema_value) else {
        return false;
    };

    validator.is_valid(&target_value)
}

/// 구조검증 후보 자격: 디렉토리가 아니고, 일반 파일이고, [`STRUCTURE_FILE_CAP`]
/// 이하 크기이고, 확장자가 `.json` 인 target 만 통과. 현재 JSON 만 지원.
fn target_eligible(target: &FileTarget) -> bool {
    if target.is_directory() {
        return false;
    }
    let target_path = target.as_path();
    let meta = match fs::metadata(target_path) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if !meta.is_file() || meta.len() > STRUCTURE_FILE_CAP {
        return false;
    }
    let ext = target_path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());
    ext.as_deref() == Some("json")
}

/// 파일을 읽어 JSON 으로 파싱. 실패는 조용히 `None`(target 읽기/파싱 실패는 매치
/// 안함으로 처리 — spec 읽기와 달리 warn 로그 없음, 기존 정책 유지).
fn read_json(path: &Path) -> Option<serde_json::Value> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// spec 파일을 읽어 JSON 으로 파싱. 읽기/파싱 실패는 각각 warn 로그.
fn read_and_parse_schema(spec_path: &Path) -> Option<serde_json::Value> {
    let schema_bytes = match fs::read(spec_path) {
        Ok(b) => b,
        Err(e) => {
            tracing::warn!(
                spec = %spec_path.display(),
                error = %e,
                "structure-check: spec read failed",
            );
            return None;
        }
    };
    match serde_json::from_slice(&schema_bytes) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(
                spec = %spec_path.display(),
                error = %e,
                "structure-check: spec is not valid JSON",
            );
            None
        }
    }
}

/// schema JSON 을 컴파일. 실패는 warn 로그.
fn compile_validator(
    spec_path: &Path,
    schema_value: &serde_json::Value,
) -> Option<jsonschema::Validator> {
    match jsonschema::validator_for(schema_value) {
        Ok(v) => Some(v),
        Err(e) => {
            tracing::warn!(
                spec = %spec_path.display(),
                error = %e,
                "structure-check: schema compile failed",
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn write(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.path().join(name);
        fs::write(&p, bytes).expect("write");
        p
    }

    #[test]
    fn matches_when_target_satisfies_schema() {
        let dir = tempfile::tempdir().unwrap();
        let schema = write(
            &dir,
            "schema.json",
            br#"{"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}"#,
        );
        let target = write(&dir, "x.json", br#"{"name":"foo"}"#);
        assert!(evaluate_structure(&schema, &FileTarget::new(target)));
    }

    #[test]
    fn rejects_when_target_violates_schema() {
        let dir = tempfile::tempdir().unwrap();
        let schema = write(
            &dir,
            "schema.json",
            br#"{"type":"object","required":["name"],"properties":{"name":{"type":"string"}}}"#,
        );
        let target = write(&dir, "x.json", br#"{"name":123}"#);
        assert!(!evaluate_structure(&schema, &FileTarget::new(target)));
    }

    #[test]
    fn rejects_non_json_extension() {
        let dir = tempfile::tempdir().unwrap();
        let schema = write(&dir, "schema.json", br#"{"type":"object"}"#);
        let target = write(&dir, "x.yaml", b"name: foo");
        assert!(!evaluate_structure(&schema, &FileTarget::new(target)));
    }

    #[test]
    fn rejects_when_target_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let schema = write(&dir, "schema.json", br#"{"type":"object"}"#);
        let bogus = dir.path().join("does-not-exist.json");
        assert!(!evaluate_structure(&schema, &FileTarget::new(bogus)));
    }

    #[test]
    fn rejects_when_schema_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let target = write(&dir, "x.json", br#"{}"#);
        let bogus_schema = dir.path().join("missing-schema.json");
        assert!(!evaluate_structure(&bogus_schema, &FileTarget::new(target)));
    }

    #[test]
    fn rejects_directory_target() {
        let dir = tempfile::tempdir().unwrap();
        let schema = write(&dir, "schema.json", br#"{"type":"object"}"#);
        let dir_target = FileTarget::new(dir.path().to_path_buf());
        assert!(!evaluate_structure(&schema, &dir_target));
    }
}
