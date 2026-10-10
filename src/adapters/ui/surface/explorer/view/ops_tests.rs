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
        leftovers: Vec::new(),
    }
}

/// 원본이 남은 이동 항목. 카드 시험은 파일 상태를 보지 않으므로 없는 경로로 만든다.
fn leftover(path: &str) -> Leftover {
    Leftover::before_remove(Path::new(path), Path::new("/tmp/Documents/copy"))
}

fn labels(text: &CardText) -> Vec<String> {
    card_actions(text).into_iter().map(|(l, _, _)| l).collect()
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
    r.leftovers = vec![leftover("/b")];
    r.skipped = vec![PathBuf::from("/c")];
    assert!(!result_is_timed(&r));
    let text = card_text(&card(r));
    assert_eq!(text.kind, ToastKind::Warning);
    assert_eq!(
        text.title,
        t_args("explorer.result.partial_move", &["2", "4", "1"])
    );
    assert_eq!(text.retry, [PathBuf::from("/a"), PathBuf::from("/c")]);
    // 원본이 남은 /b 는 다시 옮기지 않고 원본 삭제만 다시 하며, Retry 의 수에 함께 센다.
    assert_eq!(text.leftovers, [leftover("/b")]);
    assert_eq!(labels(&text)[0], t_fmt("explorer.result.retry", "3"));
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
    assert_eq!(
        text.title,
        t_count("explorer.result.undo_partial_move", 1, &["1"])
    );
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
fn a_move_that_left_originals_stays_without_undo_and_retries_the_delete() {
    let mut r = report(OpKind::Move, 2, 2);
    r.undo.truncate(1);
    r.failed = vec![failure(
        "/a",
        Reason::SourceNotRemoved("Permission denied".into()),
    )];
    r.leftovers = vec![leftover("/a")];
    assert!(
        !result_is_timed(&r),
        "a left original must not vanish on a timer"
    );
    let text = card_text(&card(r));
    assert_eq!(text.kind, ToastKind::Warning);
    assert_eq!(
        text.title,
        t_count("explorer.result.source_left_move", 1, &["2", "2", "1"])
    );
    assert!(!text.undo);
    assert!(text.retry.is_empty(), "the move itself is not sent again");
    assert_eq!(text.leftovers, [leftover("/a")]);
    assert_eq!(
        labels(&text),
        [
            t_fmt("explorer.result.retry", "1"),
            t("explorer.result.copy_paths").to_owned()
        ]
    );
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
    assert_eq!(
        text.title,
        t_count("explorer.result.undo_partial_copy", 1, &["1"])
    );
    assert_eq!(text.lines[0].1, t("explorer.result.changed_kept"));
}

/// 다시 지울 때 사본이 없어 원본을 남긴 항목은 그 사유를 보인다.
#[test]
fn a_retried_delete_names_an_original_kept_because_the_copy_is_gone() {
    let mut r = report(OpKind::Move, 1, 0);
    r.undo.clear();
    r.failed = vec![failure("/a", Reason::CopyMissing)];
    let text = card_text(&card(r));
    assert_eq!(text.lines[0].1, t("explorer.result.copy_missing"));
    // 남긴 원본은 다시 지우지 않는다. Retry 는 이동 자체를 다시 요청한다.
    assert!(text.leftovers.is_empty());
    assert_eq!(text.retry, [PathBuf::from("/a")]);
}

/// 폴더 원본에서 사본에 없는 항목을 남겼으면 원본이 남은 경고로 보이고, 원본 삭제만 다시 할 수 있다.
#[test]
fn a_folder_original_with_items_kept_stays_a_source_left_card() {
    let mut r = report(OpKind::Move, 1, 1);
    r.undo.clear();
    r.failed = vec![failure("/a", Reason::KeptNotInCopy(2))];
    r.leftovers = vec![leftover("/a")];
    assert!(!result_is_timed(&r));
    let text = card_text(&card(r));
    assert_eq!(text.kind, ToastKind::Warning);
    assert_eq!(
        text.title,
        t_count("explorer.result.source_left_move", 1, &["1", "1", "1"])
    );
    assert_eq!(
        text.lines[0].1,
        t_fmt("explorer.result.kept_not_in_copy", "2")
    );
    assert!(text.retry.is_empty(), "the folder is not moved again");
    assert_eq!(labels(&text)[0], t_fmt("explorer.result.retry", "1"));
    assert!(!text.undo);
}

/// 원본 삭제 다시 하기를 취소하면 남은 원본 카드로 돌아가 Retry 가 다시 있다.
#[test]
fn a_cancelled_delete_retry_returns_to_the_source_left_card() {
    let mut r = report(OpKind::Move, 3, 3);
    r.undo.clear();
    r.failed = vec![
        failure("/a", Reason::RemoveCancelled),
        failure("/b", Reason::RemoveCancelled),
    ];
    r.leftovers = vec![leftover("/a"), leftover("/b")];
    assert!(!result_is_timed(&r));
    let text = card_text(&card(r));
    assert_eq!(text.kind, ToastKind::Warning);
    assert_eq!(
        text.title,
        t_count("explorer.result.source_left_move", 2, &["3", "3", "2"])
    );
    assert_eq!(text.lines[0].1, t("explorer.result.remove_cancelled"));
    assert_eq!(labels(&text)[0], t_fmt("explorer.result.retry", "2"));
    assert!(!text.undo);
}

/// 사본이 원본과 같은 파일이라 남긴 원본도 원본이 남은 경고 카드에 사유와 Retry 로 보인다.
#[test]
fn a_source_kept_as_the_same_file_shows_why_and_retry() {
    let mut r = report(OpKind::Move, 1, 1);
    r.undo.clear();
    r.failed = vec![failure("/a", Reason::SameAsCopy)];
    r.leftovers = vec![leftover("/a")];
    let text = card_text(&card(r));
    assert_eq!(text.kind, ToastKind::Warning);
    assert_eq!(text.lines[0].1, t("explorer.result.same_as_copy"));
    assert_eq!(labels(&text)[0], t_fmt("explorer.result.retry", "1"));
}
