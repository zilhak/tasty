//! Agent 식별자 — 관측/비용 추적용 모델.
//!
//! [`AgentId`] 타입 자체는 아무 문자열이나 담을 수 있는 plain wrapper다. 위조
//! 가능 여부는 이 값을 *누가 채워 넣는지*(신뢰 경계를 넘는 지점)에 달려 있다 —
//! 신뢰 경계를 넘나드는 `Plugin`/`Agent` 두 caller 경로는 이미
//! `CallerContext`(`crates/tasty-ipc/src/caller.rs`) 레이어에서 검증된 값만
//! 흘려보낸다: `Agent` 는 `SessionToken` 검증을 통과해야만 생성되는 호스트-부여
//! `agent_id`, `Plugin` 은 호스트 dispatch 코드가 자신의 plugin 레지스트리에서
//! 직접 구성하는 `plugin_id` 라 외부 IPC 페이로드로 자유롭게 실어 보낼 수 없다.
//! `Local`(CLI/네트워크 IPC 클라이언트)만 env `TASTY_AGENT_ID` 를 그대로 신뢰하는데,
//! `Local` 은 애초에 권한 검사 없이 무제한 허용되는 경로라(로컬 기기 액세스 전제)
//! 이 값을 무엇으로 설정하든 telemetry 라벨링 외엔 영향이 없다 — 위조해도 얻는
//! 게 없는 의도된 설계다. 자세한 근거는 `tasty-ipc` crate 의
//! `CallerContext::agent_id` doc 참고.
//!
//! 사용처:
//! - `telemetry.record` 등 메트릭의 agent 차원
//! - dispatcher 미들웨어가 caller 식별
//! - approval/cap 의 owner

use std::fmt;

/// 위조 가능한 잠정 agent 식별자.
///
/// 호스트 자신을 가리키는 sentinel 은 [`AgentId::HOST`] (`"_host"`) — `tasty_memory::HOST_OWNER`
/// 와 동일한 문자열. memory 의 `owner` 컬럼과 호환된다.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AgentId(String);

impl AgentId {
    /// 호스트(=Local CLI/사용자) 를 가리키는 sentinel.
    pub const HOST: &'static str = "_host";

    /// env `TASTY_AGENT_ID` 환경변수 이름.
    pub const ENV_KEY: &'static str = "TASTY_AGENT_ID";

    /// 임의 식별자로 직접 생성. 빈 문자열은 [`AgentId::HOST`] 로 대체된다.
    pub fn new(value: impl Into<String>) -> Self {
        let v = value.into();
        if v.is_empty() {
            Self(Self::HOST.into())
        } else {
            Self(v)
        }
    }

    /// 호스트 sentinel.
    pub fn host() -> Self {
        Self(Self::HOST.into())
    }

    /// env 기반 도출 — `TASTY_AGENT_ID` 가 있으면 그 값, 없으면 [`AgentId::HOST`].
    ///
    /// `CallerContext` 같은 본 바이너리 타입을 사용할 수 없는 라이브러리 (예:
    /// `tasty-telemetry`) 가 자기 caller 를 식별할 때 부른다.
    pub fn from_env() -> Self {
        match std::env::var(Self::ENV_KEY) {
            Ok(v) if !v.is_empty() => Self(v),
            _ => Self::host(),
        }
    }

    /// Plugin id (reverse-domain `com.foo.bar` 형식) → telemetry-safe agent id.
    ///
    /// `validate_agent_id` 는 `[a-zA-Z0-9_-]` (최대 64자) 만 허용하지만, plugin
    /// manifest id 는 통상 점을 포함한다 (`com.tasty.claude`). 점·기타 비허용
    /// 문자는 `_` 로 치환해 telemetry 도메인에 안전한 식별자로 만든다.
    /// 64자 초과 시 절단, 빈 입력은 host sentinel.
    pub fn from_plugin_id(plugin_id: &str) -> Self {
        let mut out = String::with_capacity(plugin_id.len());
        for c in plugin_id.chars() {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                out.push(c);
            } else {
                out.push('_');
            }
        }
        if out.is_empty() {
            return Self::host();
        }
        if out.len() > 64 {
            out.truncate(64);
        }
        Self(out)
    }

    /// 호스트 sentinel 인가?
    pub fn is_host(&self) -> bool {
        self.0 == Self::HOST
    }

    /// 내부 문자열 슬라이스. memory `owner` 컬럼에 그대로 넘길 수 있다.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AgentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for AgentId {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<&str> for AgentId {
    fn from(s: &str) -> Self {
        Self::new(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// env 조작은 프로세스 전역이라 병렬 cargo test 에서 충돌한다.
    /// `from_env` 검증을 하나의 직렬 테스트로 묶고 mutex 로 보호한다.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// `TASTY_AGENT_ID` 를 테스트 동안만 바꿔두고 **원값으로 되돌리는** 가드.
    ///
    /// 이 키는 tasty 자식 터미널 환경에서 실제로 설정돼 있다. 테스트가 마지막에
    /// `remove_var` 로 "정리" 하면 그 실값을 잃고, 단언이 패닉하면 정리 자체가
    /// 건너뛰어져 — 어느 쪽이든 같은 프로세스의 뒤따르는 테스트가 오염된 env 를
    /// 물려받는다.
    ///
    /// **생성자가 [`ENV_LOCK`] 을 직접 쥔다** — 호출부가 잊어도 env 격리가 깨지지
    /// 않는다(락은 가드 수명 동안 `_lock` 필드로 유지되고, Drop 이 env 를 되돌린 뒤
    /// 풀린다). poison 은 복구한다(락은 `()` 라 오염될 상태가 없다).
    struct AgentIdEnvGuard {
        prev: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl AgentIdEnvGuard {
        fn new() -> Self {
            let _lock = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
            Self {
                prev: std::env::var_os(AgentId::ENV_KEY),
                _lock,
            }
        }
        fn set(&self, v: &str) {
            // SAFETY: ENV_LOCK 가드로 직렬화된 단위 테스트 한정.
            unsafe { std::env::set_var(AgentId::ENV_KEY, v) };
        }
        fn unset(&self) {
            // SAFETY: set 과 동일 — ENV_LOCK 로 직렬화된 단위 테스트 한정.
            unsafe { std::env::remove_var(AgentId::ENV_KEY) };
        }
    }

    impl Drop for AgentIdEnvGuard {
        fn drop(&mut self) {
            match &self.prev {
                // SAFETY: set 과 동일 — ENV_LOCK 로 직렬화된 단위 테스트 한정.
                Some(v) => unsafe { std::env::set_var(AgentId::ENV_KEY, v) },
                // SAFETY: 상동.
                None => unsafe { std::env::remove_var(AgentId::ENV_KEY) },
            }
        }
    }

    #[test]
    fn new_empty_falls_back_to_host() {
        assert!(AgentId::new("").is_host());
        assert!(AgentId::host().is_host());
    }

    #[test]
    fn non_empty_keeps_value() {
        let a = AgentId::new("child_abc");
        assert_eq!(a.as_str(), "child_abc");
        assert!(!a.is_host());
    }

    #[test]
    fn display_matches_inner() {
        let a = AgentId::new("x");
        assert_eq!(format!("{a}"), "x");
        assert_eq!(format!("{}", AgentId::host()), "_host");
    }

    #[test]
    fn from_plugin_id_sanitizes_dots_and_other_chars() {
        // 점은 telemetry 검증에서 거부되므로 _ 로 치환되어야 한다.
        assert_eq!(
            AgentId::from_plugin_id("com.tasty.claude").as_str(),
            "com_tasty_claude"
        );
        // 허용 문자 (alnum, _, -) 는 그대로.
        assert_eq!(
            AgentId::from_plugin_id("plugin-1_foo").as_str(),
            "plugin-1_foo"
        );
        // 기타 비허용 문자도 _ 로.
        assert_eq!(AgentId::from_plugin_id("a/b@c").as_str(), "a_b_c");
        // 결과는 validate_agent_id 를 통과해야 한다.
        assert!(
            crate::validate_agent_id(AgentId::from_plugin_id("com.tasty.claude").as_str()).is_ok()
        );
    }

    #[test]
    fn from_plugin_id_truncates_over_64_chars() {
        let long = "a".repeat(100);
        let a = AgentId::from_plugin_id(&long);
        assert_eq!(a.as_str().len(), 64);
        assert!(crate::validate_agent_id(a.as_str()).is_ok());
    }

    #[test]
    fn from_plugin_id_empty_falls_back_to_host() {
        assert!(AgentId::from_plugin_id("").is_host());
    }

    #[test]
    fn from_env_all_cases() {
        // 가드 생성자가 ENV_LOCK 을 직접 쥐고, 스코프 종료 시(패닉 포함) 실행 환경의
        // 원래 TASTY_AGENT_ID 를 되돌린 뒤 락을 푼다.
        let env = AgentIdEnvGuard::new();

        // 1) unset → host
        env.unset();
        assert!(AgentId::from_env().is_host(), "unset should be host");

        // 2) empty → host
        env.set("");
        assert!(
            AgentId::from_env().is_host(),
            "empty string should be treated as host"
        );

        // 3) value → that value
        env.set("child_xyz");
        let a = AgentId::from_env();
        assert_eq!(a.as_str(), "child_xyz");
        assert!(!a.is_host());
    }
}
