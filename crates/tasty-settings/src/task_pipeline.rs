//! 작업 파이프라인(타입 있는 task DAG) 설정. 지금은 report 크기 상한 두 개다.
//!
//! 숫자는 `tasty_agent::task::report` 의 기본값·범위와 같아야 한다. 이 크레이트는 저장소 크레이트에
//! 의존하지 않으므로 값을 따로 두고, 루트 패키지의 시험이 두 쪽을 대조한다.

use serde::{Deserialize, Serialize};

/// append 한 번의 기본 상한(바이트).
pub const DEFAULT_REPORT_APPEND_BYTES: u64 = 1024;
/// 회차 블록 하나의 기본 상한(바이트).
pub const DEFAULT_REPORT_BLOCK_BYTES: u64 = 16 * 1024;
/// append 상한으로 받는 범위.
pub const REPORT_APPEND_BYTES_RANGE: (u64, u64) = (64, 64 * 1024);
/// 블록 상한으로 받는 범위.
pub const REPORT_BLOCK_BYTES_RANGE: (u64, u64) = (128, 128 * 1024);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskPipelineSettings {
    /// custom 기록 한 항목의 상한. 넘으면 UTF-8 경계에서 자른다.
    pub report_append_bytes: u64,
    /// 회차 블록 하나에 저장하는 텍스트 합의 상한. 넘는 기록은 저장하지 않고 센다.
    pub report_block_bytes: u64,
}

impl Default for TaskPipelineSettings {
    fn default() -> Self {
        Self {
            report_append_bytes: DEFAULT_REPORT_APPEND_BYTES,
            report_block_bytes: DEFAULT_REPORT_BLOCK_BYTES,
        }
    }
}

impl TaskPipelineSettings {
    /// 범위와 `append < block` 을 확인한다. 적용과 파일 읽기가 같은 검사를 쓴다.
    pub fn validate(&self) -> Result<(), String> {
        let in_range = |v: u64, (lo, hi): (u64, u64), key: &str| {
            if (lo..=hi).contains(&v) {
                Ok(())
            } else {
                Err(format!("task_pipeline.{key} {v} is outside {lo}..={hi}"))
            }
        };
        in_range(
            self.report_append_bytes,
            REPORT_APPEND_BYTES_RANGE,
            "report_append_bytes",
        )?;
        in_range(
            self.report_block_bytes,
            REPORT_BLOCK_BYTES_RANGE,
            "report_block_bytes",
        )?;
        if self.report_append_bytes >= self.report_block_bytes {
            return Err(format!(
                "task_pipeline.report_append_bytes {} must be smaller than report_block_bytes {}",
                self.report_append_bytes, self.report_block_bytes
            ));
        }
        Ok(())
    }

    /// 설정 파일의 값이 검사를 통과하지 못하면 두 값을 기본값으로 되돌린다.
    pub(crate) fn normalize(&mut self, changed: &mut bool) {
        if let Err(e) = self.validate() {
            tracing::warn!("{e} → task_pipeline report limits reset to defaults");
            *self = Self::default();
            *changed = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_append_limit_must_stay_below_the_block_limit() {
        assert_eq!(TaskPipelineSettings::default().validate(), Ok(()));
        let s = |a, b| TaskPipelineSettings {
            report_append_bytes: a,
            report_block_bytes: b,
        };
        assert!(s(1024, 1024).validate().is_err());
        assert!(s(2048, 1024).validate().is_err());
        assert!(s(63, 1024).validate().is_err());
        assert!(s(64, 128).validate().is_ok());
        assert!(s(1024, 128 * 1024 + 1).validate().is_err());
    }

    #[test]
    fn a_file_with_a_broken_pair_loads_with_the_defaults() {
        let mut settings = crate::Settings::parse_with_migration(
            "[task_pipeline]\nreport_append_bytes = 4096\nreport_block_bytes = 2048\n",
        )
        .expect("parse");
        let report = settings.normalize();
        assert!(report.changed);
        assert_eq!(settings.task_pipeline, TaskPipelineSettings::default());

        let mut settings = crate::Settings::parse_with_migration(
            "[task_pipeline]\nreport_append_bytes = 512\nreport_block_bytes = 4096\n",
        )
        .expect("parse");
        settings.normalize();
        assert_eq!(settings.task_pipeline.report_append_bytes, 512);
        assert_eq!(settings.task_pipeline.report_block_bytes, 4096);
    }
}
