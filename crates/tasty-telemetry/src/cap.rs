//! 메트릭 누적값에 적용할 비용 제한과 초과 시 동작.

use serde::{Deserialize, Serialize};

use super::{Result, TelemetryError};

pub const CAP_KEY_PREFIX: &str = "tasty.telemetry.cap.";

/// `tasty.telemetry.cap.{id}`.
pub fn cap_key(id: &str) -> String {
    format!("{CAP_KEY_PREFIX}{id}")
}

/// Cap 평가 기간. `Total` 은 전 기간 누적 (보존 기간이 retention 정책 안에 있을 때 유효).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapWindow {
    Total,
    #[serde(rename = "1h")]
    Hour,
    #[serde(rename = "1d")]
    Day,
}

impl std::str::FromStr for CapWindow {
    type Err = TelemetryError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "total" => Ok(CapWindow::Total),
            "1h" => Ok(CapWindow::Hour),
            "1d" => Ok(CapWindow::Day),
            _ => Err(TelemetryError::InvalidWindow),
        }
    }
}

impl CapWindow {
    pub fn as_str(self) -> &'static str {
        match self {
            CapWindow::Total => "total",
            CapWindow::Hour => "1h",
            CapWindow::Day => "1d",
        }
    }

    /// 윈도우의 시간 범위 ms. `Total` 은 `None`.
    pub fn span_ms(self) -> Option<u64> {
        match self {
            CapWindow::Total => None,
            CapWindow::Hour => Some(3_600_000),
            CapWindow::Day => Some(86_400_000),
        }
    }
}

/// 제한 초과 시 호스트가 취할 동작. 파일의 stop은 호환용 alias로 Pause로 읽지만,
/// 신규 등록의 FromStr은 stop을 거절한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapAction {
    /// IPC 거부만. `reset` 까지 영구.
    #[serde(alias = "stop")]
    Pause,
    /// Approval 요청을 자동 발행하고 응답 따라 통과/거부.
    RequireApproval,
    /// 차단 없음. notification 만 발행.
    Notify,
}

impl std::str::FromStr for CapAction {
    type Err = TelemetryError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "stop" => Err(TelemetryError::Internal(
                "action 'stop' is no longer valid, use 'pause' instead".to_string(),
            )),
            "pause" => Ok(CapAction::Pause),
            "require_approval" => Ok(CapAction::RequireApproval),
            "notify" => Ok(CapAction::Notify),
            _ => Err(TelemetryError::Internal(format!("invalid action '{s}'"))),
        }
    }
}

impl CapAction {
    pub fn as_str(self) -> &'static str {
        match self {
            CapAction::Pause => "pause",
            CapAction::RequireApproval => "require_approval",
            CapAction::Notify => "notify",
        }
    }
}

/// Cap 발동 흔적.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapTriggered {
    /// unix ms.
    pub at: u64,
    /// 발동 당시의 측정 값.
    pub value: f64,
}

/// Cost cap — 메트릭 누적값에 대한 임계와 동작.
///
/// `triggered` 가 `Some` 이면 이미 발동된 상태. 호스트가 cap 평가 시 이 상태를
/// 그대로 적용해 IPC 를 거부/차단한다. `reset` 호출로 `None` 로 되돌릴 수 있다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostCap {
    pub id: String,
    pub agent: String,
    pub metric: String,
    pub threshold: f64,
    pub window: CapWindow,
    pub action: CapAction,
    /// unix ms.
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub triggered: Option<CapTriggered>,
}

impl CostCap {
    pub fn is_triggered(&self) -> bool {
        self.triggered.is_some()
    }
}
