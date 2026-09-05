//! 매니페스트 `contributes.cli`를 런타임에 clap 서브커맨드로 등록하고,
//! 매칭된 결과를 JSON-RPC 메서드+params로 변환한다.
//!
//! 호스트 정적 `Cli` 파싱이 `InvalidSubcommand`로 실패할 때 진입한다 — 정적 우선,
//! 정적이 모르는 이름만 plugin CLI에서 찾는다.
//!
//! 그 "정적 우선"은 **파싱 순서**의 성질이지 등록의 성질이 아니었다. 호스트와 같은
//! 이름을 선언한 plugin 도 clap 트리에는 그대로 들어갔고, 그러면 release 에서는
//! 도달 불가능한 중복 서브커맨드가 조용히 얹혔고 debug 에서는 clap 의 `assert_app`
//! 이 `command name '<이름>' is duplicated` 로 **CLI 전체를 패닉시켰다** —
//! `--help` 와 다른 모든 plugin 명령까지 함께 죽었다. 그래서 지금은
//! [`build_augmented_cli`] 가 등록 시점에 정적 명령 집합과 대조해 겹치는 이름을
//! 등록하지 않고 경고한다. 막히는 것은 release 에서 이미 도달 불가였던 이름뿐이라
//! 서드파티가 잃는 기능은 없다. 근거와 대안은
//! `docs/adr/0158-cli-name-collisions-are-judged-at-registration-not-in-the-manifest.md`.

mod build;
mod request;
mod stdin;

pub use build::{build_augmented_cli, discover_plugin_clis};
pub use request::matches_to_request;

use serde_json::{Map, Value};
use std::collections::HashMap;
use tasty_plugin_manifest::{CliCommandDecl, PollingDecl};

/// `spawn` / `tell` 같이 1 차 응답 후 chained wait 가 필요한 명령의 실행 계획.
/// `matches_to_request` 가 manifest `AutoWaitDecl` + 사용자 CLI 입력을 합쳐 빌드.
#[derive(Debug, Clone)]
pub struct AutoWaitPlan {
    pub method: String,
    pub polling: PollingDecl,
    pub map_from_response: HashMap<String, String>,
    pub map_from_request: HashMap<String, String>,
    pub timeout_field: String,
    /// 원 요청 params snapshot. wait params 구성 시 `map_from_request` 매핑과
    /// timeout 키 추출에 사용.
    pub request_params: Map<String, Value>,
    /// `--no-wait` 가 true 면 chain skip — caller 가 1 차 응답만 출력하고 종료.
    pub skipped: bool,
}

/// 한 plugin이 contribute한 CLI 묶음.
#[derive(Debug, Clone)]
pub struct PluginCliEntry {
    pub cli: CliCommandDecl,
}

#[cfg(test)]
mod tests;
