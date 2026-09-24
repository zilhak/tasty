//! 관측·비용 집계에 쓰는 에이전트 식별자.
//! 타입 자체는 문자열을 검증하지 않는다. CallerContext가 Agent 세션의 에이전트 ID와
//! 호스트 레지스트리의 Plugin ID를 가져온다. Local은 TASTY_AGENT_ID를 사용하며,
//! 이 값은 Local의 권한을 제한하거나 추가하는 수단이 아니다.

use std::fmt;

/// 호출 경로에서 신뢰 여부를 확인해야 하는 에이전트 ID.
/// 호스트 표기 HOST는 memory의 HOST_OWNER와 같아 owner 컬럼에 사용할 수 있다.
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

    /// 테스트 동안 환경변수 원값과 ENV_LOCK을 보관한다. 패닉으로 끝나도 Drop에서
    /// 원값을 복원한 뒤 락을 놓는다. 락은 ()만 보호하므로 poison을 복구한다.
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
        assert_eq!(
            AgentId::from_plugin_id("com.tasty.claude").as_str(),
            "com_tasty_claude"
        );
        assert_eq!(
            AgentId::from_plugin_id("plugin-1_foo").as_str(),
            "plugin-1_foo"
        );
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
        let env = AgentIdEnvGuard::new();

        env.unset();
        assert!(AgentId::from_env().is_host(), "unset should be host");

        env.set("");
        assert!(
            AgentId::from_env().is_host(),
            "empty string should be treated as host"
        );

        env.set("child_xyz");
        let a = AgentId::from_env();
        assert_eq!(a.as_str(), "child_xyz");
        assert!(!a.is_host());
    }
}
