//! 실패문에 붙이는 한 문장 — **이 좌표가 레포의 것인가.**
//!
//! 레포 루트에서 시작하는 순회는 작업 트리에 있는 것을 전부 본다. git 이 추적하는
//! 파일과 개발자가 그 자리에 둔 파일은 파일시스템에서 구별되지 않는다. 그래서 그런
//! 가드는 **레포 밖에서 온 파일을 위반으로 보고할 수 있다.**
//!
//! ## 왜 좌변을 git 으로 바꾸지 않는가 (2026-09-08 판단)
//!
//! 그 자리를 닫는 방법은 둘이다.
//!
//! 1. **좌변을 `git ls-files` 로 잡는다.** 원리적으로 닫히지만 값이 비싸다 — 아직
//!    `git add` 안 된 새 문서 한 장이 그 순간 좌변을 **판정 불가**로 만든다. 셸 게이트는
//!    종료 코드 2 로 그 상태를 말할 수 있어서 실제로 그렇게 한다(그 게이트들은 사람이
//!    커밋 직전에 부른다). 시험은 통과/실패 둘뿐이라 같은 상태를 **실패**로밖에 못 낸다.
//!    528 건짜리 패키지를 도는 CI 에서 그것은 작업 중인 트리마다 빨개지는 것이고,
//!    그 빨강의 처방("`git add` 하고 다시 돌려라")은 회귀와 아무 상관이 없다.
//! 2. **좌변은 그대로 두고 실패문을 고친다.** 여기서 하는 것이다.
//!
//! 둘째를 고른 근거는 이 축의 실패가 **빨강**이지 조용한 통과가 아니라는 것이다.
//! 빨강은 시끄럽지만 안전하다 — 위험한 것은 그 빨강에 붙는 **처방**이다. 이 축의
//! 가드들은 "명부에 등록해라" 로 처방하는데, 레포 밖 파일을 명부에 등록하면 그 자리가
//! 영구히 면제되고 그 면제는 실재하지 않는 위반에 붙는다. 그러니 닫아야 하는 것은
//! 순회가 아니라 **처방이 잘못 붙는 것**이다.
//!
//! 이 판단이 다시 열리는 조건: 이 축의 오탐이 **조용한 통과** 쪽으로 한 건이라도
//! 나오면(예: 레포 밖 파일이 좌변을 늘려 하한을 채워 버리는 형태) 첫째로 옮겨야 한다.
//! 그때는 실패문이 아니라 좌변이 문제다.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::Path;

thread_local! {
    /// 이 스레드에서 [`untracked_among`] 이 `git` 을 **실제로 부른** 횟수.
    ///
    /// 이 값이 없으면 "빈 입력에서는 git 을 안 부른다" 는 주장이 **반환값으로 관측되지
    /// 않는다.** 조기 반환을 지워도 `git ls-files -z --` 는 경로 인자가 없으면 추적 파일
    /// 전체를 내고, 그것을 거르는 `rels` 가 비어 있어 결과는 그대로 빈 집합이다 — 두 판이
    /// 같은 답을 낸다(실측 2026-09-08: 조기 반환을 지우고 `-p tasty-doc-guards --lib
    /// tracked_scope` rc=0 · 3 passed). 그래서 세는 자리를 따로 뒀다.
    ///
    /// **세는 쪽에 `#[cfg(test)]` 를 안 붙인다** — 두 빌드가 다른 코드를 돌면 재현이
    /// 갈린다. 읽는 쪽만 시험이다.
    ///
    /// **스레드 지역인 이유**: 이 크레이트의 시험은 한 바이너리 안에서 병렬로 돈다.
    /// 전역 계수기면 형제 시험이 올린 값이 섞여 이 판정이 부하에 흔들린다. 스레드 지역은
    /// 잠금 없이 그 섞임을 없앤다.
    static GIT_CALLS: Cell<usize> = const { Cell::new(0) };
}

/// `rels` 중 git 이 **추적하지 않는** 것.
///
/// **실패 경로에서만 부른다.** 초록일 때는 아무도 이 답을 안 쓰므로 git 을 부를 이유가
/// 없고, 부르면 초록의 비용이 된다.
///
/// git 이 없거나 실패하면 **빈 집합**을 준다. 여기서 죽지 않는 이유는 이 함수가
/// 판정이 아니기 때문이다 — 판정은 이미 났고(부르는 쪽이 실패문을 짓는 중이다) 이것은
/// 그 실패문에 한 문장을 더할 수 있는지일 뿐이다. 판정에 쓰는 자리였다면 반대로
/// 죽어야 한다(`src/source_guards/scan_population.rs` 가 그 예다).
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

/// 실패문 끝에 붙일 문단 — 위반 좌표 중 레포 밖에서 온 것이 있으면.
///
/// 없으면 빈 문자열이라 `{}` 로 그냥 이어 붙여도 된다.
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
        "\n\n★ 위 좌표 중 **git 이 추적하지 않는 것**이 있다:\n{list}\n\
         이 순회는 레포 루트에서 시작해 작업 트리에 있는 것을 전부 본다 — 그래서 네가 \
         그 자리에 둔 파일이 레포의 위반으로 보고될 수 있다. **그 좌표에는 위 처방을 \
         쓰지 마라**: 명부에 등록하면 레포에 없는 경로가 영구히 면제되고, 고쳐도 \
         레포는 아무것도 안 달라진다. 그 파일을 옮기거나 지우고 다시 돌려라."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 양성 대조(2026-09-08, 내 트리): `ls-files` 출력을 통째로 버리면(전부 미추적으로
    /// 읽힌다) **이 시험만** 죽는다 — 형제 둘은 초록이다.
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

    /// 양성 대조(2026-09-08, 내 트리): git 성공 갈래를 빈 집합으로 바꾸면 미추적을 아무도
    /// 보고하지 않게 되고 **이 시험만** 죽는다 — 형제 둘은 초록이다.
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

    /// **비용이 0 이라는 것은 반환값이 아니라 호출 횟수로만 갈린다.**
    ///
    /// 처음 판은 반환값만 봤고 그것은 항진명제였다 — 조기 반환을 지워도 답이 같다
    /// (실측 2026-09-08 rc=0). 지금은 `GIT_CALLS` 를 걸어서, 조기 반환이 사라지면
    /// 이 시험이 죽는다.
    #[test]
    fn an_empty_input_costs_nothing_and_says_nothing() {
        let root = crate::repo_root();
        let before = GIT_CALLS.with(Cell::get);
        assert!(untracked_among(&root, &[]).is_empty());
        assert_eq!(outside_repo_note(&root, &[]), "");
        assert_eq!(
            GIT_CALLS.with(Cell::get),
            before,
            "빈 입력에서 `git` 을 불렀다 — 이 함수는 실패 경로에서만 불리므로 빈 입력의 \
             비용이 곧 초록의 비용이다. 조기 반환을 지웠으면 되돌려라."
        );
    }
}
