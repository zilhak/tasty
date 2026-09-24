//! 복잡도 예외 목록의 블록별 개수 설명이 실제 항목 수와 같은지 확인한다.
//!
//! ADR과 가이드는 변하는 면제 개수를 더 이상 복제하지 않는다. 그 사본을 검사하던
//! 부분은 제거하고, 실제 allowlist가 보유한 동결·부채 블록 설명의 정합은 유지한다.
//! 복잡도 기준과 예외 정책은 docs/dev-guide/complexity-gate.md를 따른다.

use std::path::PathBuf;

const ALLOWLIST: &str = ".complexity-file-allowlist";

/// 부채 블록의 시작을 가르는 표식. allowlist 안의 주석 한 줄이다.
const DEBT_MARKER: &str = "# ──";

/// 레포 루트 — 이 크레이트가 `crates/` 아래 살아서 `CARGO_MANIFEST_DIR` 이 레포 루트가
/// 아니다. 해석과 검증을 [`tasty_doc_guards::repo_root`] 한 곳에 모은다(ADR-0048).
fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("읽을 수 없다: {} — {e}", path.display()))
}

/// allowlist 를 `(총계, 동결 블록, 부채 블록)` 으로 센다.
///
/// 순수 함수다 — 변이 테스트가 파일을 안 고치고 찌를 수 있어야 한다.
fn count_entries(text: &str) -> (usize, usize, usize) {
    let mut frozen = 0usize;
    let mut debt = 0usize;
    let mut in_debt = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(DEBT_MARKER) {
            in_debt = true;
            continue;
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if in_debt { debt += 1 } else { frozen += 1 }
    }
    (frozen + debt, frozen, debt)
}

/// `prefix` 바로 뒤에 붙은 정수를 읽는다.
///
/// 못 읽으면 `Err` 다 — **0 을 돌려주지 않는다.** 정규식이 문장을 못 찾았을 때 0 을 내면
/// 그 0 이 실제 값과 우연히 같은 날 이 테스트는 조용히 죽는다.
fn claimed(text: &str, prefix: &str) -> Result<usize, String> {
    let hits = text.match_indices(prefix).count();
    if hits != 1 {
        return Err(format!(
            "`{prefix}` 가 {hits} 번 나온다 — 정확히 1 번이어야 판정할 수 있다"
        ));
    }
    let rest = &text[text.find(prefix).expect("위에서 1 번 나오는 것을 확인했다") + prefix.len()..];
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return Err(format!(
            "`{prefix}` 뒤에 숫자가 없다 — 문장이 바뀌었으면 이 테스트의 prefix 도 고쳐라"
        ));
    }
    digits
        .parse()
        .map_err(|e| format!("`{digits}` 를 못 읽는다: {e}"))
}

#[test]
fn allowlist_comments_state_the_actual_block_counts() {
    let allowlist = read(ALLOWLIST);
    let (total, frozen, debt) = count_entries(&allowlist);
    assert!(
        total > 0 && frozen > 0 && debt > 0,
        "allowlist 항목을 읽지 못했다: 총 {total}, 동결 {frozen}, 부채 {debt}"
    );
    assert_eq!(total, frozen + debt);
    for (prefix, actual) in [("# ── 아래 ", debt), ("# 위 ", frozen)] {
        let stated = claimed(&allowlist, prefix).unwrap_or_else(|why| panic!("{why}"));
        assert_eq!(
            stated, actual,
            "{ALLOWLIST}의 `{prefix}` 설명과 실제 블록 항목 수가 다르다"
        );
    }
}

/// 이 정합을 겨냥한 변이 — 판정기가 실제로 무는지 확인한다. 파일은 안 고친다.
mod exemption_mutations {
    use super::*;

    #[test]
    fn a_wrong_number_in_an_allowlist_comment_is_caught() {
        let text = read(ALLOWLIST);
        let prefix = "# ── 아래 ";
        let said = claimed(&text, prefix).expect("무변이 대조가 먼저 읽혀야 한다");

        let mutated = text.replacen(
            &format!("{prefix}{said}"),
            &format!("{prefix}{}", said + 1),
            1,
        );
        assert_ne!(
            mutated, text,
            "변이가 안 먹었다 — prefix 가 문서와 안 맞는다"
        );
        assert_eq!(
            claimed(&mutated, prefix).expect("변이본도 읽혀야 한다"),
            said + 1,
            "틀린 수를 그대로 읽어내지 못하면 위 비교가 그것을 잡을 수 없다"
        );
    }

    #[test]
    fn a_claim_that_vanished_is_an_error_not_a_zero() {
        let text = read(ALLOWLIST);
        let prefix = "# ── 아래 ";
        assert!(claimed(&text, prefix).is_ok(), "무변이 대조");

        let removed = text.replacen(prefix, "여기 있던 문장이 사라졌다 ", 1);
        assert!(
            claimed(&removed, prefix).is_err(),
            "문장이 사라졌는데 값을 돌려줬다 — 0 이나 기본값을 내면 그 수와 우연히 같은 날 \
             이 테스트가 조용히 죽는다"
        );
    }

    #[test]
    fn the_block_split_actually_splits() {
        let text = read(ALLOWLIST);
        let (total, frozen, debt) = count_entries(&text);
        assert!(
            frozen > 0 && debt > 0 && total == frozen + debt,
            "블록 구분이 안 먹었다: 총 {total} · 동결 {frozen} · 부채 {debt}"
        );

        // 표식을 지우면 전부 동결 블록으로 접힌다 — 표식이 실제로 가르고 있다는 증거다.
        let flattened = text.replace(DEBT_MARKER, "# (표식 없음)");
        let (t2, f2, d2) = count_entries(&flattened);
        assert_eq!(
            (t2, f2, d2),
            (total, total, 0),
            "표식이 판정에 안 쓰이고 있다"
        );
    }

    /// 주석과 빈 줄이 항목으로 새지 않는가 — 새면 총계가 부풀어 문서와 어긋난다.
    #[test]
    fn comments_and_blanks_are_not_counted() {
        let text = read(ALLOWLIST);
        let (total, _, _) = count_entries(&text);
        let padded = format!("{text}\n\n# 새로 붙인 주석\n\n");
        assert_eq!(count_entries(&padded).0, total);
        assert_eq!(
            count_entries(&format!("{text}\nsrc/새_항목.rs\n")).0,
            total + 1
        );
    }
}
