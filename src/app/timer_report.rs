//! timer.list로 본체·플러그인 허브의 등록 상태를 조회한다. 타이머를 변경하지 않는다.
//! 예약한 깨우기 목표 시각이며 실제 깨움 원인이나 실행 완료 시각은 아니다.

use std::time::Duration;
use std::time::Instant;

use tasty_timer::Precision;
use tasty_timer::TimerSnapshot;

use crate::app::App;

pub(crate) const HUB_APP: &str = "app";
pub(crate) const HUB_PLUGIN: &str = "plugin";

/// 서로 다른 허브의 키를 문자열로 바꾸고 소속을 붙인 조회 행.
pub(crate) struct TimerRow {
    pub(crate) key: String,
    pub(crate) hub: &'static str,
    pub(crate) interval: Option<Duration>,
    pub(crate) next_due: Instant,
    pub(crate) precision: Precision,
    pub(crate) last_fired: Option<Instant>,
}

impl TimerRow {
    /// TimerHub::next_deadline과 같은 계산식. 실제 작업 시작·완료 시각을 보장하지 않는다.
    fn hard_deadline(&self) -> Instant {
        match self.precision {
            Precision::Strict => self.next_due,
            Precision::Lax { slack } => self.next_due + slack,
        }
    }
}

pub(crate) fn rows_from<K>(
    snapshot: &[TimerSnapshot<K>],
    hub: &'static str,
    label: impl Fn(&K) -> String,
) -> Vec<TimerRow> {
    snapshot
        .iter()
        .map(|s| TimerRow {
            key: label(&s.key),
            hub,
            interval: s.interval,
            next_due: s.next_due,
            precision: s.precision,
            last_fired: s.last_fired,
        })
        .collect()
}

/// 프로세스 밖에서 해석할 수 있도록 현재 시각 기준 밀리초로 바꾸며 지난 시각은 음수로 남긴다.
fn rel_ms(at: Instant, now: Instant) -> i64 {
    if at >= now {
        i64::try_from(at.duration_since(now).as_millis()).unwrap_or(i64::MAX)
    } else {
        i64::try_from(now.duration_since(at).as_millis())
            .map(|v| -v)
            .unwrap_or(i64::MIN)
    }
}

fn duration_ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

fn row_json(row: &TimerRow, now: Instant) -> serde_json::Value {
    let (precision, slack_ms) = match row.precision {
        Precision::Strict => ("strict", None),
        Precision::Lax { slack } => ("lax", Some(duration_ms(slack))),
    };
    serde_json::json!({
        "key": row.key,
        "hub": row.hub,
        "interval_ms": row.interval.map(duration_ms),
        "next_due_ms": rel_ms(row.next_due, now),
        "precision": precision,
        "slack_ms": slack_ms,
        "hard_deadline_ms": rel_ms(row.hard_deadline(), now),
        // 마지막 drain_due가 이 키를 반환한 이후의 시간. 실행 완료 시각은 아니다.
        "last_fired_ms_ago": row.last_fired.map(|t| rel_ms(now, t)),
    })
}

/// 가장 이른 깨우기 목표도 함께 반환한다. None은 등록된 타이머가 없다는 뜻일 뿐,
/// IPC·입력 등 다른 이유로 이벤트 루프가 깨어날 가능성을 배제하지 않는다.
pub(crate) fn to_json(rows: &[TimerRow], now: Instant) -> serde_json::Value {
    let hard = rows.iter().min_by_key(|r| r.hard_deadline());
    serde_json::json!({
        "timers": rows.iter().map(|r| row_json(r, now)).collect::<Vec<_>>(),
        "hard_deadline": hard.map(|r| serde_json::json!({
            "key": r.key,
            "hub": r.hub,
            "in_ms": rel_ms(r.hard_deadline(), now),
        })),
    })
}

impl App {
    /// App이 가진 두 허브를 합친다. CoreState만 받는 IPC 핸들러에서는 직접 조회할 수 없다.
    pub(crate) fn timer_list_json(&self, now: Instant) -> serde_json::Value {
        let app_snapshot = self.timers.snapshot();
        let mut rows = rows_from(&app_snapshot, HUB_APP, |k| format!("{k:?}"));
        if let Some(manager) = self.plugin_manager.as_ref() {
            let plugin_snapshot = manager.timer_snapshot();
            rows.extend(rows_from(&plugin_snapshot, HUB_PLUGIN, |k| {
                (*k).to_string()
            }));
        }
        to_json(&rows, now)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn row(key: &str, next_due: Instant, precision: Precision) -> TimerRow {
        TimerRow {
            key: key.to_string(),
            hub: HUB_APP,
            interval: Some(ms(1000)),
            next_due,
            precision,
            last_fired: None,
        }
    }

    #[test]
    fn lax_timers_do_not_claim_the_hard_deadline_before_their_slack() {
        let now = Instant::now();
        // 더 이른 next_due보다 slack을 더한 깨우기 목표가 요약 선택을 결정한다.
        let rows = vec![
            row("Lax", now + ms(100), Precision::Lax { slack: ms(60_000) }),
            row("Strict", now + ms(500), Precision::Strict),
        ];
        let json = to_json(&rows, now);
        assert_eq!(json["hard_deadline"]["key"], "Strict");
        assert_eq!(json["hard_deadline"]["in_ms"], 500);
    }

    #[test]
    fn an_overdue_deadline_is_reported_as_negative_not_clamped_to_zero() {
        let now = Instant::now();
        let rows = vec![row("Stale", now - ms(2_000), Precision::Strict)];
        let json = to_json(&rows, now);
        assert_eq!(json["timers"][0]["next_due_ms"], -2_000);
        assert_eq!(json["hard_deadline"]["in_ms"], -2_000);
    }

    #[test]
    fn an_empty_hub_reports_no_hard_deadline() {
        let json = to_json(&[], Instant::now());
        assert!(json["hard_deadline"].is_null());
        assert_eq!(json["timers"].as_array().map(Vec::len), Some(0));
    }

    #[test]
    fn rows_carry_the_hub_label_so_merged_lists_stay_attributable() {
        let now = Instant::now();
        let snapshot = [TimerSnapshot {
            key: "PluginPing",
            interval: Some(ms(15_000)),
            next_due: now + ms(7_000),
            precision: Precision::Strict,
            last_fired: Some(now - ms(8_000)),
        }];
        let rows = rows_from(&snapshot, HUB_PLUGIN, |k| (*k).to_string());
        let json = to_json(&rows, now);
        assert_eq!(json["timers"][0]["key"], "PluginPing");
        assert_eq!(json["timers"][0]["hub"], "plugin");
        assert_eq!(json["timers"][0]["interval_ms"], 15_000);
        assert_eq!(json["timers"][0]["last_fired_ms_ago"], 8_000);
    }

    #[test]
    fn a_one_shot_timer_reports_a_null_interval() {
        let now = Instant::now();
        let rows = vec![TimerRow {
            key: "NativeMenu".to_string(),
            hub: HUB_APP,
            interval: None,
            next_due: now + ms(8),
            precision: Precision::Strict,
            last_fired: None,
        }];
        let json = to_json(&rows, now);
        assert!(json["timers"][0]["interval_ms"].is_null());
        assert!(json["timers"][0]["last_fired_ms_ago"].is_null());
    }

    #[test]
    fn lax_slack_is_exposed_so_the_promotion_point_is_visible() {
        let now = Instant::now();
        let rows = vec![row(
            "LayoutFlush",
            now + ms(500),
            Precision::Lax { slack: ms(500) },
        )];
        let json = to_json(&rows, now);
        assert_eq!(json["timers"][0]["precision"], "lax");
        assert_eq!(json["timers"][0]["slack_ms"], 500);
        assert_eq!(json["timers"][0]["hard_deadline_ms"], 1_000);
    }
}
