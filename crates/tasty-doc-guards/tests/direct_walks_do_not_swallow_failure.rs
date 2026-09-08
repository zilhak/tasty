//! 통합 테스트 타깃이 **직접 순회하면서 읽기 실패를 삼키는** 자리를 센다.
//!
//! ## 왜 공용 순회 래칫으로는 안 보이는가
//!
//! `scripts/check-shared-walk-ratchet.sh` 는 같은 좌변에서 직접 순회 자리를 세는데,
//! 그 수는 **철자**를 센다 — 그 자리가 실패를 삼키는지 안 삼키는지를 안 본다. 실측
//! 2026-09-08(기점 `12bc0f4b2` + 이 lane 의 커밋 2 개): 세 가이드 리더의 `read_dir` 를
//! `panic` 으로 바꾸자 실패 처리 분포가 **38/17/1 → 35/20/1** 로 움직였는데 그 게이트의
//! 총합은 **56 에서 한 칸도 안 움직였다.**
//!
//! 그 눈멂은 대칭이다. 자리가 삼킴 → 안삼킴으로 가는 것(개선)이 안 보이는 것과 반대로
//! 가는 것(회귀)이 안 보이는 것이 **같은 눈멂**이다. 수를 세는 판정기는 구성원이 편을
//! 바꾸는 것을 못 본다.
//!
//! ## 왜 이름 명부가 아니라 수인가
//!
//! 같은 눈멂을 CI 잡 명부에서 찾은 자리는 처방이 **이름 집합 못박기**였다(자동 19 ·
//! 수동 전용 5). 여기서는 그 처방이 안 선다 — 비용이 자릿수로 다르다. 최근 300 커밋에서
//! 두 좌변이 움직인 횟수를 쟀다:
//!
//! | 좌변 | 추가 | 삭제 | 이름변경 |
//! |---|---|---|---|
//! | 워크플로 파일 (그쪽 좌변) | 11 | 0 | 0 |
//! | 통합 테스트 타깃 (이 좌변) | 154 | 6 | 33 |
//!
//! 이 좌변의 명부는 거의 매 회차 낡는다. 그래서 **성질로 가른 수**를 래칫한다 — 자리가
//! 편을 바꾸면 이 수가 움직이고, 파일이 늘거나 이름이 바뀌는 것으로는 안 움직인다.
//!
//! ## 무엇을 삼킴으로 세는가 (술어를 말로)
//!
//! `read_dir(` 호출 한 자리를 둘러싼 코드에서, 읽기가 **실패했을 때 그 디렉토리를 통째로
//! 건너뛰고 판정을 계속하는가**를 본다.
//!
//! ★ **`let Ok(..) = .. else` 라는 철자로는 못 가른다.** `else` 절이 `return` 인 자리와
//! `panic!` 인 자리가 둘 다 실재하고, 앞은 삼키고 뒤는 안 삼킨다. 철자로 가른 첫 판이
//! 여섯을 삼킴으로 오분류해 이 수를 35 로 냈다 — 실제는 29 다. 그래서 판정은 `else` 의
//! **본문**을 본다. 안 삼키는 표지는 `panic!` · `unwrap` · `expect` 셋이다. `?` 로 올려보내는 자리는 **전파**로 따로 센다 —
//! 그 자리 자신은 안 삼키지만 받는 쪽이 삼킬 수 있어 이 판정의 답이 아니다.
//!
//! 세는 것은 **마스킹한 사본**이다. 이 파일 자신이 그 형태를 문자열로 담고 있어서,
//! 원문에서 세면 이 가드가 자기를 위반으로 센다.
//!
//! ## 이 래칫이 못 보는 것
//!
//! 삼키지 않아도 **짝이 없으면** 부분 소실은 여전히 조용하다 — 이 수는 "실패를 삼키는가"
//! 만 보고 "그 소실을 누가 잡는가" 는 안 본다. 후자는 자리마다 하한·갈래 확인·다른 축이
//! 섞여 있어 정적으로 못 가른다(실측: 하한이 없는데 잡는 자리 둘, 하한이 있는데 못 잡는
//! 자리 하나가 나왔다).

use std::collections::BTreeMap;

use tasty_doc_guards::source_text::{mask_non_code, rust_sources};

/// 삼키는 자리 수의 상한. **양방향 래칫이다** — 늘면 새 자리가 생긴 것이고, 줄면 이
/// 수를 같이 내리라는 뜻이다. 남는 여유는 곧 조용히 안 보는 구간이다.
///
/// 실측 29 (기점 `12bc0f4b2` + 이 lane 의 커밋 2 개, 2026-09-08). 여유 0.
///
/// **좌변 하한을 따로 안 둔다.** 이 상수가 양방향이라 순회가 깨져 좌변이 0 이 되면
/// 삼킴도 0 이 되고 `0 != 29` 로 죽는다. 하한을 얹으면 그 여유가 곧 조용히 안 보는
/// 구간이 되는데, 이 회차에 그 실물을 쟀다 — `no_emoji_in_source` 는 하한 700 에 실측
/// 1294 라 여유 594 였고, 순회에서 447 개(35%)가 사라져도 초록이었다.
const SWALLOW_CAP: usize = 29;

#[derive(PartialEq, Eq, Debug)]
enum Handling {
    Swallow,
    Loud,
    Propagate,
}

fn is_target(rel: &str) -> bool {
    let parts: Vec<&str> = rel.split('/').collect();
    match parts.as_slice() {
        ["tests", f] => f.ends_with(".rs"),
        ["crates", _, "tests", f] => f.ends_with(".rs"),
        _ => false,
    }
}

/// 한 호출 자리의 처리 형태. 창은 앞 1 줄 · 뒤 6 줄 — `unwrap_or_else` 의 `panic!` 이
/// 다음 줄로 넘어가는 형태가 실재해서, 창이 좁으면 그 자리를 **삼킴으로 오분류한다.**
fn handling_at(lines: &[&str], i: usize) -> Handling {
    let lo = i.saturating_sub(1);
    let hi = (i + 7).min(lines.len());
    let ctx = lines[lo..hi].join(" ");
    let loud = ["panic!", ".unwrap()", ".expect("];
    if ctx.contains(").unwrap_or_else(") && ctx.contains("panic!") {
        return Handling::Loud;
    }
    if let Some(rest) = ctx.split_once("read_dir").map(|(_, r)| r) {
        let head: String = rest.chars().take(60).collect();
        if head.contains('?') && !head.contains("else") {
            return Handling::Propagate;
        }
        if loud.iter().any(|m| head.contains(m)) {
            return Handling::Loud;
        }
    }
    Handling::Swallow
}

#[test]
fn direct_walks_do_not_swallow_failure() {
    let root = tasty_doc_guards::repo_root();
    let mut per_file: BTreeMap<String, usize> = BTreeMap::new();
    let (mut sites, mut swallow) = (0usize, 0usize);
    for (path, text) in rust_sources(&root, &["tests", "crates"]) {
        let rel = path.to_string_lossy().replace('\\', "/");
        if !is_target(&rel) {
            continue;
        }
        let masked = mask_non_code(&text);
        let lines: Vec<&str> = masked.split('\n').collect();
        for (i, l) in lines.iter().enumerate() {
            if !l.contains("read_dir") {
                continue;
            }
            sites += 1;
            if handling_at(&lines, i) == Handling::Swallow {
                swallow += 1;
                *per_file.entry(rel.clone()).or_default() += 1;
            }
        }
    }

    let listing: Vec<String> = per_file
        .iter()
        .map(|(f, n)| format!("  {n}  {f}"))
        .collect();
    assert_eq!(
        swallow,
        SWALLOW_CAP,
        "직접 순회 중 읽기 실패를 삼키는 자리가 {swallow} 개다(상한 {SWALLOW_CAP}, \
         전체 {sites}).\n\
         늘었으면: 그 자리는 한 디렉토리를 못 읽어도 그 하위를 통째로 뺀 채 초록을 낸다. \
         `expect` 나 `unwrap_or_else(|e| panic!(..))` 로 바꿔라.\n\
         줄었으면: 이 상수도 같이 내려라 — 남는 여유는 곧 조용히 안 보는 구간이고, \
         공용 순회 래칫의 총합은 이 이동을 원리적으로 못 본다.\n\
         지금 자리:\n{}",
        listing.join("\n")
    );
}
