//! 파일을 쓰지 않고 번들 동기화에 변경·삭제가 필요한지 확인한다.
//! 프로세스 회수 중 실제 파일 작업이 필요할 때만 회수를 기다리기 위한 검사다.
//! 파일 내용 비교와 불필요한 대상 항목 검사는 sync_dir_by_content와 같은 기준을 쓴다.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;

/// 동기화로 대상이 바뀌는지 확인한다. 조회 실패도 변경 필요로 처리해 회수를 기다리게 한다.
pub(super) fn sync_would_touch(src: &Path, dst: &Path) -> bool {
    touches(src, dst).unwrap_or(true)
}

fn touches(src: &Path, dst: &Path) -> std::io::Result<bool> {
    if !dst.is_dir() {
        return Ok(true);
    }
    let mut src_names = HashSet::<OsString>::new();
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        src_names.insert(entry.file_name());
        let dest_path = dst.join(entry.file_name());
        let changed = if entry.file_type()?.is_dir() {
            touches(&entry.path(), &dest_path)?
        } else {
            super::file_content_differs(&entry.path(), &dest_path)?
        };
        if changed {
            return Ok(true);
        }
    }
    for entry in std::fs::read_dir(dst)? {
        if !src_names.contains(&entry?.file_name()) {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::sync_would_touch;

    fn tree(root: &std::path::Path, files: &[(&str, &str)]) {
        for (rel, body) in files {
            let p = root.join(rel);
            std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
            std::fs::write(p, body).expect("write");
        }
    }

    #[test]
    fn identical_trees_are_not_touched() {
        let t = tempfile::tempdir().expect("tempdir");
        let (src, dst) = (t.path().join("s"), t.path().join("d"));
        tree(&src, &[("a", "1"), ("sub/b", "2")]);
        tree(&dst, &[("a", "1"), ("sub/b", "2")]);
        assert!(!sync_would_touch(&src, &dst));
    }

    #[test]
    fn a_changed_file_a_new_file_or_a_stale_file_touches() {
        let t = tempfile::tempdir().expect("tempdir");
        let src = t.path().join("s");
        tree(&src, &[("a", "1"), ("sub/b", "2")]);

        let changed = t.path().join("changed");
        tree(&changed, &[("a", "1"), ("sub/b", "3")]);
        assert!(sync_would_touch(&src, &changed), "내용이 다른 파일");

        let missing = t.path().join("missing");
        tree(&missing, &[("a", "1")]);
        assert!(sync_would_touch(&src, &missing), "dst 에 없는 파일");

        let stale = t.path().join("stale");
        tree(&stale, &[("a", "1"), ("sub/b", "2"), ("sub/old", "x")]);
        assert!(sync_would_touch(&src, &stale), "청소할 파일");

        assert!(
            sync_would_touch(&src, &t.path().join("absent")),
            "dst 가 없다"
        );
    }
}
