//! 설정 창 저장을 위한 필드 단위 3-way 병합.
//!
//! 설정 창은 열 때의 설정(`opened`)을 복제해 편집한다. 창이 열린 동안 다른 경로(agent IPC 등)가
//! 현재 설정(`current`)을 바꿀 수 있으므로, 저장할 때 창의 결과(`edited`)를 그대로 쓰지 않고 필드마다
//! 고른다. 창이 바꾸지 않은 필드는 현재 값, 바꾼 필드는 창의 값이다. 양쪽이 같은 필드를 다르게
//! 바꿨으면 창(사용자) 값을 쓰고 충돌로 보고한다.
//!
//! 표·맵은 키마다 내려가 병합한다. 목록은 [`LIST_KEYS`] 에 키가 정해진 것만 항목 단위로 병합하고,
//! 나머지 목록은 값 하나로 다룬다.

use toml::Value;

use crate::Settings;

/// 목록 항목을 구별하는 방법.
#[derive(Clone, Copy)]
enum ItemKey {
    /// 항목 표의 이 필드 값.
    Field(&'static str),
    /// 항목 값 전체(집합처럼 다룬다).
    Whole,
}

/// 항목 단위로 병합하는 목록의 경로와 키. `*` 는 목록 항목 하나를 뜻한다.
const LIST_KEYS: &[(&[&str], ItemKey)] = &[
    (&["terminal_input", "rules"], ItemKey::Field("app")),
    (&["scripts", "scripts"], ItemKey::Field("id")),
    (&["scripts", "scripts", "*", "triggers"], ItemKey::Whole),
    (&["keybindings", "script_bindings"], ItemKey::Whole),
];

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
        (Some(Value::Array(ea)), Some(Value::Array(ca))) if list_key(path).is_some() => {
            let key = list_key(path).expect("checked");
            let empty = Vec::new();
            let oa = match o {
                Some(Value::Array(a)) => a,
                _ => &empty,
            };
            Some(Value::Array(merge_list(oa, ea, ca, key, path, conflicts)))
        }
        _ => {
            conflicts.push(render(path));
            e.cloned()
        }
    }
}

/// 키로 항목을 맞춰 병합한다. 순서는 창의 결과를 따르고, 저장 시점에만 새로 생긴 항목은 뒤에 붙인다.
fn merge_list(
    o: &[Value],
    e: &[Value],
    c: &[Value],
    key: ItemKey,
    path: &mut Vec<String>,
    conflicts: &mut Vec<String>,
) -> Vec<Value> {
    let find = |list: &[Value], k: &Value| {
        list.iter()
            .find(|v| item_key(v, key).as_ref() == Some(k))
            .cloned()
    };
    let mut out = Vec::new();
    for item in e {
        let Some(k) = item_key(item, key) else {
            out.push(item.clone());
            continue;
        };
        let (before, now) = (find(o, &k), find(c, &k));
        path.push(format!("[{}]", key_label(&k)));
        path.push("*".into());
        if let Some(v) = merge_value(before.as_ref(), Some(item), now.as_ref(), path, conflicts) {
            out.push(v);
        }
        path.pop();
        path.pop();
    }
    // 창이 지운 항목: 저장 시점에 다른 경로가 바꿨으면 충돌이지만 창의 삭제를 따른다.
    for item in o {
        let Some(k) = item_key(item, key) else {
            continue;
        };
        if find(e, &k).is_none() && find(c, &k).is_some_and(|now| &now != item) {
            conflicts.push(format!("{}[{}]", render(path), key_label(&k)));
        }
    }
    // 창을 연 뒤 다른 경로가 더한 항목.
    for item in c {
        let Some(k) = item_key(item, key) else {
            continue;
        };
        if find(o, &k).is_none() && find(e, &k).is_none() {
            out.push(item.clone());
        }
    }
    out
}

/// 충돌 경로 표기. 항목 표시 `*` 는 빼고 `[키]` 는 앞 조각에 붙인다.
fn render(path: &[String]) -> String {
    let mut out = String::new();
    for part in path.iter().filter(|p| *p != "*") {
        if !out.is_empty() && !part.starts_with('[') {
            out.push('.');
        }
        out.push_str(part);
    }
    out
}

fn item_key(item: &Value, key: ItemKey) -> Option<Value> {
    match key {
        ItemKey::Whole => Some(item.clone()),
        ItemKey::Field(field) => item.get(field).cloned(),
    }
}

fn key_label(k: &Value) -> String {
    match k {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// `path` 에 해당하는 목록 키. 항목 단계는 `[키]` 와 `*` 두 조각으로 쌓이므로 `[..]` 조각은 건너뛴다.
fn list_key(path: &[String]) -> Option<ItemKey> {
    let shape: Vec<&str> = path
        .iter()
        .filter(|p| !p.starts_with('['))
        .map(String::as_str)
        .collect();
    LIST_KEYS
        .iter()
        .find(|(p, _)| *p == shape.as_slice())
        .map(|(_, key)| *key)
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

    /// 입력 규칙은 앱 이름으로 맞춰 병합한다.
    #[test]
    fn input_rules_merge_per_app() {
        let mut opened = Settings::default();
        opened.terminal_input.set_rule("shared", false).unwrap();
        opened.terminal_input.set_rule("gone", true).unwrap();
        opened.terminal_input.set_rule("edited", false).unwrap();
        let mut current = opened.clone();
        current.terminal_input.set_rule("agent", true).unwrap();
        current.terminal_input.set_rule("shared", true).unwrap();
        let mut edited = opened.clone();
        edited.terminal_input.remove_rule("gone");
        edited.terminal_input.set_rule("edited", true).unwrap();
        edited.terminal_input.set_rule("window", true).unwrap();

        let out = merge(&opened, &edited, &current);
        assert!(out.conflicts.is_empty(), "{:?}", out.conflicts);
        let mut got = rules(&out.settings);
        got.sort();
        assert_eq!(
            got,
            [
                ("agent".to_string(), true),
                ("edited".to_string(), true),
                ("shared".to_string(), true),
                ("window".to_string(), true),
            ]
        );

        // 창이 지운 규칙을 다른 경로가 바꿨으면 창의 삭제를 따르고 충돌로 보고한다.
        current.terminal_input.set_rule("gone", false).unwrap();
        let out = merge(&opened, &edited, &current);
        assert!(!rules(&out.settings).iter().any(|(app, _)| app == "gone"));
        assert_eq!(out.conflicts, ["terminal_input.rules[gone]"]);
    }

    /// 스크립트는 id 로 맞춘다. 창이 열린 동안 승인한 해시는 남고 창의 추가·삭제·이름 변경도 반영된다.
    #[test]
    fn scripts_merge_per_id_and_keep_hashes_approved_meanwhile() {
        let mut opened = Settings::default();
        let s = &mut opened.scripts;
        let kept = s.add("kept".into(), "/kept.lua".into(), "old".into());
        let renamed = s.add("renamed".into(), "/renamed.lua".into(), "old".into());
        let removed = s.add("removed".into(), "/removed.lua".into(), "old".into());
        let untouched = s.add("untouched".into(), "/untouched.lua".into(), "old".into());
        let mut current = opened.clone();
        for id in [&kept, &renamed, &removed] {
            current.scripts.update_hash(id, "approved".into());
        }
        let mut edited = opened.clone();
        edited.scripts.rename(&renamed, "new name".into());
        edited.scripts.remove(&removed);
        let added = edited
            .scripts
            .add("added".into(), "/added.lua".into(), "fresh".into());

        let out = merge(&opened, &edited, &current);
        let hash = |id: &str| out.settings.scripts.get(id).map(|e| e.sha256.clone());
        assert_eq!(hash(&kept).as_deref(), Some("approved"));
        assert_eq!(hash(&renamed).as_deref(), Some("approved"));
        assert_eq!(out.settings.scripts.get(&renamed).unwrap().name, "new name");
        assert_eq!(hash(&removed), None, "removed in the window");
        assert_eq!(hash(&untouched).as_deref(), Some("old"));
        assert_eq!(hash(&added).as_deref(), Some("fresh"));
        assert_eq!(out.conflicts, [format!("scripts.scripts[{removed}]")]);
    }

    /// 스크립트 트리거·단축키 바인딩은 항목 값으로 맞춘다.
    #[test]
    fn script_triggers_and_bindings_merge_as_sets() {
        let mut opened = Settings::default();
        let id = opened
            .scripts
            .add("s".into(), "/s.lua".into(), String::new());
        let event = |name: &str| crate::AutoTrigger::Event { name: name.into() };
        opened.scripts.add_trigger(&id, event("tab.create.post"));
        let mut current = opened.clone();
        current.scripts.add_trigger(&id, event("tab.delete.post"));
        current
            .keybindings
            .script_bindings
            .push(crate::ScriptBinding {
                script_id: id.clone(),
                combo: "ctrl+alt+1".into(),
            });
        let mut edited = opened.clone();
        edited
            .scripts
            .remove_trigger(&id, &event("tab.create.post"));
        edited.scripts.add_trigger(&id, event("pane.create.post"));
        edited
            .keybindings
            .script_bindings
            .push(crate::ScriptBinding {
                script_id: id.clone(),
                combo: "ctrl+alt+2".into(),
            });

        let out = merge(&opened, &edited, &current);
        assert!(out.conflicts.is_empty(), "{:?}", out.conflicts);
        let triggers = &out.settings.scripts.get(&id).unwrap().triggers;
        assert_eq!(
            triggers,
            &[event("pane.create.post"), event("tab.delete.post")]
        );
        let combos: Vec<&str> = out
            .settings
            .keybindings
            .script_bindings
            .iter()
            .map(|b| b.combo.as_str())
            .collect();
        assert_eq!(combos, ["ctrl+alt+2", "ctrl+alt+1"]);
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
