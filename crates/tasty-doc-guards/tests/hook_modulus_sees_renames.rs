//! **pre-commit 의 주 모수가 이름 바뀐 파일을 보는가** 를 원문 대조로 지킨다.
//!
//! ## 왜 훅이 스스로 못 지키는가
//!
//! 훅의 주 모수(`STAGED_ALL`)는 한때 `--diff-filter=ACM` 이었다. `R`(이름 바뀜)이 빠져
//! 있어서 **순수 rename 커밋의 모수가 0** 이 됐고, 그 값에서 조기 return 하는 검사 여섯
//! (mod/use 순서 · `cargo fmt` · `let _` 사유 · `egui::Window` · 그 외 둘)이 **한 줄도 안
//! 보고 초록**을 냈다. 실측: 옮기기만 한 커밋에서 `staged .rs 0 · staged 전체 0`.
//!
//! ★ 이 좁아짐은 **훅 안에서는 못 잡는다.** 훅이 걸 수 있는 것은 "모수가 N 이상" 인데,
//! 문서만 담은 커밋은 `.rs` 0 이 **정상**이다. 하한을 두면 정상 커밋이 빨개지고, 안 두면
//! 좁아짐이 안 잡힌다 — **값으로는 정상과 좁아짐을 못 가른다.** 그래서 값이 아니라
//! **필터 문자열 자체**를 밖에서 읽는다. 이 시험이 그 자리다.
//!
//! ## `D`(지움)를 요구하지 않는 이유
//!
//! 주 모수를 소비하는 검사들은 전부 **파일을 연다**. 지워진 경로를 열면 없는 파일이고,
//! 그것을 실패로 셀지 통과로 셀지가 또 갈린다. 지움을 봐야 하는 검사(P.1)는 자기 자리에서
//! `ACMRD` 로 따로 묻는다 — **모수는 검사의 성질을 따라간다.** 한 파일에 모수가 둘인 것은
//! 결함이 아니라 옳은 상태다. 그래서 여기서는 `R` 만 요구하고 `D` 는 요구하지 않는다.
//!
//! ## 0 을 통과로 만들지 않는다
//!
//! 훅 파일이 없거나 비었거나 주 모수 줄의 모양이 바뀌면 **찾은 것이 0** 이 되고 0 은 언제나
//! 초록이다(ADR-0133). 그래서 단정 앞에 "몇 개를 찾았는가" 를 세우고, 그 수를 초록에서도
//! 남긴다:
//!
//! ```text
//! cargo test -p tasty-doc-guards --test hook_modulus_sees_renames -- --nocapture
//! ```

use tasty_doc_guards::repo_root;

/// 훅 경로. 빌드 산출물이 아니라 추적되는 파일이다.
const HOOK: &str = ".githooks/pre-commit";

/// 주 모수를 정하는 대입. 이 철자가 바뀌면 아래 수가 0 이 되고 그것이 실패로 잡힌다.
const MODULUS_ASSIGN: &str = "STAGED_ALL=";

/// 주 모수에서 `.rs` 를 갈라 내는 파생. 이것이 없으면 여섯 검사가 다른 것을 본다.
const DERIVED_ASSIGN: &str = "STAGED_RS=";

#[test]
fn the_staged_modulus_includes_renames() {
    let path = repo_root().join(HOOK);
    let src = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("훅을 읽을 수 없다: {} — {e}", path.display()));

    let modulus_lines: Vec<&str> = src
        .lines()
        .filter(|l| l.trim_start().starts_with(MODULUS_ASSIGN))
        .collect();
    let derived_lines: Vec<&str> = src
        .lines()
        .filter(|l| l.trim_start().starts_with(DERIVED_ASSIGN))
        .collect();
    let consumers = src.matches("$STAGED_RS").count();
    let filter = modulus_lines
        .first()
        .and_then(|l| l.split("--diff-filter=").nth(1))
        .map(|rest| {
            rest.chars()
                .take_while(|c| c.is_ascii_uppercase())
                .collect::<String>()
        });

    // 단정보다 **앞**에 둔다 — 빨간 경로에서도 모수가 남아야 한다(형제 가드와 같은 형태).
    eprintln!(
        "[hook-modulus] 훅 {} 줄 · 주 모수 대입 {} · 파생 대입 {} · 소비 자리 {} · 필터 {:?}",
        src.lines().count(),
        modulus_lines.len(),
        derived_lines.len(),
        consumers,
        filter.as_deref().unwrap_or("(못 찾음)")
    );

    // ── 자기-공허 방지: 찾은 것이 0 이면 아래 초록은 거짓이다 ──────────────────
    assert_eq!(
        modulus_lines.len(),
        1,
        "주 모수 대입 `{MODULUS_ASSIGN}` 을 {} 개 찾았다(1 이어야 한다).\n  \
         [판별식] 0 이면 이름이 바뀌었거나 대입 모양이 달라진 것이고, 그때 이 시험은 \
         **아무것도 안 보면서 초록**이 된다. 2 이상이면 모수가 둘로 갈렸다는 뜻이라 \
         어느 쪽이 여섯 검사에 흘러가는지 이 시험이 답을 못 한다.",
        modulus_lines.len()
    );
    assert_eq!(
        derived_lines.len(),
        1,
        "`.rs` 파생 대입 `{DERIVED_ASSIGN}` 을 {} 개 찾았다(1 이어야 한다) — 주 모수를 \
         넓혀도 파생이 딴 곳에서 오면 여섯 검사가 넓어진 모수를 안 본다.",
        derived_lines.len()
    );
    assert!(
        derived_lines[0].contains("$STAGED_ALL"),
        "`.rs` 목록이 주 모수에서 갈라져 나오지 않는다 — 두 값이 **같은 한 번의 조회**에서 \
         나와야 통과 줄이 찍는 수가 검사들이 실제로 순회한 것과 같다.\n  받은 줄: {}",
        derived_lines[0].trim()
    );
    assert!(
        consumers >= 1,
        "주 모수의 `.rs` 파생을 쓰는 자리가 하나도 없다 — 모수를 만들어 놓고 아무도 안 \
         쓰는 상태다. 검사들이 다른 목록을 보고 있다."
    );

    // ── 실판정: 이름 바뀐 파일이 모수에 들어오는가 ──────────────────────────
    let filter = filter.expect("주 모수 줄에 `--diff-filter=` 가 없다 — 필터 없이 전부를 세면 이 시험의 물음 자체가 사라진다");
    for (letter, why) in [
        ('A', "새로 더한 파일"),
        ('C', "복사된 파일"),
        ('M', "고쳐진 파일"),
        ('R', "**이름이 바뀐 파일**"),
    ] {
        assert!(
            filter.contains(letter),
            "주 모수 필터가 `{filter}` 라 {why}({letter})이 빠져 있다.\n  \
             그 종류의 파일만 담은 커밋은 **모수가 0** 이 되고, 이 모수에서 조기 return 하는 \
             검사들이 한 줄도 안 보고 초록을 낸다.\n  \
             [실측] `R` 이 빠져 있던 동안 순수 rename 커밋에서 `staged .rs 0 · staged 전체 0` \
             이 찍혔다. 옮긴 파일은 내용이 같아도 **자리가 달라진다** — mod/use 순서와 \
             크레이트 귀속은 자리가 정한다.\n  \
             [`D` 를 안 넣는 이유] 이 모수를 쓰는 검사들은 파일을 연다. 지움은 파일을 안 여는 \
             검사가 자기 자리에서 따로 묻는다(모듈 주석 참조)."
        );
    }
}
