//! 실패 결과 중 Git이 추적하지 않는 파일을 별도로 안내한다.
//! 새 파일도 검사하기 위해 파일시스템 순회는 유지하되, 미추적 파일을 저장소의 예외 목록에
//! 등록하도록 잘못 안내하지 않게 한다. 미추적 파일이 대상 수를 늘려 순회 누락을 가리는
//! 경우는 실패 문구만으로 해결할 수 없으므로 검사 범위를 다시 검토해야 한다.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::Path;

thread_local! {
    /// 이 스레드가 Git을 호출한 횟수. 빈 입력의 조기 반환은 반환값만으로 구분할 수 없어 따로 센다.
    /// 병렬 시험의 계수가 섞이지 않도록 스레드별로 관리한다.
    static GIT_CALLS: Cell<usize> = const { Cell::new(0) };
}

/// rels 중 Git이 추적하지 않는 경로를 반환한다. 실패 진단을 만들 때만 호출한다.
/// Git 조회가 실패하면 원래 검사 오류를 가리지 않도록 빈 결과를 반환한다.
/// 따라서 빈 결과가 모든 경로의 추적 여부를 보장하지는 않는다.
pub fn untracked_among(root: &Path, rels: &[String]) -> BTreeSet<String> {
    if rels.is_empty() {
        return BTreeSet::new();
    }
    GIT_CALLS.with(|c| c.set(c.get() + 1));
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["ls-files", "-z", "--"])
        .args(rels)
        .output();
    let Ok(out) = out else {
        return BTreeSet::new();
    };
    if !out.status.success() {
        return BTreeSet::new();
    }
    let tracked: BTreeSet<&str> = std::str::from_utf8(&out.stdout)
        .unwrap_or("")
        .split('\0')
        .filter(|s| !s.is_empty())
        .collect();
    rels.iter()
        .filter(|r| !tracked.contains(r.as_str()))
        .cloned()
        .collect()
}

/// 미추적 경로가 있으면 실패 메시지에 붙일 안내를 반환하고 없으면 빈 문자열을 반환한다.
pub fn outside_repo_note(root: &Path, rels: &[String]) -> String {
    let outside = untracked_among(root, rels);
    if outside.is_empty() {
        return String::new();
    }
    let list = outside
        .iter()
        .map(|r| format!("  {r}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "\n\ngit 이 추적하지 않는 것이 검사 결과에 포함돼 있다:\n{list}\n이 파일에는 위 처방을 쓰지 마라. 저장소의 예외 목록에 등록하는 대신 검사 범위 밖으로 옮기거나 불필요한 파일인지 확인한다."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tracked_path_is_not_reported() {
        let root = crate::repo_root();
        let rels = vec!["Cargo.toml".to_string()];
        assert!(
            untracked_among(&root, &rels).is_empty(),
            "추적되는 파일이 밖에서 온 것으로 세어졌다"
        );
        assert_eq!(outside_repo_note(&root, &rels), "");
    }

    #[test]
    fn a_path_that_is_not_in_the_index_is_reported() {
        let root = crate::repo_root();
        // 실재하지 않아도 된다 — 물음은 "git 이 아는가" 이지 "디스크에 있는가" 가 아니다.
        let rels = vec!["this-path-is-not-in-the-index-93.md".to_string()];
        assert_eq!(
            untracked_among(&root, &rels),
            rels.iter().cloned().collect()
        );
        let note = outside_repo_note(&root, &rels);
        assert!(
            note.contains("git 이 추적하지 않는 것"),
            "문단이 안 나왔다: {note}"
        );
        assert!(
            note.contains("위 처방을 쓰지 마라"),
            "처방을 쓰지 말라는 말이 빠졌다 — 이 문단의 존재 이유다: {note}"
        );
    }

    /// 빈 입력에서는 반환값뿐 아니라 Git 호출 횟수도 증가하지 않아야 한다.
    #[test]
    fn an_empty_input_costs_nothing_and_says_nothing() {
        let root = crate::repo_root();
        let before = GIT_CALLS.with(Cell::get);
        assert!(untracked_among(&root, &[]).is_empty());
        assert_eq!(outside_repo_note(&root, &[]), "");
        assert_eq!(
            GIT_CALLS.with(Cell::get),
            before,
            "빈 입력에서 git을 호출했다. 불필요한 조회를 하지 않도록 조기 반환을 유지한다."
        );
    }
}
