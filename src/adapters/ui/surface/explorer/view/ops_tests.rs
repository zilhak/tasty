//! 파일 작업 표시의 결과 문구·수명 시험.

use super::*;
use crate::app::explorer_files::job::Failure;
use std::time::Duration;

fn report(kind: OpKind, total: usize, done: usize) -> Report {
    Report {
        kind,
        dest: Some(PathBuf::from("/tmp/Documents")),
        total,
        done,
        undo: (0..done)
            .map(|i| UndoStep::Created(PathBuf::from(format!("/tmp/Documents/{i}")), None))
            .collect(),
        failed: Vec::new(),
        skipped: Vec::new(),
        cancelled: false,
        trash_unavailable: false,
    }
}

fn card(report: Report) -> ResultCard {
    ResultCard {
        id: 1,
        report,
        undo_of: None,
        expires: None,
    }
}

fn failure(path: &str, reason: Reason) -> Failure {
    Failure {
        path: PathBuf::from(path),
        reason,
    }
}

#[test]
fn a_finished_copy_offers_undo_and_goes_away_on_time() {
    let r = report(OpKind::Copy, 2, 2);
    assert!(result_is_timed(&r));
    let text = card_text(&card(r));
    assert_eq!(text.kind, ToastKind::Success);
    assert_eq!(
        text.title,
        t_fmt2("explorer.result.copied", "2", "Documents")
    );
    assert!(text.undo);
    assert!(text.retry.is_empty());
    assert!(text.lines.is_empty());
}

#[test]
fn a_partial_move_stays_and_retries_only_what_can_run_again() {
    let mut r = report(OpKind::Move, 4, 2);
    r.failed = vec![
        failure("/a", Reason::Os("Permission denied".into())),
        failure("/b", Reason::SourceNotRemoved("busy".into())),
    ];
    r.skipped = vec![PathBuf::from("/c")];
    assert!(!result_is_timed(&r));
    let text = card_text(&card(r));
    assert_eq!(text.kind, ToastKind::Warning);
    assert_eq!(
        text.title,
        t_args("explorer.result.partial_move", &["2", "4", "1"])
    );
    assert_eq!(text.retry, [PathBuf::from("/a"), PathBuf::from("/c")]);
    assert!(!text.undo, "undo is only offered when everything finished");
    assert_eq!(text.lines.len(), 3);
    assert!(text.more.is_none());
}

#[test]
fn more_than_three_problems_are_summarised() {
    let mut r = report(OpKind::Copy, 5, 0);
    r.failed = (0..5)
        .map(|i| failure(&format!("/f{i}"), Reason::Os("No space left".into())))
        .collect();
    let text = card_text(&card(r));
    assert_eq!(text.kind, ToastKind::Error);
    assert_eq!(text.title, t_fmt("explorer.result.failed_copy", "5"));
    assert_eq!(text.lines.len(), MAX_RESULT_LINES);
    assert_eq!(text.more, Some(t_fmt("explorer.result.more", "2")));
}

#[test]
fn trash_that_is_not_available_says_nothing_was_deleted() {
    let mut r = report(OpKind::Trash, 1, 0);
    r.dest = None;
    r.failed = vec![failure("/x", Reason::Os("no trash".into()))];
    r.trash_unavailable = true;
    assert!(!result_is_timed(&r));
    let text = card_text(&card(r));
    assert_eq!(text.kind, ToastKind::Error);
    assert_eq!(text.title, t("explorer.trash.unavailable"));
}

#[test]
fn a_cancelled_job_neither_retries_nor_undoes() {
    let mut r = report(OpKind::Copy, 10, 3);
    r.cancelled = true;
    assert!(result_is_timed(&r));
    let text = card_text(&card(r));
    assert_eq!(text.kind, ToastKind::Info);
    assert_eq!(
        text.title,
        t_fmt2("explorer.result.cancelled_copy", "3", "10")
    );
    assert!(!text.undo);
    assert!(text.retry.is_empty());
}

#[test]
fn an_undo_result_lists_what_could_not_be_put_back() {
    let mut r = report(OpKind::Undo, 2, 1);
    r.undo.clear();
    r.failed = vec![failure("/a", Reason::NewerThere)];
    let mut c = card(r);
    c.undo_of = Some(OpKind::Move);
    let text = card_text(&c);
    assert_eq!(text.kind, ToastKind::Warning);
    assert_eq!(text.title, t_fmt("explorer.result.undo_partial_move", "1"));
    assert_eq!(text.lines[0].1, t("explorer.result.newer_there"));
    assert!(!text.undo);
    assert!(text.retry.is_empty());
}

#[test]
fn timed_cards_expire_and_others_stay() {
    let mut ops = OpsState::default();
    let now = Instant::now();
    ops.push_result(report(OpKind::Copy, 1, 1), None, Some(now));
    ops.push_result(report(OpKind::Copy, 1, 0), None, None);
    assert_eq!(ops.next_expiry(), Some(now));
    assert!(ops.expire(now + Duration::from_millis(1)));
    assert_eq!(ops.result_count(), 1);
    assert_eq!(ops.next_expiry(), None);
}

#[test]
fn a_move_that_left_originals_stays_without_undo() {
    let mut r = report(OpKind::Move, 2, 2);
    r.undo.truncate(1);
    r.failed = vec![failure(
        "/a",
        Reason::SourceNotRemoved("Permission denied".into()),
    )];
    assert!(
        !result_is_timed(&r),
        "a left original must not vanish on a timer"
    );
    let text = card_text(&card(r));
    assert_eq!(text.kind, ToastKind::Warning);
    assert_eq!(
        text.title,
        t_args("explorer.result.source_left_move", &["2", "2", "1"])
    );
    assert!(!text.undo);
    assert!(text.retry.is_empty());
    assert_eq!(
        text.lines[0].1,
        t_fmt("explorer.result.source_not_removed", "Permission denied")
    );
}

#[test]
fn an_undo_result_names_a_copy_kept_because_it_changed() {
    let mut r = report(OpKind::Undo, 1, 0);
    r.undo.clear();
    r.failed = vec![failure("/a", Reason::ChangedSince)];
    let mut c = card(r);
    c.undo_of = Some(OpKind::Copy);
    let text = card_text(&c);
    assert_eq!(text.title, t_fmt("explorer.result.undo_partial_copy", "1"));
    assert_eq!(text.lines[0].1, t("explorer.result.changed_kept"));
}
