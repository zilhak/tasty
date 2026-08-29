//! `timer.list` 응답 조립 — 등록된 모든 타이머의 read-only 스냅샷.
//!
//! "이 인스턴스가 idle 인데 왜 계속 깨어나는가" 를 재빌드 없이 실행 중 인스턴스에
//! 물어보기 위한 관측 표면이다. 조회 전용 — 등록/취소/강제발화 경로는 열지 않는다.
//! 외부가 내부 스케줄을 흔들면 회수 지연 상한 같은 계약이 무너진다.
//!
//! 허브가 여러 개라는 사실(본체 + plugin manager)은 응답에서 `hub` 필드로만 드러난다.
//! 대기 계산이 `min_deadline` 으로 하나로 접히는 것과 같은 이유로, 관측도 하나의
//! 목록으로 합쳐야 "무엇이 깨우고 있는가" 에 답할 수 있다
//! (`docs/dev-guide/timer-hub.md`).

use std::time::Duration;
use std::time::Instant;

use tasty_timer::Precision;
use tasty_timer::TimerSnapshot;

use crate::app::App;

/// 본체 허브 항목의 `hub` 라벨.
pub(crate) const HUB_APP: &str = "app";
/// plugin manager 자체 허브 항목의 `hub` 라벨.
pub(crate) const HUB_PLUGIN: &str = "plugin";

/// 허브 소속을 붙인 스냅샷 1건. 키만 표시용 문자열로 옮겨 두어 서로 다른 키 타입의
/// 허브들을 한 목록으로 합칠 수 있다.
pub(crate) struct TimerRow {
    pub(crate) key: String,
    pub(crate) hub: &'static str,
    pub(crate) interval: Option<Duration>,
    pub(crate) next_due: Instant,
    pub(crate) precision: Precision,
    pub(crate) last_fired: Option<Instant>,
}

impl TimerRow {
    /// 이 타이머가 이벤트 루프를 깨우기를 요구하는 시각.
    /// `TimerHub::next_deadline` 이 min 을 취하는 값과 같은 정의여야 한다 —
    /// 어긋나면 요약 라인이 실제 wakeup 원인과 다른 항목을 지목한다.
    fn hard_deadline(&self) -> Instant {
        match self.precision {
            Precision::Strict => self.next_due,
            Precision::Lax { slack } => self.next_due + slack,
        }
    }
}

/// 스냅샷을 허브 라벨과 함께 행으로 옮긴다. 키 타입마다 표시 방법이 달라
/// (본체는 `Debug`, plugin 은 이미 문자열) 라벨링은 호출자가 넘긴다.
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

/// `Instant` 는 프로세스 밖에서 의미가 없으므로 호출 시각 기준 상대 밀리초로 낸다.
/// 이미 지난 시각은 음수가 되어 "밀려 있다" 는 사실이 그대로 보인다 — 여기서
/// 0 으로 접으면 stale 데드라인(과거 시각 재등록으로 인한 스핀)이 관측에서 사라진다.
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
        // 양수 = "이만큼 전에 발화했다". 한 번도 발화하지 않았으면 null.
        "last_fired_ms_ago": row.last_fired.map(|t| rel_ms(now, t)),
    })
}

/// 행 목록을 `timer.list` 응답으로 직렬화한다.
///
/// `hard_deadline` 이 이 응답의 요점이다 — 지금 무엇이 인스턴스를 깨우고 있는지에
/// 직접 답한다. 등록된 타이머가 없으면 `null` 이고, 그건 "무기한 자도 된다" 를 뜻한다.
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
    /// 본체 허브 + plugin manager 허브를 합친 `timer.list` 응답.
    ///
    /// gui 는 `app_methods` step 에서, headless 는 dispatch pump 에서 호출한다 —
    /// 허브가 `App` 필드라 `CoreState` 만 받는 일반 IPC 핸들러에서는 닿지 않는다.
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
        // Lax 가 더 이르게 due 해도 slack 만큼은 깨움을 요구하지 않는다 —
        // 요약 라인이 Strict 쪽을 지목해야 한다.
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
