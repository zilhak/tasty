//! 토큰 census 를 말하는 자리가 **정본 하나와 갈리지 않는가.**
//!
//! 정본은 `crates/tasty-design-tokens/tests/freshness.rs` 의
//! `token_census_matches_design_export` 다 — 티어별 셋과 합계 하나를 `assert_eq!` 로
//! 박아 두어, vendor json 이 바뀌면 그 자리가 빨개진다.
//!
//! ## 왜 필요한가 — 같은 수가 세 자리에 있고 둘만 낡았다
//!
//! 그 수를 **산문으로 다시 적은 자리가 둘** 더 있다(크레이트 README 의 갱신 절차 4 단계,
//! 크레이트 모듈 주석). 둘 다 정본에서 파생되지 않는다 — 손으로 적힌 사본이다.
//!
//! 실제로 갈렸다. `statusbar-glyph-size` 한 종이 들어와 census 가 817 → 818 이 된 회차에
//! `freshness.rs` 만 818 로 갔고, 두 산문은 817 로 남았다. 빌드도 시험도 CI 도 전부
//! 초록이었다 — 그 두 자리를 읽는 것이 레포에 없었기 때문이다. 그 상태에서 다음 사람이
//! README 의 절차를 따라 "census 가 817 에서 바뀌었나" 를 판단하면 **틀린 기준과 견준다.**
//!
//! ## 무엇을 재는가
//!
//! - [`CLAIMS`] 의 각 자리가 정본과 **같은 수**를 적는가.
//! - 그 크레이트 안에 **등록 안 된 census 서술**이 새로 생기지 않았는가. 자리를 하나
//!   늘리고 명부에 안 올리면 그 자리는 영구히 안 보인다.
//!
//! ## 오차 방향
//!
//! **더 잡는 쪽으로 틀린다.** census 꼴(`a/b/c` 삼중항, `총 N 토큰`)을 쓰는 다른 문장이
//! 그 크레이트에 생기면 등록을 요구한다. 그 처방(명부에 올려라 · 사유와 함께 제외해라)은
//! 아무것도 헐겁게 만들지 않는다.
//!
//! ## 자동 채널
//!
//! `doc-guards.yml` 이 main push · PR 마다 이 크레이트를 돌린다(경로 필터 없음).

use std::path::PathBuf;

const CANONICAL: &str = "crates/tasty-design-tokens/tests/freshness.rs";
const CRATE_DIR: &str = "crates/tasty-design-tokens";

/// census 를 산문으로 다시 적는 자리 — (파일, 그 수 **직전**의 고정 문구).
///
/// 문구로 집는 이유는 위치가 움직이기 때문이다. 문구가 바뀌면 그 자리를 못 찾아
/// 실패하므로, 조용히 사면되는 갈래가 없다.
const CLAIMS: &[(&str, &str)] = &[
    ("crates/tasty-design-tokens/README.md", "토큰 census("),
    ("crates/tasty-design-tokens/src/lib.rs", "component, 총 "),
];

/// 등록 안 된 census 서술 검사에서 뺄 자리와 사유. **여유 0** 이다.
const EXCLUDED: &[(&str, &str)] = &[(
    CANONICAL,
    "정본 자신이다. 여기 적힌 수는 사본이 아니라 판정하는 값이고, 지난 회차 수를 함께 \
     적는 것이 그 파일의 일이다",
)];

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 를 못 읽었다: {e}", path.display()))
}

/// `at` 부터 이어지는 십진 숫자를 읽는다. 없으면 `None`.
fn number_at(s: &str, at: usize) -> Option<(u32, usize)> {
    let b = s.as_bytes();
    let mut end = at;
    while end < b.len() && b[end].is_ascii_digit() {
        end += 1;
    }
    if end == at {
        return None;
    }
    Some((s[at..end].parse().ok()?, end))
}

/// 정본 — `token_census_matches_design_export` 가 박은 (primitive, semantic, component, 합).
fn canonical_census() -> (u32, u32, u32, u32) {
    let src = read(CANONICAL);
    let at = src
        .find("fn token_census_matches_design_export()")
        .unwrap_or_else(|| panic!("{CANONICAL}: census 시험 함수를 못 찾았다"));
    let body = &src[at..];
    let end = body
        .find("\n}")
        .unwrap_or_else(|| panic!("{CANONICAL}: census 시험 함수의 끝을 못 찾았다"));
    let body = &body[..end];

    let mut tier = |needle: &str| -> u32 {
        let hit = body.find(needle).unwrap_or_else(|| {
            panic!(
                "{CANONICAL}: `{needle}` 을 못 찾았다 — 정본의 형태가 \
                                       바뀌었으면 이 판정기를 함께 고쳐라"
            )
        });
        let rest = &body[hit + needle.len()..];
        let digit = rest
            .find(|c: char| c.is_ascii_digit())
            .unwrap_or_else(|| panic!("{CANONICAL}: `{needle}` 뒤에 수가 없다"));
        number_at(rest, digit)
            .unwrap_or_else(|| panic!("{CANONICAL}: `{needle}` 뒤의 수를 못 읽었다"))
            .0
    };
    let p = tier("dtcg::Tier::Primitive)");
    let s = tier("dtcg::Tier::Semantic)");
    let c = tier("dtcg::Tier::Component)");
    let total = tier("set.len(),");
    (p, s, c, total)
}

/// 정본이 실제로 읽혔는가 — 이것이 먼저 통과해야 아래 대조의 초록이 뜻을 갖는다.
#[test]
fn the_canonical_census_is_actually_read() {
    let (p, s, c, total) = canonical_census();
    for (what, n) in [
        ("primitive", p),
        ("semantic", s),
        ("component", c),
        ("합계", total),
    ] {
        assert!(
            n > 0,
            "정본에서 {what} census 를 {n} 으로 읽었다 — 0 이면 아래 대조가 안 보고 초록이 된다"
        );
    }
    assert_eq!(
        p + s + c,
        total,
        "정본 자신의 티어 합({p}+{s}+{c})이 합계({total})와 다르다 — 정본이 먼저 어긋났다"
    );
}

/// 명부의 자리가 실재하고, 정본과 같은 수를 적는가.
#[test]
fn every_prose_copy_states_the_canonical_census() {
    let (p, s, c, total) = canonical_census();
    let mut problems: Vec<String> = Vec::new();

    for (path, needle) in CLAIMS {
        let src = read(path);
        let Some(hit) = src.find(needle) else {
            problems.push(format!(
                "{path}: 고정 문구 `{needle}` 이 없다 — 그 문장이 바뀌었으면 이 명부도 \
                 함께 고쳐라(못 찾은 채 넘어가면 그 자리는 영구히 안 보인다)"
            ));
            continue;
        };
        let rest = &src[hit + needle.len()..];
        let Some((claimed, after)) = number_at(rest, 0) else {
            problems.push(format!("{path}: `{needle}` 바로 뒤에 수가 없다"));
            continue;
        };
        if claimed != total {
            problems.push(format!(
                "{path}: 합계를 {claimed} 로 적는데 정본은 {total} 이다"
            ));
        }
        // 삼중항이 이어지면 그것도 본다 — README 가 `818 = 121/143/554` 꼴로 적는다.
        let tail = &rest[after..];
        if let Some(eq) = tail.find('=')
            && tail[..eq].trim().is_empty()
        {
            let triple = tail[eq + 1..].trim_start();
            let got: Vec<u32> = triple
                .split('/')
                .take(3)
                .filter_map(|part| number_at(part.trim_start(), 0).map(|(n, _)| n))
                .collect();
            if got != vec![p, s, c] {
                problems.push(format!(
                    "{path}: 티어 내역을 {got:?} 로 적는데 정본은 [{p}, {s}, {c}] 다"
                ));
            }
        }
    }

    assert!(
        problems.is_empty(),
        "토큰 census 의 산문 사본이 정본(`{CANONICAL}`)과 갈렸다. 정본이 맞으면 산문을 \
         고치고, 산문이 맞으면 정본이 먼저 틀린 것이므로 vendor 갱신부터 다시 본다:\n  {}",
        problems.join("\n  ")
    );
}

/// 등록 안 된 census 서술이 그 크레이트에 새로 생기지 않았는가.
#[test]
fn no_unregistered_census_phrase_lives_in_the_crate() {
    let registered: Vec<&str> = CLAIMS.iter().map(|(p, _)| *p).collect();
    let excluded: Vec<&str> = EXCLUDED.iter().map(|(p, _)| *p).collect();
    for (path, why) in EXCLUDED {
        assert!(!why.trim().is_empty(), "{path} 의 제외에 사유가 없다");
        assert!(
            repo_root().join(path).exists(),
            "{path} 가 없다 — 제외 명부에 죽은 자리가 남았다"
        );
    }

    let (_, _, _, total) = canonical_census();
    let needle = format!("총 {total} 토큰");
    let mut unregistered: Vec<String> = Vec::new();

    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(["ls-files", CRATE_DIR])
        .output()
        .expect("git ls-files 를 못 돌렸다");
    assert!(out.status.success(), "git ls-files 가 실패했다");
    let listed = String::from_utf8_lossy(&out.stdout);
    let files: Vec<&str> = listed.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        files.len() >= 5,
        "{CRATE_DIR} 에서 {} 개만 봤다 — 순회가 죽었으면 아래는 안 봐서 나온 초록이다",
        files.len()
    );

    for rel in files {
        if registered.contains(&rel) || excluded.contains(&rel) {
            continue;
        }
        if !(rel.ends_with(".rs") || rel.ends_with(".md")) {
            continue;
        }
        let src = read(rel);
        if src.contains("census") || src.contains(&needle) {
            unregistered.push(rel.to_string());
        }
    }

    assert!(
        unregistered.is_empty(),
        "census 를 서술하는 자리가 명부 밖에 있다: {unregistered:?}. 그 수를 주장하면 \
         `CLAIMS` 에, 주장이 아니면 사유와 함께 `EXCLUDED` 에 넣어라 — 어느 쪽도 아니면 \
         그 자리는 정본이 움직여도 안 따라온다"
    );
}
