//! Claude 세션 프로필 여러 개를 하나의 `settings.json` 조각으로 머지한다.
//!
//! `--settings` 는 반복 지정이 last-wins 이라(실측, `profile.rs` 참고) 슬롯이
//! 하나뿐이다 — 프로필 둘 이상을 동시에 걸려면 이 머지가 유일한 조합 지점이다.
//!
//! 키 유형별 규칙:
//! - 객체: 키 단위 재귀 병합
//! - 배열: union(중복 제거) — 훅 이벤트 배열(`hooks.Stop` 등)도 이 규칙으로
//!   "concat" 이 된다(같은 훅 command 문자열이 중복 등록되는 것만 막는다)
//! - 스칼라: 값이 다르면 충돌. `permissions.defaultMode` 는 권한 모드가 조용히
//!   약해질 위험이 있어 **거부**. 그 외 스칼라는 경고 후 나중 값으로 last-wins
//!
//! 병합 후 불변식 강제: `permissions.deny` 에 있는 항목은 `permissions.allow`
//! 에서 제거한다 — deny 가 allow 를 이겨야 조합 시 샌드박스가 풀리지 않는다.

use serde_json::Value;
use tasty_plugin_sdk::i18n::Translator;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergeError {
    /// 프로필 파일의 최상위가 JSON object 가 아님.
    NotAnObject(String),
    /// 보안 민감 스칼라 키(`permissions.defaultMode`)가 서로 다른 값으로 충돌.
    ScalarConflict {
        path: String,
        existing: Value,
        incoming: Value,
    },
}

impl MergeError {
    pub(crate) fn translate(&self, tr: &Translator) -> String {
        match self {
            Self::NotAnObject(label) => tr.t_fmt("claude.profile_merge.not_an_object", label),
            Self::ScalarConflict {
                path,
                existing,
                incoming,
            } => tr
                .t("claude.profile_merge.scalar_conflict")
                .replacen("{}", path, 1)
                .replacen("{}", &existing.to_string(), 1)
                .replacen("{}", &incoming.to_string(), 1),
        }
    }
}

/// 충돌 시 거부(경고로 넘기지 않음)하는 스칼라 키 경로. 권한 모드가 조합으로
/// 조용히 약해지는 것을 막는다(위 실측 — `permissions.defaultMode`).
const HARD_REJECT_SCALAR_PATHS: &[&str] = &["$.permissions.defaultMode"];

/// `contents`(각 프로필의 JSON 최상위 object, 등록 순서)를 순서대로 접어 하나의
/// object 로 만든다. 발생한 경고(스칼라 last-wins 충돌 등)는 반환값에 모아
/// 호출자가 로그로 남긴다. 비어 있으면 빈 object 를 반환.
pub fn merge_contents(contents: &[(String, Value)]) -> Result<(Value, Vec<String>), MergeError> {
    let mut warnings = Vec::new();
    let mut acc = Value::Object(serde_json::Map::new());
    for (label, v) in contents {
        if !v.is_object() {
            return Err(MergeError::NotAnObject(label.clone()));
        }
        merge_value(&mut acc, v, "$", &mut warnings)?;
    }
    enforce_deny_beats_allow(&mut acc);
    Ok((acc, warnings))
}

fn merge_value(
    base: &mut Value,
    incoming: &Value,
    path: &str,
    warnings: &mut Vec<String>,
) -> Result<(), MergeError> {
    match (base, incoming) {
        (Value::Object(b), Value::Object(i)) => {
            for (k, v) in i {
                let child_path = format!("{path}.{k}");
                match b.get_mut(k) {
                    Some(existing) => merge_value(existing, v, &child_path, warnings)?,
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
            Ok(())
        }
        (Value::Array(b), Value::Array(i)) => {
            for item in i {
                if !b.contains(item) {
                    b.push(item.clone());
                }
            }
            Ok(())
        }
        (b, i) => {
            if b == i {
                return Ok(());
            }
            if HARD_REJECT_SCALAR_PATHS.contains(&path) {
                return Err(MergeError::ScalarConflict {
                    path: path.to_string(),
                    existing: b.clone(),
                    incoming: i.clone(),
                });
            }
            warnings.push(format!(
                "scalar key '{path}' conflict: {b} -> {i} (last-wins, later profile overrides earlier)"
            ));
            *b = i.clone();
            Ok(())
        }
    }
}

/// `permissions.deny` 에 있는 항목을 `permissions.allow` 에서 제거한다. deny 가
/// 없으면 no-op. 이 함수 호출 전에 `permissions.allow`/`deny` 는 이미 union 이
/// 끝난 상태여야 한다(재귀 배열 병합이 처리).
fn enforce_deny_beats_allow(root: &mut Value) {
    let Some(perms) = root.get_mut("permissions").and_then(|p| p.as_object_mut()) else {
        return;
    };
    let deny: Vec<Value> = perms
        .get("deny")
        .and_then(|d| d.as_array())
        .cloned()
        .unwrap_or_default();
    if deny.is_empty() {
        return;
    }
    if let Some(allow) = perms.get_mut("allow").and_then(|a| a.as_array_mut()) {
        allow.retain(|item| !deny.contains(item));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn labeled(v: Value) -> (String, Value) {
        ("p".into(), v)
    }

    #[test]
    fn empty_input_yields_empty_object() {
        let (merged, warnings) = merge_contents(&[]).unwrap();
        assert_eq!(merged, json!({}));
        assert!(warnings.is_empty());
    }

    #[test]
    fn hook_arrays_concat_across_profiles() {
        let a = json!({"hooks": {"Stop": [{"type":"command","command":"a"}]}});
        let b = json!({"hooks": {"Stop": [{"type":"command","command":"b"}]}});
        let (merged, _) = merge_contents(&[("a".into(), a), ("b".into(), b)]).unwrap();
        let stop = merged["hooks"]["Stop"].as_array().unwrap();
        assert_eq!(stop.len(), 2);
    }

    #[test]
    fn duplicate_hook_entries_are_not_duplicated() {
        let a = json!({"hooks": {"Stop": [{"type":"command","command":"same"}]}});
        let b = json!({"hooks": {"Stop": [{"type":"command","command":"same"}]}});
        let (merged, _) = merge_contents(&[("a".into(), a), ("b".into(), b)]).unwrap();
        assert_eq!(merged["hooks"]["Stop"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn env_maps_merge_key_wise() {
        let a = json!({"env": {"A": "1"}});
        let b = json!({"env": {"B": "2"}});
        let (merged, warnings) = merge_contents(&[("a".into(), a), ("b".into(), b)]).unwrap();
        assert_eq!(merged["env"]["A"], "1");
        assert_eq!(merged["env"]["B"], "2");
        assert!(warnings.is_empty());
    }

    #[test]
    fn env_key_conflict_warns_and_last_wins() {
        let a = json!({"env": {"A": "1"}});
        let b = json!({"env": {"A": "2"}});
        let (merged, warnings) = merge_contents(&[("a".into(), a), ("b".into(), b)]).unwrap();
        assert_eq!(merged["env"]["A"], "2");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn permission_lists_union() {
        let a = json!({"permissions": {"allow": ["Read"]}});
        let b = json!({"permissions": {"allow": ["Write"]}});
        let (merged, _) = merge_contents(&[("a".into(), a), ("b".into(), b)]).unwrap();
        let mut allow: Vec<String> = merged["permissions"]["allow"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        allow.sort();
        assert_eq!(allow, vec!["Read".to_string(), "Write".to_string()]);
    }

    /// 보안 회귀 테스트 — deny 프로필과 allow 프로필을 조합해도 샌드박스가
    /// 풀리면 안 된다(TODO 검증 절차 9번과 동일 시나리오, JSON 레벨).
    #[test]
    fn deny_beats_allow_even_when_allow_profile_applied_later() {
        let deny_profile = json!({"permissions": {"deny": ["Bash"]}});
        let allow_profile = json!({"permissions": {"allow": ["Bash"]}});
        let (merged, _) = merge_contents(&[
            ("deny".into(), deny_profile),
            ("allow".into(), allow_profile),
        ])
        .unwrap();
        let allow = merged["permissions"]["allow"].as_array().unwrap();
        assert!(
            !allow.iter().any(|v| v == "Bash"),
            "Bash must not survive in allow when a deny profile denies it: {merged}"
        );
        let deny = merged["permissions"]["deny"].as_array().unwrap();
        assert!(deny.iter().any(|v| v == "Bash"));
    }

    /// allow 에 **둘**을 둔다. 하나만 두면 "deny 된 것만 뺐다" 와 "allow 를 통째로
    /// 비웠다" 가 같은 관측이 되고, 권한 병합에서 그 둘은 전혀 다른 사고다.
    #[test]
    fn deny_beats_allow_regardless_of_profile_order() {
        let deny_profile = json!({"permissions": {"deny": ["Bash"]}});
        let allow_profile = json!({"permissions": {"allow": ["Bash", "Read"]}});
        let (merged, _) = merge_contents(&[
            ("allow".into(), allow_profile),
            ("deny".into(), deny_profile),
        ])
        .unwrap();
        let allow = merged["permissions"]["allow"].as_array().unwrap();
        assert!(!allow.iter().any(|v| v == "Bash"));
        // deny 와 무관한 항목은 allow 에 남는다.
        assert!(allow.iter().any(|v| v == "Read"));
    }

    #[test]
    fn default_mode_conflict_is_rejected() {
        let a = json!({"permissions": {"defaultMode": "default"}});
        let b = json!({"permissions": {"defaultMode": "acceptEdits"}});
        let err = merge_contents(&[("a".into(), a), ("b".into(), b)]).unwrap_err();
        assert!(matches!(err, MergeError::ScalarConflict { .. }));
    }

    #[test]
    fn default_mode_same_value_is_not_a_conflict() {
        let a = json!({"permissions": {"defaultMode": "default"}});
        let b = json!({"permissions": {"defaultMode": "default"}});
        let (merged, warnings) = merge_contents(&[("a".into(), a), ("b".into(), b)]).unwrap();
        assert_eq!(merged["permissions"]["defaultMode"], "default");
        assert!(warnings.is_empty());
    }

    #[test]
    fn other_scalar_conflict_warns_and_last_wins() {
        let a = json!({"theme": "dark"});
        let b = json!({"theme": "light"});
        let (merged, warnings) = merge_contents(&[("a".into(), a), ("b".into(), b)]).unwrap();
        assert_eq!(merged["theme"], "light");
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn non_object_top_level_is_rejected() {
        let err = merge_contents(&[labeled(json!(["not", "an", "object"]))]).unwrap_err();
        assert!(matches!(err, MergeError::NotAnObject(_)));
    }

    #[test]
    fn single_profile_passthrough() {
        let a = json!({"env": {"A": "1"}, "permissions": {"allow": ["Read"]}});
        let (merged, warnings) = merge_contents(&[("a".into(), a.clone())]).unwrap();
        assert_eq!(merged, a);
        assert!(warnings.is_empty());
    }
}
