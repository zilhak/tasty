//! 설정 창 저장을 위한 필드 단위 3-way 병합.
//!
//! 설정 창은 열 때의 설정(`opened`)을 복제해 편집한다. 창이 열린 동안 다른 경로(agent IPC 등)가
//! 현재 설정(`current`)을 바꿀 수 있으므로, 저장할 때 창의 결과(`edited`)를 그대로 쓰지 않고 필드마다
//! 고른다. 창이 바꾸지 않은 필드는 현재 값, 바꾼 필드는 창의 값이다. 양쪽이 같은 필드를 다르게
//! 바꿨으면 창(사용자) 값을 쓰고 충돌로 보고한다.
//!
//! 표·맵은 키마다 내려가 병합한다. 목록은 값 하나로 다룬다.

use toml::Value;

use crate::Settings;

/// 병합 결과와 양쪽이 다르게 바꿔 창 값을 고른 경로들.
pub struct MergeOutcome {
    pub settings: Settings,
    pub conflicts: Vec<String>,
}

/// `opened`(창을 열 때) · `edited`(창의 결과) · `current`(저장 시점)를 병합한다.
/// 직렬화·역직렬화에 실패하면 창의 결과를 그대로 쓰고 그 사유를 충돌 목록에 넣는다.
pub fn merge_settings_edit(
    opened: &Settings,
    edited: &Settings,
    current: &Settings,
) -> MergeOutcome {
    let encode = |s: &Settings| Value::try_from(s);
    let (o, e, c) = match (encode(opened), encode(edited), encode(current)) {
        (Ok(o), Ok(e), Ok(c)) => (o, e, c),
        (Err(err), _, _) | (_, Err(err), _) | (_, _, Err(err)) => {
            return MergeOutcome {
                settings: edited.clone(),
                conflicts: vec![format!("<encode failed: {err}>")],
            };
        }
    };
    let mut conflicts = Vec::new();
    let mut path = Vec::new();
    let merged = merge_value(Some(&o), Some(&e), Some(&c), &mut path, &mut conflicts)
        .unwrap_or_else(|| e.clone());
    match merged.try_into::<Settings>() {
        Ok(mut settings) => {
            settings.origin = edited.origin;
            MergeOutcome {
                settings,
                conflicts,
            }
        }
        Err(err) => MergeOutcome {
            settings: edited.clone(),
            conflicts: vec![format!("<decode failed: {err}>")],
        },
    }
}

/// 값 하나를 병합한다. `None` 은 그 필드가 없다는 뜻이다.
fn merge_value(
    o: Option<&Value>,
    e: Option<&Value>,
    c: Option<&Value>,
    path: &mut Vec<String>,
    conflicts: &mut Vec<String>,
) -> Option<Value> {
    if e == o || e == c {
        return c.cloned();
    }
    if c == o {
        return e.cloned();
    }
    match (e, c) {
        (Some(Value::Table(et)), Some(Value::Table(ct))) => {
            let empty = toml::map::Map::new();
            let ot = match o {
                Some(Value::Table(t)) => t,
                _ => &empty,
            };
            let keys = et.keys().chain(ct.keys()).chain(ot.keys());
            let mut out = toml::map::Map::new();
            let mut seen = std::collections::HashSet::new();
            for key in keys {
                if !seen.insert(key.clone()) {
                    continue;
                }
                path.push(key.clone());
                if let Some(v) = merge_value(ot.get(key), et.get(key), ct.get(key), path, conflicts)
                {
                    out.insert(key.clone(), v);
                }
                path.pop();
            }
            Some(Value::Table(out))
        }
        _ => {
            conflicts.push(render(path));
            e.cloned()
        }
    }
}

/// 충돌 경로 표기.
fn render(path: &[String]) -> String {
    path.join(".")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn merge(opened: &Settings, edited: &Settings, current: &Settings) -> MergeOutcome {
        merge_settings_edit(opened, edited, current)
    }

    fn rules(s: &Settings) -> Vec<(String, bool)> {
        s.terminal_input
            .rules
            .iter()
            .map(|r| (r.app.clone(), r.shift_enter_newline))
            .collect()
    }

    /// 창이 바꾸지 않은 필드는 저장 시점 값, 바꾼 필드는 창의 값이다.
    #[test]
    fn fields_the_window_did_not_change_keep_their_current_values() {
        let opened = Settings::default();
        let mut current = opened.clone();
        current.remote_transfer.dir = "/agent".into();
        current.webhook.allow_external = !opened.webhook.allow_external;
        current.modifier_hint.pos = Some(Default::default());
        let mut edited = opened.clone();
        edited.origin = crate::SettingsOrigin::Unparsable;
        edited.general.shell = "/bin/window-shell".into();
        edited.remote_transfer.max_mb = opened.remote_transfer.max_mb + 7;

        let out = merge(&opened, &edited, &current);
        assert!(out.conflicts.is_empty(), "{:?}", out.conflicts);
        let s = out.settings;
        assert_eq!(s.general.shell, "/bin/window-shell");
        assert_eq!(s.remote_transfer.dir, "/agent");
        assert_eq!(s.remote_transfer.max_mb, opened.remote_transfer.max_mb + 7);
        assert_eq!(s.webhook.allow_external, current.webhook.allow_external);
        assert_eq!(s.modifier_hint.pos, current.modifier_hint.pos);
        assert_eq!(
            s.origin,
            crate::SettingsOrigin::Unparsable,
            "the save policy is kept"
        );
    }

    /// 양쪽이 같은 필드를 다르게 바꾸면 창 값을 쓰고 그 경로를 충돌로 보고한다.
    #[test]
    fn a_field_changed_on_both_sides_takes_the_windows_value_and_is_reported() {
        let opened = Settings::default();
        let mut current = opened.clone();
        current.remote_transfer.dir = "/agent".into();
        current.keybindings.quit = vec!["ctrl+alt+q".into()];
        let mut edited = opened.clone();
        edited.remote_transfer.dir = "/window".into();
        edited.keybindings.quit = vec!["ctrl+shift+q".into()];

        let out = merge(&opened, &edited, &current);
        assert_eq!(out.settings.remote_transfer.dir, "/window");
        assert_eq!(
            out.settings.keybindings.quit,
            vec!["ctrl+shift+q".to_string()]
        );
        assert_eq!(out.conflicts, ["keybindings.quit", "remote_transfer.dir"]);

        // 같은 값으로 바꿨으면 충돌이 아니다.
        current.remote_transfer.dir = "/window".into();
        current.keybindings.quit = edited.keybindings.quit.clone();
        assert!(merge(&opened, &edited, &current).conflicts.is_empty());
    }

    /// 창이 아무것도 바꾸지 않았으면 저장 시점 설정 그대로다.
    #[test]
    fn an_untouched_window_saves_the_current_settings() {
        let opened = Settings::default();
        let mut current = opened.clone();
        current.terminal_input.set_rule("agent", true).unwrap();
        current.general.shell = "/bin/agent".into();
        let out = merge(&opened, &opened, &current);
        assert!(out.conflicts.is_empty());
        assert_eq!(rules(&out.settings), rules(&current));
        assert_eq!(out.settings.general.shell, "/bin/agent");
    }
}
