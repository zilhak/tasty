//! 요청 params 를 호스트 `terminal.*` 로 넘기기 전 다듬는 헬퍼.

use serde_json::{Map, Value};

/// 요청 params 에서 지정한 키들을 **존재할 때만** 그대로 새 Map 에 복사한다. CLI
/// 인자를 호스트 `terminal.*` 로 pass-through 하는 용도 — 없는 키를 `null` 로 채워
/// 보내면 호스트가 "값을 명시했다" 로 읽는 자리가 있어 존재 여부를 보존한다.
pub fn forward(params: &Value, keys: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    for k in keys {
        if let Some(v) = params.get(*k) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    out
}

/// 대상 parent surface 를 읽다 실패한 갈래. 문구는 **plugin 이** 자기 카탈로그로
/// 만든다 — 두 plugin 의 i18n namespace 가 다르므로 여기서 문자열을 짓지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TargetSurfaceError {
    /// 값이 왔는데 32 비트 정수가 아니다. **자르지 않고** 거절한다 —
    /// `4_294_967_297 as u32` 는 `1` 이고, 잘린 id 는 실재할 수 있는 다른 surface 다.
    Malformed { key: &'static str, raw: String },
    /// 두 이름이 서로 다른 대상을 가리킨다. 어느 쪽을 골라도 절반의 호출자에게는
    /// 지목하지 않은 대상이 되므로 고르지 않는다.
    Conflict { surface: u32, surface_id: u32 },
}

/// 대상 parent surface — `surface` / `surface_id` **두 이름을 한 필드로** 읽는다.
/// 아무 이름도 안 왔으면 `None`.
///
/// 두 이름이 생긴 내력: 매니페스트의 CLI 인자는 `surface`, 호스트 IPC 의 표준 키는
/// `surface_id`, 그래서 CLI 는 두 키를 모두 채워 보낸다(`crates/tasty-cli` 의 dynamic
/// runner). 그 이중 기입이 어긋남을 가려서, **raw IPC 호출에서만** 드러났다.
///
/// 실측(2026-09-05, 격리 인스턴스): 두 plugin 다 `kill` 이 대상을 `forward` 로만
/// 넘겨서, `surface_id` 로 지목한 호출은 **아무 대상도 안 실은 호출**이 됐고 호스트의
/// 유일-parent 폴백에 떨어졌다. 존재하지 않는 surface 999 를 지목한
/// `claude.kill` / `codex.kill` 이 성공을 돌려주며 남의 자식을 죽였다 — 폴백은
/// namespace 를 가리지 않아서 `codex.kill` 이 claude 자식을 죽이는 것까지 관측됐다.
/// 같은 값을 `surface` 로 주면 호스트가
/// `no live surface 999 … a named target is never resolved by focus` 로 막는다.
/// **이름이 달라서 그 가드를 우회한 것이다.**
pub fn target_surface(params: &Value) -> Result<Option<u32>, TargetSurfaceError> {
    let mut surface = None;
    let mut surface_id = None;
    for key in ["surface", "surface_id"] {
        let Some(raw) = params.get(key).filter(|v| !v.is_null()) else {
            continue;
        };
        let v = raw
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| TargetSurfaceError::Malformed {
                key,
                raw: raw.to_string(),
            })?;
        if key == "surface" {
            surface = Some(v);
        } else {
            surface_id = Some(v);
        }
    }
    match (surface, surface_id) {
        (Some(a), Some(b)) if a != b => Err(TargetSurfaceError::Conflict {
            surface: a,
            surface_id: b,
        }),
        (a, b) => Ok(a.or(b)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 두 이름이 **같은 필드**다 — 어느 쪽으로 지목해도 같은 대상이 나온다.
    #[test]
    fn either_name_names_the_same_target() {
        for p in [json!({ "surface": 7 }), json!({ "surface_id": 7 })] {
            assert_eq!(
                target_surface(&p),
                Ok(Some(7)),
                "{p} 에서 대상을 못 읽었다 — 호출자는 유일-parent 폴백을 받게 된다"
            );
        }
    }

    /// 아무 이름도 안 주면 `None` 이다 — 호출자가 그때 아무것도 안 싣도록,
    /// 여기서 값을 **지어내지 않는다.** 폴백이 곧 `--surface` 생략 동작이다.
    #[test]
    fn naming_no_target_yields_none() {
        assert_eq!(target_surface(&json!({ "child": 0 })), Ok(None));
    }

    /// 두 이름이 다른 값이면 고르지 않고 거절한다. 같은 값이면 CLI 가 보내는
    /// 정상 형태이므로 막지 않는다.
    #[test]
    fn two_names_disagreeing_is_refused_not_picked() {
        assert_eq!(
            target_surface(&json!({ "surface": 1, "surface_id": 2 })),
            Err(TargetSurfaceError::Conflict {
                surface: 1,
                surface_id: 2
            })
        );
        assert_eq!(
            target_surface(&json!({ "surface": 3, "surface_id": 3 })),
            Ok(Some(3))
        );
    }

    /// 32 비트를 넘는 값을 **자르지 않는다** — 잘린 id 는 실재할 수 있는 다른 surface 다.
    #[test]
    fn an_oversized_id_is_refused_not_truncated() {
        let err = target_surface(&json!({ "surface": 4_294_967_297u64 })).unwrap_err();
        assert!(
            matches!(err, TargetSurfaceError::Malformed { key: "surface", .. }),
            "잘라서 1 로 만들었다: {err:?}"
        );
    }

    #[test]
    fn absent_keys_stay_absent_and_null_is_preserved() {
        let p = json!({ "a": 1, "b": null });
        let out = forward(&p, &["a", "b", "c"]);
        assert_eq!(out.get("a"), Some(&json!(1)));
        assert_eq!(out.get("b"), Some(&json!(null)), "명시된 null 은 보존한다");
        assert!(!out.contains_key("c"), "없는 키를 만들어내지 않는다");
    }
}
