// 이유: 팔레트 상태를 읽고 쓰는 것이 gui 어댑터뿐이라 headless 빌드엔 호출자가 없다. 모듈을
// `#[cfg]` 로 가리지 않는 것은 headless 에서도 타입체크를 받게 하려는 것이다.
#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]
//! Command palette state + 매칭 로직.
//!
//! 팔레트는 사용자 입력으로만 열리는 popup이다 (Ctrl+Shift+P 또는 Tools 메뉴).
//! popup이 query를 누적하고 후보를 매칭하면, Enter 시 `pending_run`에 실행할 명령을
//! 적재한다. `MainView` 메인 루프가 매 프레임 drain하여 호스트 명령은 keybinding
//! 단축키와 동일한 action body를, plugin 명령은 App 메인 루프의 dispatch 큐를
//! 호출한다 (`src/view/main/redraw.rs`, `src/app/dispatch/palette_plugin_commands.rs`).
//!
//! 후보 목록은 두 출처를 합친다:
//! - 호스트: `tasty_settings::KeybindingSettings::GENERAL_BINDING_FIELDS`. 새
//!   단축키를 추가하면 자동으로 팔레트에도 노출되므로 별도 등록 필요 없음.
//! - Plugin: `AppState.palette_plugin_commands`(`PluginManager::plugin_palette_commands()`
//!   snapshot). **`CommandScope::Global`만** 노출한다 — `Surface` scope
//!   명령은 owner plugin의 surface가 포커스되어 있을 때만 의미가 있는데, 팔레트
//!   실행 시점엔 그 컨텍스트를 보장할 수 없다(포커스 없이 매칭되는 키보드 단축키
//!   경로 `match_global_shortcut`과 동일 판단 — `plugin_palette_commands()`가 이미
//!   `iter_global()`로 필터링해 snapshot에 담아준다).

use tasty_settings::KeybindingSettings;

/// 팔레트가 보여줄 단일 항목. 실행 시 필요한 최소 식별 정보를 담아 `pending_run`에도
/// 그대로 재사용한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaletteCommand {
    /// 호스트 내장 명령 (keybinding field).
    Host {
        /// keybinding `field_id` (예: `"new_workspace"`)
        id: &'static str,
        /// i18n 키 (예: `"settings.keybindings.new_workspace_label"`)
        label_key: &'static str,
    },
    /// Plugin이 `[[contributes.commands]]`로 선언한 전역(scope=Global) command.
    Plugin {
        plugin_id: String,
        command_id: String,
        /// plugin 자신의 lang 네임스페이스에서 해석해야 하는 i18n 키.
        title_i18n_key: String,
    },
}

/// 팔레트 목록에서 제외할 keybinding field_id.
///
/// - `toggle_command_palette`: 이미 팔레트 안이므로 자기 자신을 여는 명령은 무의미하다.
/// - `fullscreen_stage_exit`: 무대가 올라와 있으면 0단계 게이트가 모든 키를 소비해
///   팔레트를 **열 수 없고**, 무대가 없으면 이 명령은 아무 것도 닫을 게 없는 no-op 이다.
///   즉 팔레트에서 고를 수 있는 시점과 의미가 있는 시점이 서로 배타적이라 노출하지 않는다.
const PALETTE_EXCLUDED: &[&str] = &["toggle_command_palette", "fullscreen_stage_exit"];

/// 실행 가능한 명령 전체 목록: 호스트 keybinding 필드 + `plugin_commands`(팔레트에
/// 노출할 plugin 전역 command snapshot, 이미 비활성 plugin 필터링됨).
///
/// [`PALETTE_EXCLUDED`] 의 field_id 는 목록에서 뺀다.
pub fn all_commands(
    plugin_commands: &[crate::plugin::command_registry::PluginCommandEntry],
) -> Vec<PaletteCommand> {
    let mut out: Vec<PaletteCommand> = KeybindingSettings::GENERAL_BINDING_FIELDS
        .iter()
        .filter(|(id, _)| !PALETTE_EXCLUDED.contains(id))
        .map(|(id, label_key)| PaletteCommand::Host { id, label_key })
        .collect();
    out.extend(plugin_commands.iter().map(|e| PaletteCommand::Plugin {
        plugin_id: e.plugin_id.clone(),
        command_id: e.command_id.clone(),
        title_i18n_key: e.title_i18n_key.clone(),
    }));
    out
}

/// 팔레트 UI 상태. `AppState`가 소유한다.
#[derive(Debug, Default)]
pub struct CommandPaletteState {
    /// 사용자 입력 쿼리.
    pub query: String,
    /// 현재 선택 인덱스 (필터링된 결과 기준).
    pub selected: usize,
    /// Enter 시 popup이 채워두면, MainView가 다음 프레임에 drain하여 dispatch한다.
    pub pending_run: Option<PaletteCommand>,
}

impl CommandPaletteState {
    pub fn reset(&mut self) {
        self.query.clear();
        self.selected = 0;
    }
}

/// 쿼리 문자열을 명령 목록에 적용하여 `(score, command)` 쌍을 반환한다.
///
/// 매칭 알고리즘은 단순 case-insensitive subsequence + word-prefix bonus.
/// 외부 crate 없이 동작하며, 후보가 수십 개 규모라 성능은 문제되지 않는다.
pub fn search<'a>(
    query: &str,
    commands: &'a [PaletteCommand],
    labels: &[String],
) -> Vec<(i32, &'a PaletteCommand)> {
    let q = query.trim();
    if q.is_empty() {
        return commands.iter().map(|c| (0, c)).collect();
    }
    let q_lower = q.to_lowercase();

    let mut scored: Vec<(i32, &PaletteCommand)> = Vec::new();
    for (cmd, label) in commands.iter().zip(labels.iter()) {
        let label_lower = label.to_lowercase();
        if let Some(score) = match_score(&q_lower, &label_lower) {
            scored.push((score, cmd));
        }
    }
    scored.sort_by_key(|s| std::cmp::Reverse(s.0));
    scored
}

/// `query`(이미 lowercase)가 `text`(이미 lowercase)의 부분 시퀀스인지 확인하고,
/// 매칭 시 점수를 반환. 없으면 None.
///
/// 점수 룰:
/// - 정확 substring 매칭: 1000 + (단어 시작 위치면 +500) - 시작 인덱스
/// - 부분 시퀀스 매칭: 100 - 매칭 간격 합
fn match_score(query: &str, text: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    if let Some(idx) = text.find(query) {
        let mut score = 1000 - (idx as i32);
        let is_word_start = idx == 0
            || text
                .as_bytes()
                .get(idx.wrapping_sub(1))
                .map(|b| !b.is_ascii_alphanumeric())
                .unwrap_or(true);
        if is_word_start {
            score += 500;
        }
        return Some(score);
    }
    // Subsequence match
    let mut chars = query.chars();
    let mut current = chars.next()?;
    let mut last_idx: Option<usize> = None;
    let mut gap_sum: i32 = 0;
    for (i, ch) in text.char_indices() {
        if ch == current {
            if let Some(prev) = last_idx {
                gap_sum += (i - prev) as i32;
            }
            last_idx = Some(i);
            match chars.next() {
                Some(c) => current = c,
                None => return Some(100 - gap_sum),
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(id: &'static str) -> PaletteCommand {
        PaletteCommand::Host { id, label_key: "" }
    }

    #[test]
    fn empty_query_returns_all() {
        let cmds = all_commands(&[]);
        let labels: Vec<String> = cmds
            .iter()
            .map(|c| match c {
                PaletteCommand::Host { id, .. } => id.to_string(),
                PaletteCommand::Plugin { command_id, .. } => command_id.clone(),
            })
            .collect();
        let results = search("", &cmds, &labels);
        assert_eq!(results.len(), cmds.len());
    }

    /// [`PALETTE_EXCLUDED`] 가 실제로 목록에서 빠지는지. `fullscreen_stage_exit` 은
    /// 팔레트를 열 수 있는 시점(무대 없음)과 의미가 있는 시점(무대 있음)이 배타적이라
    /// 노출하지 않는다 — 목록에 되살아나면 아무 것도 하지 않는 명령이 검색에 잡힌다.
    #[test]
    fn excluded_ids_are_absent_from_the_palette() {
        let cmds = all_commands(&[]);
        for excluded in PALETTE_EXCLUDED {
            assert!(
                !cmds
                    .iter()
                    .any(|c| matches!(c, PaletteCommand::Host { id, .. } if id == excluded)),
                "'{excluded}' 가 팔레트 목록에 노출됐다"
            );
        }
        // 제외 목록이 통째로 비면 위 단정이 공허해진다 — 대조군으로 일반 액션 하나를
        // 확인해 필터가 전부를 걸러내지 않는다는 것도 함께 고정한다.
        assert!(
            cmds.iter()
                .any(|c| matches!(c, PaletteCommand::Host { id, .. } if *id == "new_tab")),
            "일반 액션까지 걸러졌다 — 필터가 과하게 잡는다"
        );
    }

    #[test]
    fn plugin_commands_are_appended() {
        let plugin_entry = crate::plugin::command_registry::PluginCommandEntry {
            plugin_id: "com.example.a".to_string(),
            command_id: "a.open".to_string(),
            title_i18n_key: "a.open.title".to_string(),
            manifest_default: None,
            binding_mode: tasty_plugin_manifest::BindingMode::Independent,
            scope: tasty_plugin_manifest::CommandScope::Global,
            action: None,
        };
        let cmds = all_commands(std::slice::from_ref(&plugin_entry));
        assert!(cmds.iter().any(|c| matches!(
            c,
            PaletteCommand::Plugin { plugin_id, command_id, .. }
                if plugin_id == "com.example.a" && command_id == "a.open"
        )));
    }

    #[test]
    fn substring_outranks_subsequence() {
        let cmds = vec![host("a"), host("b")];
        // "open" appears verbatim in first label, scattered in second.
        let labels = vec!["Open file".to_string(), "Other punks even".to_string()];
        let results = search("open", &cmds, &labels);
        assert_eq!(results.first().unwrap().1, &cmds[0]);
    }

    #[test]
    fn word_start_gets_bonus() {
        let cmds = vec![host("early"), host("late")];
        let labels = vec!["new tab".to_string(), "renew tab".to_string()];
        let results = search("new", &cmds, &labels);
        // "new tab" starts with "new" → higher score
        assert_eq!(results.first().unwrap().1, &cmds[0]);
    }

    #[test]
    fn no_match_returns_empty() {
        let cmds = vec![host("x")];
        let labels = vec!["foo bar".to_string()];
        let results = search("xyz", &cmds, &labels);
        assert!(results.is_empty());
    }
}
