#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]
//! Command palette state + 매칭 로직.
//!
//! 팔레트는 사용자 입력으로만 열리는 popup이다 (Ctrl+Shift+P 또는 Tools 메뉴).
//! popup이 query를 누적하고 후보를 매칭하면, Enter 시 `pending_run`에 action_id를
//! 적재한다. `MainView` 메인 루프가 매 프레임 drain하여 keybinding 단축키와 동일한
//! action body를 호출한다.
//!
//! 후보 목록은 `tasty_settings::KeybindingSettings::GENERAL_BINDING_FIELDS`에서
//! 가져온다. 새 단축키를 추가하면 자동으로 팔레트에도 노출되므로 별도 등록 필요 없음.

use tasty_settings::KeybindingSettings;

/// 팔레트가 보여줄 단일 항목.
#[derive(Debug, Clone, Copy)]
pub struct PaletteCommand {
    /// keybinding `field_id` (예: `"new_workspace"`)
    pub id: &'static str,
    /// i18n 키 (예: `"settings.keybindings.new_workspace_label"`)
    pub label_key: &'static str,
}

/// 실행 가능한 명령 전체 목록. 단축키 필드와 1:1 대응.
///
/// `toggle_command_palette` 자신은 제외한다 (이미 팔레트 안이므로 의미 없음).
pub fn all_commands() -> Vec<PaletteCommand> {
    KeybindingSettings::GENERAL_BINDING_FIELDS
        .iter()
        .filter(|(id, _)| *id != "toggle_command_palette")
        .map(|(id, label_key)| PaletteCommand { id, label_key })
        .collect()
}

/// 팔레트 UI 상태. `AppState`가 소유한다.
#[derive(Debug, Default)]
pub struct CommandPaletteState {
    /// 사용자 입력 쿼리.
    pub query: String,
    /// 현재 선택 인덱스 (필터링된 결과 기준).
    pub selected: usize,
    /// Enter 시 popup이 채워두면, MainView가 다음 프레임에 drain하여 dispatch한다.
    pub pending_run: Option<&'static str>,
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

    #[test]
    fn empty_query_returns_all() {
        let cmds = all_commands();
        let labels: Vec<String> = cmds.iter().map(|c| c.id.to_string()).collect();
        let results = search("", &cmds, &labels);
        assert_eq!(results.len(), cmds.len());
    }

    #[test]
    fn substring_outranks_subsequence() {
        let cmds = vec![
            PaletteCommand {
                id: "a",
                label_key: "",
            },
            PaletteCommand {
                id: "b",
                label_key: "",
            },
        ];
        // "open" appears verbatim in first label, scattered in second.
        let labels = vec!["Open file".to_string(), "Other punks even".to_string()];
        let results = search("open", &cmds, &labels);
        assert_eq!(results.first().unwrap().1.id, "a");
    }

    #[test]
    fn word_start_gets_bonus() {
        let cmds = vec![
            PaletteCommand {
                id: "early",
                label_key: "",
            },
            PaletteCommand {
                id: "late",
                label_key: "",
            },
        ];
        let labels = vec!["new tab".to_string(), "renew tab".to_string()];
        let results = search("new", &cmds, &labels);
        // "new tab" starts with "new" → higher score
        assert_eq!(results.first().unwrap().1.id, "early");
    }

    #[test]
    fn no_match_returns_empty() {
        let cmds = vec![PaletteCommand {
            id: "x",
            label_key: "",
        }];
        let labels = vec!["foo bar".to_string()];
        let results = search("xyz", &cmds, &labels);
        assert!(results.is_empty());
    }
}
