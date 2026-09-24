//! 복잡도 예외 목록의 동결·부채 블록 설명을 실제 항목 수와 대조한다.
//! 기준과 예외 정책은 docs/dev-guide/complexity-gate.md를 따른다.

use std::path::PathBuf;

const ALLOWLIST: &str = ".complexity-file-allowlist";

/// 부채 블록의 시작을 가르는 표식. allowlist 안의 주석 한 줄이다.
const DEBT_MARKER: &str = "# ──";

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("읽을 수 없다: {} — {e}", path.display()))
}

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

/// 접두어 뒤 정수를 읽는다. 없거나 중복되면 0으로 대신하지 않고 오류를 반환한다.
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
            "합성 변경이 적용되지 않았다. 문서의 prefix를 확인한다."
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
            "설명 문장이 없는데 오류 대신 값을 반환했다"
        );
    }

    #[test]
    fn the_block_split_actually_splits() {
        let text = read(ALLOWLIST);
        let (total, frozen, debt) = count_entries(&text);
        assert!(
            frozen > 0 && debt > 0 && total == frozen + debt,
            "블록 집계가 다르다: 총 {total}·동결 {frozen}·부채 {debt}"
        );

        let flattened = text.replace(DEBT_MARKER, "# (표식 없음)");
        let (t2, f2, d2) = count_entries(&flattened);
        assert_eq!(
            (t2, f2, d2),
            (total, total, 0),
            "표식이 판정에 안 쓰이고 있다"
        );
    }

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
