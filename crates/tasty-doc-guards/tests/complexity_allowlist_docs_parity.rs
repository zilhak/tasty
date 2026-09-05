//! 복잡도 게이트의 **면제 개수**를 문서가 손으로 베끼는 것을 막는다.
//!
//! `.complexity-file-allowlist` 의 항목 수와 `cognitive_complexity` 면제 속성의
//! 출현 수는 어디서도 파생되지 않는다. 문서 두 곳이 그 수를 각자 평문으로 들고 있어서,
//! 항목이 늘거나 줄면 사람이 두 곳을 따로 고쳐야 한다. 실제로 안 고쳐졌다 —
//! ADR-0131 이 allowlist 에 26 건을 더하면서 ADR-0037 의 "현재 18개" 를 두고 갔고,
//! 함수 쪽 "35곳" 은 두 문서에 각자 베껴진 채 둘 다 34 와 어긋나 있었다.
//!
//! **틀린 수는 조용하다.** 게이트는 개수를 안 보므로 계속 초록이고, 문서를 읽는 사람만
//! 잘못된 규모를 갖고 판단한다. 그래서 기계가 본다.
//!
//! ## 무엇을 보고 무엇을 안 보는가
//!
//! 보는 것은 **현재 상태를 주장하는 수**뿐이다 — `docs/dev-guide/complexity-gate.md` 와
//! `docs/adr/0037-complexity-gate.md` 의 "현재 N", 그리고 `.complexity-file-allowlist`
//! 자신이 두 블록을 가리키며 쓴 수.
//!
//! **안 보는 것**: `docs/adr/0131-file-sloc-gate-needs-a-firing-trigger.md` 의 18/26 은
//! 날짜가 붙은 시점 측정이다("게이트 도입은 2026-07-06 … 그때 18 건을 동결했다",
//! "2026-09-04 현재 … 26 건"). 현재 상태 주장이 아니라 결정의 근거로 남아야 하는 기록이라
//! 지금 값과 갈라지는 것이 정상이다. 같은 이유로 `complexity-gate.md` 가 "동결한 18 건에서
//! 래칫으로 한 건이 빠진 나머지" 라고 쓸 때의 **18 도 안 본다** — 그건 도입 시점 값이다.
//! `docs/dev-guide/clippy-policy.md` 는 allowlist 를 언급하지만 수를 주장하지 않아 대상이
//! 아니다. 세는 척하지 않으려고 여기 적어 둔다.
//!
//! ## 채널
//!
//! 이 타깃은 `doc-guards.yml` 이 main push · PR 마다 돌리고, `check-headless` 의 전체
//! 스위트에서도 돈다. 자동 잡은 push 된 커밋만 보므로 **커밋 전에 직접 돌리면 그 자리에서 잡힌다**
//! (`docs/dev-guide/ci-gates.md`).

use std::path::PathBuf;
use std::process::Command;

const ALLOWLIST: &str = ".complexity-file-allowlist";
const GUIDE: &str = "docs/dev-guide/complexity-gate.md";
const ADR: &str = "docs/adr/0037-complexity-gate.md";

/// 부채 블록의 시작을 가르는 표식. allowlist 안의 주석 한 줄이다.
const DEBT_MARKER: &str = "# ──";

/// 레포 루트 — 이 크레이트가 `crates/` 아래 살아서 `CARGO_MANIFEST_DIR` 이 레포 루트가
/// 아니다. 해석과 검증을 [`tasty_doc_guards::repo_root`] 한 곳에 모은다(ADR-0138).
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

/// 찾을 문자열. **쪼개서 만든다** — 이 파일이 바늘을 통째로 갖고 있으면 자기 자신을 세게
/// 되고, 그러면 문서가 적어야 할 수가 "레포의 면제 수 + 이 테스트가 자기를 언급한 횟수"
/// 라는 자기 참조가 된다(실제로 처음엔 34 대신 37 이 나왔다). 자기 제외 면제를 두는 대신
/// 애초에 매칭되지 않게 만든다 — 면제는 그것을 겨냥한 변이를 또 요구한다.
/// `the_needle_is_not_written_whole_in_this_file` 이 이 쪼갬을 못박는다.
fn needle() -> String {
    format!("allow(clippy::{}", "cognitive_complexity)")
}

/// 추적 `.rs` 안의 `cognitive_complexity` 면제 속성 출현 수.
///
/// 모수를 git 으로 잡는다 — ADR-0037 이 이 값의 기준을 "추적 `.rs`" 라고 적었고,
/// 레포 전체 `grep -rn` 은 `clippy.toml`·문서까지 세어 다른 값을 낸다.
fn cognitive_allow_sites() -> usize {
    let root = repo_root();
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-co", "--exclude-standard", "--", "*.rs"])
        .output()
        .unwrap_or_else(|e| panic!("`git ls-files` 를 실행할 수 없다 — {e}"));
    assert!(
        output.status.success(),
        "`git ls-files` 가 실패했다: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );

    let needle = needle();
    let mut files = 0usize;
    let mut sites = 0usize;
    for rel in String::from_utf8_lossy(&output.stdout).lines() {
        let path = root.join(rel.split('/').collect::<PathBuf>());
        if !path.is_file() {
            continue;
        }
        files += 1;
        sites += std::fs::read_to_string(&path)
            .unwrap_or_default()
            .matches(needle.as_str())
            .count();
    }
    assert!(
        files > 500,
        "추적 `.rs` 를 {files} 개만 읽었다 — 모수가 무너지면 출현 수 0 도 초록이 된다"
    );
    sites
}

#[test]
fn docs_state_the_actual_exemption_counts() {
    let (total, frozen, debt) = count_entries(&read(ALLOWLIST));
    let sites = cognitive_allow_sites();

    // 같은 산출물의 비영 대조를 판정 앞에 둔다 — 전부 0 이면 아래 비교는 전부 통과한다.
    assert!(
        total > 0 && frozen > 0 && debt > 0 && sites > 0,
        "센 값이 0 이다 — allowlist 총 {total}(동결 {frozen} · 부채 {debt}) · allow {sites}. \
         파싱이 깨졌는지 확인해라"
    );
    assert_eq!(total, frozen + debt, "블록 합이 총계와 다르다");

    let guide = read(GUIDE);
    let adr = read(ADR);
    let allowlist = read(ALLOWLIST);
    let claims: &[(&str, &str, &str, usize)] = &[
        (GUIDE, &guide, "`.complexity-file-allowlist` (현재 ", total),
        (GUIDE, &guide, "도입 시 동결분 잔여 ", frozen),
        (GUIDE, &guide, "채널 부재로 쌓인 부채 ", debt),
        (GUIDE, &guide, "블록이 둘이다.** 위 ", frozen),
        (GUIDE, &guide, "// complexity-exempt:` (현재 ", sites),
        (ADR, &adr, "`.complexity-file-allowlist`(현재 ", total),
        (ADR, &adr, "complexity-exempt: <사유>`(현재 ", sites),
        (ALLOWLIST, &allowlist, "# ── 아래 ", debt),
        (ALLOWLIST, &allowlist, "# 위 ", frozen),
    ];

    let mut wrong = Vec::new();
    for (name, text, prefix, actual) in claims {
        match claimed(text, prefix) {
            Ok(said) if said == *actual => {}
            Ok(said) => wrong.push(format!(
                "  {name}: `{prefix}…` 가 {said} 라는데 실측은 {actual}"
            )),
            Err(why) => wrong.push(format!("  {name}: `{prefix}…` 를 못 읽었다 — {why}")),
        }
    }

    assert!(
        wrong.is_empty(),
        "문서가 주장하는 면제 개수가 실측과 다르다. 이 수는 어디서도 파생되지 않고 사람이 \
         베끼므로, 항목을 더하거나 지운 커밋에서 문서도 같이 고쳐라.\n\
         실측: allowlist 총 {total}(동결 {frozen} · 부채 {debt}) · cognitive allow {sites} 곳\n{}",
        wrong.join("\n")
    );
}

/// 이 정합을 겨냥한 변이 — 판정기가 실제로 무는지 확인한다. 파일은 안 고친다.
mod exemption_mutations {
    use super::*;

    #[test]
    fn a_wrong_number_in_a_doc_is_caught() {
        let text = read(GUIDE);
        let prefix = "`.complexity-file-allowlist` (현재 ";
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
        let text = read(GUIDE);
        let prefix = "`.complexity-file-allowlist` (현재 ";
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

    /// 이 파일이 바늘을 통째로 갖고 있지 않은가 — 가지면 자기 자신을 세어 문서의 수가
    /// 자기 참조가 된다. 위 `needle()` 의 쪼갬이 살아 있다는 증거다.
    #[test]
    fn the_needle_is_not_written_whole_in_this_file() {
        let me = read("crates/tasty-doc-guards/tests/complexity_allowlist_docs_parity.rs");
        assert!(
            !me.contains(&needle()),
            "이 파일이 바늘을 통째로 갖고 있다 — 세는 대상에 자기가 들어간다. \
             `needle()` 의 쪼갬을 되돌리지 마라"
        );
        // 0 을 보고하는 자리라 같은 산출물의 비영 대조를 같은 자리에 둔다: 파일은 실제로 읽혔다.
        assert!(
            me.contains("cognitive_complexity"),
            "이 파일을 못 읽었다 — 그러면 위 단정은 언제나 통과한다"
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
