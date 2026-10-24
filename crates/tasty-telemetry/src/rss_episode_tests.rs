use crate::AnomalyDetector;

#[test]
fn a_continuing_rss_surge_fires_once_not_every_cooldown() {
    let d = small_detector();
    let fired = (0..20u64)
        .filter_map(|i| d.record_rss_sample("plugin_a", 100 + i, 1_000 + i * 30_000, i))
        .count();
    assert_eq!(fired, 1);
}

#[test]
fn plateau_or_decrease_rearms_a_new_trend_after_cooldown() {
    for break_sample in [104, 99] {
        let d = small_detector();
        let mut fired = 0;
        for (i, rss) in [100, 101, 102, 103, 104, break_sample, 105, 106, 107, 108]
            .into_iter()
            .enumerate()
        {
            fired += usize::from(
                d.record_rss_sample("plugin_a", rss, 1000 + i as u64 * 30_000, i as u64)
                    .is_some(),
            );
        }
        assert_eq!(fired, 2);
    }
}

#[test]
fn restarting_growth_inside_cooldown_still_waits_for_cooldown() {
    let d = small_detector();
    let mut fired = 0;
    for (i, rss) in [100, 101, 102, 103, 104, 100, 101, 102, 103, 104]
        .into_iter()
        .enumerate()
    {
        fired += usize::from(
            d.record_rss_sample("plugin_a", rss, 1000 + i as u64, i as u64)
                .is_some(),
        );
    }
    assert_eq!(fired, 1);
    assert!(d.record_rss_sample("plugin_a", 105, 62_000, 20).is_some());
}

fn small_detector() -> AnomalyDetector {
    let d = AnomalyDetector::new();
    d.set_thresholds(crate::AnomalyThresholds {
        rss_surge_min_bytes: 0,
        ..Default::default()
    })
    .unwrap();
    d
}
