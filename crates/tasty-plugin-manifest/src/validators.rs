//! 매니페스트 검증에 쓰이는 형식 검사 / 예약 키워드 검사 자유 함수들.

pub(super) fn is_valid_plugin_id(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_')
        && s.contains('.')
}

pub(super) fn is_valid_kind(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
}

/// detector id 형식 검증 (manifest 측 schema 차원). 소문자 ascii + 숫자 + `-`,
/// 길이 1..=64. host 측 `file::format::types::is_valid_detector_id` 와 동일 규칙이며,
/// 본 함수는 manifest 가 host file 도메인 결합 없이 schema 검증을 마치기 위한
/// 자체 복제 — 두 함수가 어긋나면 install 단계에서 reject 된다.
pub(super) fn is_valid_simple_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 64 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// hook handler short-name 형식 검증 (manifest 측 schema 차원). 소문자 ascii + 숫자 +
/// `-`, 길이 1..=32. host 측 `hook_handler::types::is_valid_hook_handler_short_name`
/// 과 동일 규칙이며, 본 함수는 manifest 가 host hook_handler 도메인 결합 없이
/// `hook_handler.handle:<id>` scope 를 검증하기 위한 자체 복제다 — 두 함수가 어긋나면
/// install 단계에서 reject 된다.
pub(super) fn is_valid_hook_handler_id(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 완료 판정 전략 short-name 형식 검증 (manifest 측 schema 차원). 소문자 ascii +
/// 숫자 + `-`, 길이 1..=32 — hook handler short-name 과 동일 규칙(id 규약
/// 미러링). host 측 `completion_strategy::types::is_valid_completion_strategy_short_name`
/// 과 동일 규칙이며, 본 함수는 manifest 가 host completion_strategy 도메인 결합
/// 없이 schema 검증을 마치기 위한 자체 복제다.
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

/// 호스트가 자기 IPC 메서드에 쓰는 prefix들. plugin이 점유하면 호스트 메서드가 가려진다.
///
/// 호스트 메서드 표(`tasty_ipc::method_meta::METHOD_TABLE`)와 **집합으로 맞물려 있다** —
/// 본체의 `source_guards::reserved_ipc_prefixes` 가 양방향으로 대조하므로, 새 호스트
/// prefix 가 생기면 여기에 넣거나 왜 넣지 않는지를 그 가드에 적어야 한다. 이 크레이트가
/// 표를 직접 읽지 못하는 이유는 의존 방향이다 — `tasty-ipc` 가 이 크레이트를 쓴다.
pub const RESERVED_IPC_PREFIXES: &[&str] = &[
    "agent",
    "approval",
    "attach",
    "banner",
    "clipboard",
    "completion_strategy",
    "debug",
    "file_handler",
    "file_picker",
    "fs",
    "git_viewer",
    "global_hook",
    "hook",
    "hook_handler",
    // plugin ↔ host 보조 채널 계열(`host.shared_buffer.*`). 매니페스트로 이 이름을
    // 점유하면 그 뒤 호스트가 같은 prefix 에 메서드를 더할 때 표에 없는 `host.*` 가
    // plugin 으로 forward 된다.
    "host",
    "ime",
    "ipc",
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

/// 호스트가 자기 CLI 서브커맨드로 쓰는 명령들. plugin이 가로채면 호스트가 가려진다.
pub(super) fn is_reserved_cli_name(s: &str) -> bool {
    matches!(
        s,
        "plugin"
            | "new"
            | "close"
            | "list"
            | "set"
            | "send"
            | "read"
            | "move"
            | "split"
            | "tree"
            | "debug"
            | "wait"
            | "send-key"
            | "send-combo"
            | "surface-meta"
            | "is-typing"
            | "notify"
            | "unset"
    )
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

/// publish 패턴이 정확 키를 cover하는지. 매니페스트 검증된 패턴만 받는다.
///
/// - 패턴이 정확 키와 같으면 cover.
/// - 패턴이 `<prefix>.*`이고 키가 `<prefix>.<segment>` 형태면 cover.
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

/// `[[contributes.commands]] id` 형식 검증. `<namespace>.<action>` 관례
/// (예: `"explorer.refresh"`, `"clipboard.copy"`)를 따르되 강제하진 않는다 — `.`으로
/// 구분된 세그먼트(소문자 ascii + 숫자 + `_`, 알파벳 시작)가 1개 이상이면 된다.
/// 길이 1..=64. `contributes.tool`/`contributes.popup` id(대시 기반, `is_valid_tool_id`)와
/// 달리 command id는 기존 관례상 점(`.`) 구분자를 쓰므로 별도 규칙을 둔다.
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

/// 패턴의 최상위 namespace 세그먼트. 검증 통과 후 호출하면 절대 빈 값을 반환하지 않는다.
pub(super) fn event_pattern_namespace(s: &str) -> &str {
    s.split('.').next().unwrap_or("")
}

/// `[[contributes.hook_events]]` key 형식 검증. surface hook 이벤트 키는 점(.) 구분
/// 이벤트 버스 키(`is_valid_event_key`)와 달리 `process-exit` 류 kebab-case 식별자다.
/// 소문자 ascii + 숫자 + `-`. 알파벳으로 시작. 길이 1..=64. `:`/`*`/`.` 불가
/// (`:` 는 내장 prefix 이벤트와, `*` 는 와일드카드 혼동 방지).
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

/// host 가 코어에 내장한 surface hook 이벤트와 충돌하는지. plugin 이 점유하면
/// `HookEvent::parse` 가 내장 변형으로 먼저 해석해 plugin 선언이 죽으므로 거부한다.
/// 정확 이름 `process-exit`/`bell`/`notification` 과 prefix 이벤트
/// `output-match:`/`idle-timeout:` 을 막는다 (prefix 는 `:` 라 형식 검증에서 이미
/// 걸리지만 방어적으로 함께 검사).
pub(super) fn is_reserved_hook_event_key(s: &str) -> bool {
    matches!(s, "process-exit" | "bell" | "notification")
        || s.starts_with("output-match:")
        || s.starts_with("idle-timeout:")
}

/// 호스트만 publish 가능한 예약 네임스페이스.
/// plugin은 자기 도메인의 namespace로만 발화할 수 있다.
pub(super) fn is_reserved_event_namespace(ns: &str) -> bool {
    matches!(
        ns,
        "surface"
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
