//! Explicit fixture measurements use existing directories and never create workload files.
use super::*;
use std::time::Instant;

#[test]
#[ignore = "explicit serial release measurement with EXPLORER_PERF_FIXTURES"]
fn existing_directory_read_samples() {
    let fixtures = std::env::var("EXPLORER_PERF_FIXTURES").expect("fixture manifest path");
    let fixtures: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixtures).unwrap()).unwrap();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_test_writer()
        .finish();
    let _log = tracing::subscriber::set_default(subscriber);
    let mut cases = 0;
    for fixture in fixtures.as_array().unwrap() {
        let count = fixture["count"].as_u64().unwrap() as usize;
        if count == 500 {
            continue;
        }
        let path = Path::new(fixture["path"].as_str().unwrap());
        let started = Instant::now();
        let mut entries = read_dir_entries(path).unwrap();
        let read_ms = started.elapsed().as_secs_f64() * 1000.0;
        assert_eq!(entries.len(), count);
        let started = Instant::now();
        sort_entries(&mut entries, SortColumn::Name, SortDir::Asc);
        let sort_ms = started.elapsed().as_secs_f64() * 1000.0;
        tracing::info!(
            "EXPLORER_DIRECTORY {}",
            serde_json::json!({
                "medium":fixture["medium"],"count":count,"read_ms":read_ms,"sort_ms":sort_ms,
                "cache":"existing fixture; cache not dropped"
            })
        );
        cases += 1;
    }
    assert_eq!(cases, 6, "SSD and NFS at three sizes");
}
