//! Apply validated configuration to the shared detector without resetting observation state.
use tasty_settings::{AnomalyDetectionSettings, Settings};
use tasty_telemetry::{AnomalyKind, AnomalyThresholds};

pub(crate) fn thresholds(settings: &AnomalyDetectionSettings) -> AnomalyThresholds {
    if let Err(error) = settings.validate() {
        tracing::warn!(%error,"anomaly detector uses default thresholds");
        return AnomalyThresholds::default();
    }
    AnomalyThresholds {
        call_burst_window_ms: settings.call_burst_window_secs * 1000,
        call_burst_threshold: settings.call_burst_threshold as usize,
        slow_loop_window_ms: settings.slow_loop_window_secs * 1000,
        slow_loop_threshold: settings.slow_loop_threshold as usize,
        rss_surge_samples: settings.rss_surge_samples as usize,
        rss_surge_min_bytes: settings.rss_surge_min_mib * tasty_telemetry::MIB,
    }
}
pub(crate) fn apply(core: &super::services::AppServices, settings: &Settings) {
    if let Err(error) = core
        .anomaly_detector
        .set_thresholds(thresholds(&settings.anomaly_detection))
    {
        tracing::warn!(%error,"anomaly thresholds not applied");
    }
}
pub(crate) fn should_notify(settings: &AnomalyDetectionSettings, kind: AnomalyKind) -> bool {
    match kind {
        AnomalyKind::CallBurst => settings.call_burst_notify,
        AnomalyKind::SlowLoop => settings.slow_loop_notify,
        AnomalyKind::RssSurge => settings.rss_surge_notify,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn settings_defaults_and_ranges_match_the_runtime() {
        use tasty_settings as s;
        use tasty_telemetry as t;
        assert_eq!(
            thresholds(&s::AnomalyDetectionSettings::default()),
            t::AnomalyThresholds::default()
        );
        assert_eq!(
            s::RSS_SURGE_SAMPLES_RANGE,
            (t::RSS_SAMPLES_RANGE.0 as u64, t::RSS_SAMPLES_RANGE.1 as u64)
        );
        assert_eq!(
            s::RSS_SURGE_MIN_MIB_RANGE,
            (
                t::RSS_MIN_BYTES_RANGE.0 / t::MIB,
                t::RSS_MIN_BYTES_RANGE.1 / t::MIB
            )
        );
        assert_eq!(
            s::SLOW_LOOP_WINDOW_SECS_RANGE,
            (
                t::SLOW_WINDOW_MS_RANGE.0 / 1000,
                t::SLOW_WINDOW_MS_RANGE.1 / 1000
            )
        );
        assert_eq!(
            s::SLOW_LOOP_THRESHOLD_RANGE,
            (
                t::SLOW_THRESHOLD_RANGE.0 as u64,
                t::SLOW_THRESHOLD_RANGE.1 as u64
            )
        );
        assert_eq!(
            s::CALL_BURST_WINDOW_SECS_RANGE,
            (
                t::BURST_WINDOW_MS_RANGE.0 / 1000,
                t::BURST_WINDOW_MS_RANGE.1 / 1000
            )
        );
        assert_eq!(
            s::CALL_BURST_THRESHOLD_RANGE,
            (
                t::BURST_THRESHOLD_RANGE.0 as u64,
                t::BURST_THRESHOLD_RANGE.1 as u64
            )
        );
    }
    #[test]
    fn invalid_apply_falls_back_and_kind_notification_gates_are_independent() {
        let bad = AnomalyDetectionSettings {
            rss_surge_samples: 0,
            ..Default::default()
        };
        assert_eq!(thresholds(&bad), AnomalyThresholds::default());
        let mut settings = AnomalyDetectionSettings::default();
        assert!(should_notify(&settings, AnomalyKind::RssSurge));
        assert!(should_notify(&settings, AnomalyKind::CallBurst));
        assert!(!should_notify(&settings, AnomalyKind::SlowLoop));
        settings.rss_surge_notify = false;
        settings.call_burst_notify = false;
        settings.slow_loop_notify = true;
        assert!(!should_notify(&settings, AnomalyKind::RssSurge));
        assert!(!should_notify(&settings, AnomalyKind::CallBurst));
        assert!(should_notify(&settings, AnomalyKind::SlowLoop));
    }
    #[test]
    fn notification_toggle_does_not_replay_an_existing_rss_episode() {
        let core = crate::ipc::handler::cli_entry_tests::test_core();
        let mut settings = Settings::default();
        settings.anomaly_detection.rss_surge_min_mib = 0;
        settings.anomaly_detection.rss_surge_notify = false;
        apply(&core, &settings);
        let observe = |bytes, time| core.record_rss_sample("a", bytes, time);
        for i in 0..4 {
            assert!(observe(100 + i, 1000 + i).is_none());
        }
        let event = observe(104, 1004).unwrap();
        assert!(!should_notify(&settings.anomaly_detection, event.kind));
        settings.anomaly_detection.rss_surge_notify = true;
        apply(&core, &settings);
        assert!(observe(105, 62_000).is_none());
        assert!(observe(100, 62_001).is_none());
        for i in 0..3 {
            assert!(observe(101 + i, 62_002 + i).is_none());
        }
        let event = observe(104, 62_005).unwrap();
        assert!(should_notify(&settings.anomaly_detection, event.kind));
    }
    #[test]
    #[cfg(debug_assertions)]
    fn both_live_intent_variants_reject_invalid_anomaly_settings() {
        use crate::app::command::DomainIntent;
        use crate::app::settings_change::{SettingsChange, SettingsOutcome, SettingsPoll, pair};
        let mut core = crate::ipc::handler::cli_entry_tests::test_core();
        let (_, mut session) = crate::state::tests::test_state();
        let mut engine = session.borrow_mut();
        let mut invalid = engine.runtime.settings.clone();
        invalid.anomaly_detection.slow_loop_threshold = 1;
        assert!(
            core.apply_live(&mut engine, DomainIntent::UpdateSettings(invalid.clone()))
                .is_err()
        );
        let (done, mut wait) = pair(std::sync::Arc::new(|| {}));
        let events = core
            .apply_live(
                &mut engine,
                DomainIntent::ChangeSettings {
                    change: SettingsChange::Replace(Box::new(invalid)),
                    done,
                },
            )
            .unwrap();
        assert!(events.is_empty());
        assert!(matches!(
            wait.poll(),
            SettingsPoll::Ready(SettingsOutcome::Rejected(_))
        ));
        assert_eq!(
            engine
                .runtime
                .settings
                .anomaly_detection
                .slow_loop_threshold,
            20
        );
    }
}
