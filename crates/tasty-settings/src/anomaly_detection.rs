//! 이상 신호의 기준과 알림 출력 설정. runtime과의 숫자 일치는 앱 시험이 확인한다.
use serde::{Deserialize, Serialize};

pub const DEFAULT_RSS_SURGE_SAMPLES: u64 = 5;
pub const DEFAULT_RSS_SURGE_MIN_MIB: u64 = 256;
pub const DEFAULT_SLOW_LOOP_WINDOW_SECS: u64 = 300;
pub const DEFAULT_SLOW_LOOP_THRESHOLD: u64 = 20;
pub const DEFAULT_CALL_BURST_WINDOW_SECS: u64 = 60;
pub const DEFAULT_CALL_BURST_THRESHOLD: u64 = 1000;
pub const RSS_SURGE_SAMPLES_RANGE: (u64, u64) = (3, 20);
pub const RSS_SURGE_MIN_MIB_RANGE: (u64, u64) = (0, 65536);
pub const SLOW_LOOP_WINDOW_SECS_RANGE: (u64, u64) = (30, 3600);
pub const SLOW_LOOP_THRESHOLD_RANGE: (u64, u64) = (2, 10000);
pub const CALL_BURST_WINDOW_SECS_RANGE: (u64, u64) = (1, 600);
pub const CALL_BURST_THRESHOLD_RANGE: (u64, u64) = (10, 100000);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnomalyDetectionSettings {
    pub rss_surge_notify: bool,
    pub rss_surge_samples: u64,
    pub rss_surge_min_mib: u64,
    pub slow_loop_notify: bool,
    pub slow_loop_window_secs: u64,
    pub slow_loop_threshold: u64,
    pub call_burst_notify: bool,
    pub call_burst_window_secs: u64,
    pub call_burst_threshold: u64,
}
impl Default for AnomalyDetectionSettings {
    fn default() -> Self {
        Self {
            rss_surge_notify: true,
            rss_surge_samples: DEFAULT_RSS_SURGE_SAMPLES,
            rss_surge_min_mib: DEFAULT_RSS_SURGE_MIN_MIB,
            slow_loop_notify: false,
            slow_loop_window_secs: DEFAULT_SLOW_LOOP_WINDOW_SECS,
            slow_loop_threshold: DEFAULT_SLOW_LOOP_THRESHOLD,
            call_burst_notify: true,
            call_burst_window_secs: DEFAULT_CALL_BURST_WINDOW_SECS,
            call_burst_threshold: DEFAULT_CALL_BURST_THRESHOLD,
        }
    }
}
impl AnomalyDetectionSettings {
    pub fn validate(&self) -> Result<(), String> {
        for (key, value, (lo, hi)) in [
            (
                "rss_surge_samples",
                self.rss_surge_samples,
                RSS_SURGE_SAMPLES_RANGE,
            ),
            (
                "rss_surge_min_mib",
                self.rss_surge_min_mib,
                RSS_SURGE_MIN_MIB_RANGE,
            ),
            (
                "slow_loop_window_secs",
                self.slow_loop_window_secs,
                SLOW_LOOP_WINDOW_SECS_RANGE,
            ),
            (
                "slow_loop_threshold",
                self.slow_loop_threshold,
                SLOW_LOOP_THRESHOLD_RANGE,
            ),
            (
                "call_burst_window_secs",
                self.call_burst_window_secs,
                CALL_BURST_WINDOW_SECS_RANGE,
            ),
            (
                "call_burst_threshold",
                self.call_burst_threshold,
                CALL_BURST_THRESHOLD_RANGE,
            ),
        ] {
            if !(lo..=hi).contains(&value) {
                return Err(format!(
                    "anomaly_detection.{key} {value} is outside {lo}..={hi}"
                ));
            }
        }
        Ok(())
    }
    pub(crate) fn normalize(&mut self, changed: &mut bool) {
        if let Err(error) = self.validate() {
            tracing::warn!(%error,"anomaly_detection reset to defaults");
            *self = Self::default();
            *changed = true;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_numeric_boundaries_and_missing_defaults_are_checked() {
        let default = AnomalyDetectionSettings::default();
        assert_eq!(default.validate(), Ok(()));
        assert!(!default.slow_loop_notify);
        assert!(default.rss_surge_notify && default.call_burst_notify);
        for (key, (lo, hi)) in [
            ("rss_surge_samples", RSS_SURGE_SAMPLES_RANGE),
            ("rss_surge_min_mib", RSS_SURGE_MIN_MIB_RANGE),
            ("slow_loop_window_secs", SLOW_LOOP_WINDOW_SECS_RANGE),
            ("slow_loop_threshold", SLOW_LOOP_THRESHOLD_RANGE),
            ("call_burst_window_secs", CALL_BURST_WINDOW_SECS_RANGE),
            ("call_burst_threshold", CALL_BURST_THRESHOLD_RANGE),
        ] {
            for (value, valid) in [(lo, true), (hi, true), (hi + 1, false)]
                .into_iter()
                .chain(lo.checked_sub(1).map(|v| (v, false)))
            {
                let settings: AnomalyDetectionSettings =
                    toml::from_str(&format!("{key} = {value}")).unwrap();
                assert_eq!(settings.validate().is_ok(), valid, "{key}={value}");
            }
        }
    }
    #[test]
    fn normalize_resets_whole_invalid_section_and_preserves_valid_values() {
        let mut s = crate::Settings::parse_with_migration(
            "[anomaly_detection]\nslow_loop_threshold=1\nrss_surge_notify=false\n",
        )
        .unwrap();
        assert!(s.normalize().changed);
        assert_eq!(s.anomaly_detection, AnomalyDetectionSettings::default());
        let mut s = crate::Settings::parse_with_migration(
            "[anomaly_detection]\nrss_surge_min_mib=0\nslow_loop_notify=true\n",
        )
        .unwrap();
        s.normalize();
        assert_eq!(s.anomaly_detection.rss_surge_min_mib, 0);
        assert!(s.anomaly_detection.slow_loop_notify);
        let s = crate::Settings::parse_with_migration("").unwrap();
        assert_eq!(s.anomaly_detection, AnomalyDetectionSettings::default());
    }
}
