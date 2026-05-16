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
    fn from_env_all_cases() {
        let _guard = ENV_LOCK.lock().unwrap();
        // SAFETY: mutex 로 다른 env-touching 테스트와 직렬화. 프로세스 다른 곳에서
        // TASTY_AGENT_ID 를 동시에 만지지 않는다는 가정 (단위 테스트 한정).

        // 1) unset → host
        unsafe { std::env::remove_var(AgentId::ENV_KEY) };
        assert!(AgentId::from_env().is_host(), "unset should be host");

        // 2) empty → host
        unsafe { std::env::set_var(AgentId::ENV_KEY, "") };
        assert!(
            AgentId::from_env().is_host(),
            "empty string should be treated as host"
        );

        // 3) value → that value
        unsafe { std::env::set_var(AgentId::ENV_KEY, "child_xyz") };
        let a = AgentId::from_env();
        assert_eq!(a.as_str(), "child_xyz");
        assert!(!a.is_host());

        // cleanup
        unsafe { std::env::remove_var(AgentId::ENV_KEY) };
    }
}
