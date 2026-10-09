use super::*;

fn collect(root: &Path, query: &str, stop: bool) -> Vec<SearchEvent> {
    let (sender, receiver) = mpsc::channel();
    let spec = SearchSpec {
        root: root.into(),
        query: query.into(),
        stop: Arc::new(AtomicBool::new(stop)),
    };
    run(spec, sender, &|| {});
    receiver.try_iter().collect()
}

fn hits(events: &[SearchEvent]) -> Vec<String> {
    let mut names: Vec<String> = events
        .iter()
        .flat_map(|e| match e {
            SearchEvent::Hits(h) => h.iter().map(|e| e.name.clone()).collect(),
            _ => Vec::new(),
        })
        .collect();
    names.sort();
    names
}

#[test]
fn subfolders_are_searched_case_insensitively_and_the_end_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let deep = dir.path().join("Documents").join("finance");
    std::fs::create_dir_all(&deep).unwrap();
    std::fs::write(dir.path().join("Report.pdf"), b"").unwrap();
    std::fs::write(deep.join("q3-report.xlsx"), b"").unwrap();
    std::fs::write(deep.join("notes.md"), b"").unwrap();
    let events = collect(dir.path(), "report", false);
    assert_eq!(hits(&events), ["Report.pdf", "q3-report.xlsx"]);
    assert!(matches!(
        events.last(),
        Some(SearchEvent::Done { stopped: false })
    ));
    assert_eq!(
        relative_folder(dir.path(), &deep.join("q3-report.xlsx")),
        Path::new("Documents").join("finance").display().to_string()
    );
    assert_eq!(
        relative_folder(dir.path(), &dir.path().join("Report.pdf")),
        "."
    );
}

#[test]
fn a_stopped_search_reads_nothing_more_and_an_unreadable_start_fails() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("report"), b"").unwrap();
    let events = collect(dir.path(), "report", true);
    assert!(hits(&events).is_empty());
    assert!(matches!(
        events.last(),
        Some(SearchEvent::Done { stopped: true })
    ));
    let events = collect(&dir.path().join("missing"), "report", false);
    assert!(matches!(events.as_slice(), [SearchEvent::Failed(_)]));
}

#[cfg(unix)]
#[test]
fn unreadable_subfolders_are_skipped_and_linked_folders_are_not_entered() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let private = dir.path().join("private");
    std::fs::create_dir(&private).unwrap();
    std::fs::write(private.join("report"), b"").unwrap();
    std::fs::write(dir.path().join("report.txt"), b"").unwrap();
    std::os::unix::fs::symlink(dir.path(), dir.path().join("loop")).unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o000)).unwrap();
    // root 는 권한과 관계없이 읽으므로 그때는 건너뛴 폴더가 생기지 않는다.
    let readable_anyway = std::fs::read_dir(&private).is_ok();
    let events = collect(dir.path(), "report", false);
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o755)).unwrap();
    let skipped: Vec<&PathBuf> = events
        .iter()
        .filter_map(|e| match e {
            SearchEvent::Skipped(p) => Some(p),
            _ => None,
        })
        .collect();
    if !readable_anyway {
        assert_eq!(skipped, [&private]);
        // 링크 "loop" 를 따라 들어갔다면 report.txt 가 두 번 나온다.
        assert_eq!(hits(&events), ["report.txt"]);
    }
    assert!(matches!(
        events.last(),
        Some(SearchEvent::Done { stopped: false })
    ));
}
