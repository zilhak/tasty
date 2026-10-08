//! 설정의 작업 파이프라인 상한을 task runtime 의 report 상한으로 옮긴다.

use tasty_agent::task::report::ReportLimits;
use tasty_settings::TaskPipelineSettings;

/// 검사를 통과하지 못한 값(직접 고친 설정 파일 등)은 기본 상한으로 바꾼다.
pub(crate) fn report_limits(settings: &TaskPipelineSettings) -> ReportLimits {
    if let Err(e) = settings.validate() {
        tracing::warn!("{e}; the task report uses the default limits");
        return ReportLimits::default();
    }
    ReportLimits {
        append_bytes: settings.report_append_bytes,
        block_bytes: settings.report_block_bytes,
    }
}

/// 러너 스레드와 IPC 가 읽는 상한을 설정 값으로 바꾼다.
pub(crate) fn apply(core: &crate::app::services::AppServices, settings: &tasty_settings::Settings) {
    core.tasks
        .set_report_limits(report_limits(&settings.task_pipeline));
}

#[cfg(test)]
mod tests {
    use tasty_agent::task::report as r;

    use super::*;

    /// 설정 크레이트는 저장소 크레이트에 의존하지 않아 같은 숫자를 따로 둔다. 둘이 갈리면 화면의
    /// 범위와 실제로 받는 범위가 달라진다.
    #[test]
    fn the_settings_numbers_match_the_report_store() {
        assert_eq!(
            tasty_settings::DEFAULT_REPORT_APPEND_BYTES,
            r::DEFAULT_APPEND_LIMIT
        );
        assert_eq!(
            tasty_settings::DEFAULT_REPORT_BLOCK_BYTES,
            r::DEFAULT_BLOCK_LIMIT
        );
        assert_eq!(
            tasty_settings::REPORT_APPEND_BYTES_RANGE,
            r::APPEND_LIMIT_RANGE
        );
        assert_eq!(
            tasty_settings::REPORT_BLOCK_BYTES_RANGE,
            r::BLOCK_LIMIT_RANGE
        );
        assert_eq!(
            report_limits(&TaskPipelineSettings::default()),
            ReportLimits::default()
        );
    }

    #[test]
    fn a_pair_that_fails_the_check_falls_back_to_the_defaults() {
        let bad = TaskPipelineSettings {
            report_append_bytes: 4096,
            report_block_bytes: 2048,
        };
        assert_eq!(report_limits(&bad), ReportLimits::default());
        let ok = TaskPipelineSettings {
            report_append_bytes: 512,
            report_block_bytes: 4096,
        };
        let limits = report_limits(&ok);
        assert_eq!((limits.append_bytes, limits.block_bytes), (512, 4096));
        assert_eq!(limits.validate(), Ok(()));
    }
}
