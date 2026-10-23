//! Large fixtures exercise the same renderer and selection logic as the smaller regression suite.
use super::*;
use std::time::Instant;

const LARGE: usize = 100_000;

#[test]
fn a_hundred_thousand_entries_keep_selection_find_and_typeahead_semantics() {
    for mode in MODES {
        let mut h = Harness::with_count(mode, LARGE);
        h.focused = true;
        assert!(h.frame(Vec::new()).len() < 200);
        h.view.select_all();
        assert_eq!(h.view.selected.len(), LARGE);
        assert_eq!(h.view.anchor.as_deref(), Some(path(LARGE - 1).as_path()));
        h.view.select_only(&path(LARGE - 1));
        h.frame(vec![Event::Text("i".into())]);
        assert_eq!(h.view.selected, HashSet::from([path(0)]));
        h.view.open_find();
        h.view.find.as_mut().unwrap().set_query("item9");
        h.frame(Vec::new());
        assert_eq!(h.view.shown_count(), 10_000);
        h.view.select_all();
        assert_eq!(h.view.selected.len(), 10_000);
        assert!(h.view.selected.contains(&path(90_000)));
        assert!(h.view.selected.contains(&path(99_999)));
        h.view.find.as_mut().unwrap().set_query("not-present");
        h.frame(Vec::new());
        h.view.select_all();
        assert!(h.view.selected.is_empty());
    }
}

/// CPU renderer samples only: this deliberately does not claim GPU/present or OS input latency.
#[test]
#[ignore = "explicit release measurement; serial, same fixture before and after changes"]
fn large_directory_cpu_samples() {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .finish();
    let _log = tracing::subscriber::set_default(subscriber);
    for count in [1_000, 10_000, LARGE] {
        for mode in MODES {
            let mut h = Harness::with_count(mode, count);
            h.cell = Rect::from_min_max(Pos2::ZERO, Pos2::new(1600.0, 1000.0));
            h.focused = true;
            let first = Instant::now();
            h.frame(Vec::new());
            let first_ms = first.elapsed().as_secs_f64() * 1000.0;
            for _ in 0..5 {
                h.frame(Vec::new());
            }
            let cell = h.cell;
            let first_visible = |drawn: Vec<(String, Rect)>| {
                drawn
                    .into_iter()
                    .find(|(_, rect)| cell.contains(rect.center()))
                    .map(|(name, _)| name)
            };
            let before = first_visible(h.frame(Vec::new()));
            let mut samples = Vec::new();
            for _ in 0..90 {
                let start = Instant::now();
                h.frame(wheel(Pos2::new(1000.0, 700.0), 240.0));
                samples.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            let after = h
                .frame(Vec::new())
                .into_iter()
                .find(|(_, rect)| h.cell.contains(rect.center()))
                .map(|(name, _)| name);
            assert_ne!(
                before, after,
                "{count} {mode:?}: wheel must actually scroll"
            );
            let start = Instant::now();
            h.view.select_all();
            h.frame(Vec::new());
            let all_ms = start.elapsed().as_secs_f64() * 1000.0;
            h.view.clear_selection();
            let start = Instant::now();
            h.frame(vec![Event::Text("z".into())]);
            let typeahead_ms = start.elapsed().as_secs_f64() * 1000.0;
            h.view.open_find();
            h.view.find.as_mut().unwrap().set_query("item9");
            let start = Instant::now();
            h.frame(Vec::new());
            let find_ms = start.elapsed().as_secs_f64() * 1000.0;
            tracing::info!(
                "EXPLORER_CPU {}",
                serde_json::json!({
                    "count":count, "mode":format!("{mode:?}"), "scroll_ms":samples, "first_cpu_frame_ms":first_ms,
                    "select_all_ms":all_ms, "typeahead_ms":typeahead_ms, "find_ms":find_ms
                })
            );
        }
    }
}

/// Real View sync and egui rendering, without an OS event loop or network transport.
#[test]
#[ignore = "explicit serial release measurement of first lookup, tabs, and remote Loaded sync"]
fn index_lifecycle_cpu_samples() {
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .finish();
    let _log = tracing::subscriber::set_default(subscriber);
    for mode in MODES {
        let mut h = Harness::with_count(mode, LARGE);
        h.cell = Rect::from_min_max(Pos2::ZERO, Pos2::new(1600.0, 1000.0));
        h.focused = true;
        for _ in 0..5 {
            h.frame(Vec::new());
        }
        let started = Instant::now();
        h.frame(vec![Event::Text("i".into())]);
        let first_input_frame_ms = started.elapsed().as_secs_f64() * 1000.0;
        let entries = h.view.entries.clone();
        h.panel.add_tab();
        h.view.sync(&h.panel, None);
        h.view.set_entries(entries.clone());
        h.frame(Vec::new());
        let started = Instant::now();
        h.panel.active = 0;
        h.view.sync(&h.panel, None);
        h.view.reveal_created(&path(LARGE - 1));
        h.frame(Vec::new());
        let restored_tab_frame_ms = started.elapsed().as_secs_f64() * 1000.0;
        h.view = super::super::ExplorerView::new();
        h.view.sync(&h.panel, Some(42));
        let request = h.view.outbox[0].request_id;
        let started = Instant::now();
        assert!(
            h.view
                .apply_remote_list_dir_result(request, &h.panel, Ok(entries))
        );
        let remote_apply_ms = started.elapsed().as_secs_f64() * 1000.0;
        for _ in 0..5 {
            h.frame(Vec::new());
        }
        let started = Instant::now();
        h.frame(vec![Event::Text("i".into())]);
        let remote_first_input_frame_ms = started.elapsed().as_secs_f64() * 1000.0;
        let generation = h.view.entries_gen;
        let mut remote_frames_ms = Vec::new();
        for _ in 0..90 {
            let started = Instant::now();
            h.view.sync(&h.panel, Some(42));
            h.frame(Vec::new());
            remote_frames_ms.push(started.elapsed().as_secs_f64() * 1000.0);
        }
        assert_eq!(
            h.view.entries_gen, generation,
            "Loaded sync keeps its generation"
        );
        tracing::info!(
            "EXPLORER_INDEX_LIFECYCLE {}",
            serde_json::json!({
                "mode":format!("{mode:?}"),"count":LARGE,"first_input_frame_ms":first_input_frame_ms,
                "restored_tab_frame_ms":restored_tab_frame_ms,"remote_apply_ms":remote_apply_ms,
                "remote_first_input_frame_ms":remote_first_input_frame_ms,"remote_sync_and_frame_ms":remote_frames_ms
            })
        );
    }
}
