//! [`super::sync_dir_by_content`] 가 **무엇이든 건드릴지**를 쓰지 않고 묻는다.
//!
//! 같은 버전 재동기화는 대개 아무것도 안 쓴다. 그 plugin 이 회수 중이면 쓰기 전에 회수를
//! 끝까지 기다려야 하는데(실행 중인 파일은 Windows 에서 덮어쓰거나 지울 수 없고 Linux 에서는
//! `ETXTBSY` 가 난다), 쓸 것이 없는데 기다리면 메인 스레드가 이유 없이 최대 2 s 선다. 그래서
//! 회수 중일 때만 이것으로 먼저 묻는다.
//!
//! 판정은 동기화와 같은 자리를 같은 술어로 본다 — 층마다 청소할 항목이 있는가(src 에 없는
//! dest 항목), 파일마다 내용이 다른가([`super::file_content_differs`]). 청소도 "건드림" 으로
//! 센다: 동기화의 반환값은 복사만 세지만, 실행 중인 파일을 지우는 것도 같은 이유로 막힌다.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;

/// `src` 로 `dst` 를 맞추면 dst 의 무엇이든 바뀌는가. 읽다 실패하면 **바뀐다** 로 답한다 —
/// 그래야 판정 실패가 기다리지 않는 쪽이 아니라 기다리는 쪽으로 물러난다.
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
