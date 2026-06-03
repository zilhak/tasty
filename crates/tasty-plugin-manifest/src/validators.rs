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
pub(super) fn is_reserved_ipc_prefix(s: &str) -> bool {
    matches!(
        s,
        "plugin"
            | "system"
            | "surface"
            | "tab"
            | "pane"
            | "workspace"
            | "split"
            | "tree"
            | "hook"
            | "global_hook"
            | "message"
            | "tool"
            | "notification"
            | "window"
            | "debug"
            | "ui"
            | "ime"
            | "ipc"
            | "memory"
            | "output"
            | "approval"
    )
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
