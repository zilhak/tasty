//! 사본 metadata 시험. 휴지통은 사용자 홈의 실제 휴지통이라 여기서 부르지 않는다.

use super::super::*;

const OLD: Duration = Duration::from_secs(1_000_000_000);

fn set_modified(path: &Path, at: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(path)
        .and_then(|f| f.set_modified(at))
        .expect("set a test modified time");
}

fn modified(path: &Path) -> SystemTime {
    path.symlink_metadata()
        .and_then(|m| m.modified())
        .expect("read a modified time")
}

#[test]
fn a_copied_file_keeps_its_modified_time() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(src.join("inner")).expect("mkdir");
    std::fs::create_dir_all(&dst).expect("mkdir");
    std::fs::write(src.join("a.txt"), "a").expect("write");
    std::fs::write(src.join("inner/b.txt"), "b").expect("write");
    let old = SystemTime::UNIX_EPOCH + OLD;
    set_modified(&src.join("a.txt"), old);
    set_modified(&src.join("inner/b.txt"), old);

    let report = run_transfer(
        &Shared::fixed(Choice::KeepBoth),
        &[src.join("a.txt"), src.join("inner")],
        &dst,
        false,
    );

    assert_eq!(report.done, 2);
    assert_eq!(modified(&dst.join("a.txt")), old);
    assert_eq!(modified(&dst.join("inner/b.txt")), old);
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    path.symlink_metadata()
        .expect("metadata")
        .permissions()
        .mode()
        & 0o7777
}

#[cfg(unix)]
fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
}

/// 읽기 전용 폴더도 안쪽까지 복사해 공개한 뒤 같은 권한을 갖는다.
#[cfg(unix)]
#[test]
fn copied_folders_keep_their_permissions_even_when_read_only() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("tree");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(src.join("shared")).expect("mkdir");
    std::fs::create_dir_all(src.join("locked")).expect("mkdir");
    std::fs::create_dir_all(&dst).expect("mkdir");
    std::fs::write(src.join("locked/x.txt"), "x").expect("write");
    chmod(&src.join("shared"), 0o750);
    chmod(&src.join("locked"), 0o555);
    chmod(&src, 0o555);

    let report = run_transfer(
        &Shared::fixed(Choice::KeepBoth),
        std::slice::from_ref(&src),
        &dst,
        false,
    );

    let copy = dst.join("tree");
    let modes = [
        mode(&copy),
        mode(&copy.join("shared")),
        mode(&copy.join("locked")),
    ];
    for path in [&src, &copy, &copy.join("locked")] {
        chmod(path, 0o755);
    }
    assert_eq!(report.done, 1, "failed: {:?}", report.failed);
    assert_eq!(modes, [0o555, 0o750, 0o555]);
    assert_eq!(read_text(&copy.join("locked/x.txt")), "x");
    assert_eq!(names(&dst), ["tree"]);
}

/// 복사가 중간에 실패해도 읽기 전용 폴더가 임시 폴더 정리를 막지 않는다. 권한은 공개 뒤에만 걸기 때문이다.
/// 폴더를 읽는 순서는 파일시스템이 정하므로, 읽지 못하는 파일보다 먼저 끝나는 읽기 전용 폴더가
/// 있도록 여럿 둔다.
#[cfg(unix)]
#[test]
fn a_failed_copy_with_a_read_only_folder_leaves_nothing_behind() {
    const LOCKED: [&str; 3] = ["a-locked", "keep", "sub"];
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("tree");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&dst).expect("mkdir");
    for name in LOCKED {
        std::fs::create_dir_all(src.join(name)).expect("mkdir");
        std::fs::write(src.join(name).join("x.txt"), "x").expect("write");
        chmod(&src.join(name), 0o555);
    }
    std::fs::write(src.join("unreadable.txt"), "u").expect("write");
    chmod(&src.join("unreadable.txt"), 0o000);

    let report = run_transfer(
        &Shared::fixed(Choice::KeepBoth),
        std::slice::from_ref(&src),
        &dst,
        false,
    );

    for name in LOCKED {
        chmod(&src.join(name), 0o755);
    }
    chmod(&src.join("unreadable.txt"), 0o644);
    assert_eq!(report.done, 0);
    assert_eq!(report.failed.len(), 1);
    assert!(names(&dst).is_empty(), "left behind: {:?}", names(&dst));
}

/// 사본이 원본의 옛 수정 시각을 가져도 되돌리기의 "작업 뒤 바뀌었나" 판정은 그대로다. 단계는 공개 뒤
/// 사본의 수정 시각(곧 원본의 시각)을 적고, 사용자가 고치면 시각이 달라져 남긴다.
#[test]
fn undo_still_tells_a_kept_time_from_a_later_edit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&src).expect("mkdir");
    std::fs::create_dir_all(&dst).expect("mkdir");
    std::fs::write(src.join("a.txt"), "copied").expect("write");
    let old = SystemTime::UNIX_EPOCH + OLD;
    set_modified(&src.join("a.txt"), old);

    let report = run_transfer(
        &Shared::fixed(Choice::KeepBoth),
        &[src.join("a.txt")],
        &dst,
        false,
    );
    let copy = dst.canonicalize().expect("canonical").join("a.txt");
    assert_eq!(report.undo, [UndoStep::Created(copy.clone(), Some(old))]);
    assert_eq!(
        modified_at(&copy),
        Some(old),
        "an untouched copy is still undoable"
    );

    std::fs::write(&copy, "edited").expect("edit the copy");
    let undone = run_undo(&Shared::fixed(Choice::KeepBoth), &report.undo);
    assert_eq!(
        undone.failed,
        [Failure {
            path: copy.clone(),
            reason: Reason::ChangedSince,
        }]
    );
    assert_eq!(read_text(&copy), "edited");
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).expect("read a test file")
}

#[cfg(unix)]
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("list a test folder")
        .map(|e| {
            e.expect("read a test entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}
