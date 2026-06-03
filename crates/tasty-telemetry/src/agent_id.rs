//! Agent 식별자 — Phase 4 (관측/비용) 잠정 모델.
//!
//! 현재 위조 가능: env `TASTY_AGENT_ID` 또는 plugin manifest id 를 그대로 신뢰한다.
//! Phase 6 의 session token 인증이 도입되면 verifiable 로 승격한다.
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
        let _guard = ENV_LOCK.lock().unwrap();
        // SAFETY: mutex 로 다른 env-touching 테스트와 직렬화. 프로세스 다른 곳에서
        // TASTY_AGENT_ID 를 동시에 만지지 않는다는 가정 (단위 테스트 한정).

        // 1) unset → host
        unsafe { std::env::remove_var(AgentId::ENV_KEY) };
        assert!(AgentId::from_env().is_host(), "unset should be host");

        // 2) empty → host
        // SAFETY: ENV_LOCK 가드로 직렬화된 단위 테스트 한정. 본 모듈 위 SAFETY 주석 참조.
        unsafe { std::env::set_var(AgentId::ENV_KEY, "") };
        assert!(
            AgentId::from_env().is_host(),
            "empty string should be treated as host"
        );

        // 3) value → that value
        // SAFETY: ENV_LOCK 가드로 직렬화된 단위 테스트 한정. 본 모듈 위 SAFETY 주석 참조.
        unsafe { std::env::set_var(AgentId::ENV_KEY, "child_xyz") };
        let a = AgentId::from_env();
        assert_eq!(a.as_str(), "child_xyz");
        assert!(!a.is_host());

        // cleanup
        // SAFETY: ENV_LOCK 가드로 직렬화된 단위 테스트 한정. 본 모듈 위 SAFETY 주석 참조.
        unsafe { std::env::remove_var(AgentId::ENV_KEY) };
    }
}
