//! 매니페스트 검증에 쓰이는 형식 검사 / 예약 키워드 검사 자유 함수들.

pub(super) use tasty_utils::plugin_id::is_valid_plugin_id;

pub(super) fn is_valid_kind(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
}

/// 감지기 ID: 영문 소문자·숫자·하이픈, 길이 1..=64. 호스트와 별도로 기본 형식을 검사한다.
pub(super) fn is_valid_simple_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 훅 핸들러 이름: 영문 소문자·숫자·하이픈, 길이 1..=32.
pub(super) fn is_valid_hook_handler_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 완료 전략 이름: 영문 소문자·숫자·하이픈, 길이 1..=32.
pub(super) fn is_valid_completion_strategy_id(s: &str) -> bool {
    is_valid_hook_handler_id(s)
}

/// IPC namespace prefix 형식 검증.
/// 소문자 ascii + 숫자 + `_`. 알파벳으로 시작. 길이 1..=32. `.` 포함 불가.
pub(super) fn is_valid_ipc_prefix(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// 호스트 IPC가 사용하는 접두어. 플러그인이 같은 이름을 등록하지 못하게 한다.
/// 본체의 reserved_ipc_prefixes 가드가 실제 메서드 목록과 대조한다.
/// 의존 방향상 여기서는 tasty-ipc의 목록을 직접 읽을 수 없다.
pub const RESERVED_IPC_PREFIXES: &[&str] = &[
    "agent",
    "approval",
    "attach",
    "banner",
    "clipboard",
    "completion_strategy",
    "debug",
    "events",
    "file_handler",
    "file_picker",
    "fs",
    "git_viewer",
    "global_hook",
    "hook",
    "hook_handler",
    // 호스트 보조 채널과 이후 추가될 같은 접두어의 메서드도 보호한다.
    "host",
    "ime",
    "ipc",
    "markdown_mirror",
    "memory",
    "message",
    "notification",
    "output",
    "pane",
    "plugin",
    "popup",
    "preset",
    "pty",
    "recent",
    "remote",
    "session",
    "settings",
    "split",
    "surface",
    "system",
    "tab",
    "telemetry",
    "terminal",
    "theme",
    "timer",
    "tool",
    "tree",
    "ui",
    "view",
    "webhook",
    "webview",
    "window",
    "workspace",
    "workspace_category",
];

pub(super) fn is_reserved_ipc_prefix(s: &str) -> bool {
    RESERVED_IPC_PREFIXES.contains(&s)
}

/// CLI 명령 이름 형식 검증.
/// 소문자 ascii + 숫자 + `-`. 알파벳으로 시작. 길이 1..=32.
pub(super) fn is_valid_tool_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    let first = s.chars().next().unwrap();
    if !first.is_ascii_lowercase() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// `[[contributes.settings_pages]]` id 와 item id 의 형식 검증.
/// 비어있지 않고 영숫자(소문자) + `_` + `-` 만 허용. 길이 1..=64.
pub(super) fn is_valid_settings_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

pub(super) fn is_valid_cli_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    let first = s.chars().next().unwrap();
    if !first.is_ascii_lowercase() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Event Bus 패턴 검증. 정확한 키 또는 `<namespace>(.<segment>)*.*` 형태.
///
/// - `surface.created`: 정확한 key → 허용
/// - `surface.*`: namespace 와일드카드 → 허용
/// - `surface.lifecycle.*`: 깊이 2 와일드카드 → 허용
/// - `*`, `*.bar`, `foo.*.bar`, `foo*`, 빈 문자열 → 거부
pub(super) fn is_valid_event_pattern(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let segments: Vec<&str> = s.split('.').collect();
    if segments.len() < 2 {
        // 모든 이벤트는 `<namespace>.<name>` 최소 2 세그먼트.
        return false;
    }
    for (i, seg) in segments.iter().enumerate() {
        let is_last = i + 1 == segments.len();
        if *seg == "*" {
            if !is_last {
                return false;
            }
            continue;
        }
        if !is_valid_event_segment(seg) {
            return false;
        }
    }
    true
}

/// 와일드카드를 허용하지 않는 정확 이벤트 키 검증. `events_emitted.key`에 사용.
pub(super) fn is_valid_event_key(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let segments: Vec<&str> = s.split('.').collect();
    if segments.len() < 2 {
        return false;
    }
    segments.iter().all(|seg| is_valid_event_segment(seg))
}

/// 유효한 발행 패턴에 이벤트 키가 포함되는지 확인한다. 정확히 같거나 prefix.*에 속해야 한다.
pub(super) fn event_pattern_covers(pattern: &str, key: &str) -> bool {
    if pattern == key {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix(".*")
        && let Some(rest) = key.strip_prefix(prefix)
    {
        return rest.starts_with('.') && rest.len() > 1;
    }
    false
}

/// 명령 ID: 점으로 나눈 영문 소문자·숫자·밑줄, 각 부분은 알파벳으로 시작한다.
/// 전체 길이는 1..=64이며 namespace.action 형식을 권장한다.
pub(super) fn is_valid_command_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    s.split('.').all(is_valid_event_segment)
}

fn is_valid_event_segment(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

/// 검증된 이벤트 키의 첫 부분을 namespace로 사용한다.
pub(super) fn event_pattern_namespace(s: &str) -> &str {
    s.split('.').next().unwrap_or("")
}

/// 훅 이벤트 키: 영문 소문자·숫자·하이픈, 알파벳으로 시작하며 길이는 1..=64.
/// 이벤트 버스와 달리 점 구분이나 와일드카드·콜론을 허용하지 않는다.
pub(super) fn is_valid_hook_event_key(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    let first = s.chars().next().unwrap();
    if !first.is_ascii_lowercase() {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 내장 훅 이름과 접두어를 플러그인이 선언하지 못하게 한다.
/// 콜론은 앞선 형식 검사에서도 거부하지만 여기서도 확인한다.
pub(super) fn is_reserved_hook_event_key(s: &str) -> bool {
    matches!(s, "process-exit" | "bell" | "notification")
        || s.starts_with("output-match:")
        || s.starts_with("idle-timeout:")
}

/// 플러그인이 발행할 수 없는 호스트 전용 namespace.
pub(super) fn is_reserved_event_namespace(ns: &str) -> bool {
    matches!(
        ns,
        "agent"
            | "surface"
            | "tab"
            | "pane"
            | "split"
            | "workspace"
            | "window"
            | "clipboard"
            | "plugin"
            | "extension"
            | "tool"
            | "command"
            | "ime"
            | "theme"
            | "language"
            | "notification"
            | "hook"
            | "process"
            | "memory"
            | "system"
    )
}
