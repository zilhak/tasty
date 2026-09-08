//! 하한이 적은 **잰 트리** 좌표가 영구한 이름인지 본다.
//!
//! 회차 94 는 규율대로 값 옆에 잰 트리를 적었다. 그런데 회차가 **체리픽으로 착지**하므로
//! lane tip 은 `main` 의 조상이 절대 안 되고, 워크트리를 다음 기점으로 옮기는 순간 그
//! 해시는 어느 ref 로도 안 잡힌다. 실측 2026-09-08: 그렇게 적힌 lane tip 다섯이 전부
//! 도달 불가였다. 규율은 옳았고 어긋난 것은 **그 좌표가 영구하다는 가정**이다 — merge 로
//! 착지하면 참이고 체리픽으로 착지하면 거짓이다.
//!
//! ## 여기서 판정하는 것과 안 하는 것
//!
//! 판정하는 것은 **임시 좌표를 적은 자리의 수**다. 그 수는 소스만 읽으면 나오고, 늘면
//! 누군가 규약을 안 지킨 것이다.
//!
//! ★ **도달 가능성 자체는 안 잰다 — ADR-0243 의 (ㄷ) 채널 칸이다.** 그것을 재려면
//! `git merge-base` 가 필요한데, 이 레포의 CI 체크아웃 열하나 중 **열이 얕다**(깊이를
//! 명시한 워크플로가 하나뿐이다). 얕은 체크아웃에는 그 커밋이 아예 없으므로 판정이 항상
//! "판정 불가" 로 끝나고, 그런 자리는 안 걸리는 술어가 된다.
//!
//! **되돌아올 조건**: 깊이를 다 받는 워크플로가 **둘 이상**이 되면 그때 도달 가능성을
//! 직접 잰다. 그 전까지 이 파일이 잡는 것은 수뿐이고, **`Tree` 로 적힌 해시가 실제로
//! 도달 가능한지는 아무도 안 본다** — 그 자리를 알고 읽어라.

use std::path::Path;
use tasty_doc_guards::floored_walk::{
    CountedOn, Descend, Floor, Walked, populations, walk_with_floor,
};

/// 레포 전체 `.rs` 순회의 하한. 이 판정은 소스를 전수로 읽어야 뜻이 있고, 순회가 절반만
///모으면 **임시 좌표를 적은 자리를 못 보고 초록**이 된다.
const SOURCE_FLOOR: Floor = Floor {
    min: 1232,
    measured: 1343,
    measured_on: "2026-09-08",
    counted_on: CountedOn::Tree("12bc0f4b2 + 이 커밋"),
    why_this_gap: "이 모수는 레포 전체 `.rs` 다. 감소 진폭은 **안 쟀다** — 그래서 곱수를 안 \
                   건다: 안 잰 수를 곱하면 그 곱이 실측으로 읽힌다. 여유는 사건 하나분이다 \
                   — 크레이트 하나가 통째로 접힐 때 빠지는 `.rs` 의 최대가 지금 트리에서 \
                   111(`tasty-gallery`)이고, 여유 111 이 그것이다. 그 다음이 97 \
                   (`tasty-doc-guards`)인데 그것이 접히는 것은 크레이트 하나가 접히는 \
                   사건이 아니라 이 가드 자신이 사라지는 일이라 곱수에 안 넣는다",
};

/// 임시 좌표(`LaneTip`)를 적은 자리의 수. **양방향 래칫**이다 — 늘면 규약을 어긴 것이고,
/// 줄면 그 자리가 다시 재어진 것이니 이 수를 함께 내려라. 남는 여유는 안 보는 구간이다.
const LANE_TIP_SITES: usize = 4;

/// 갈래 이름을 **조각으로 조립한다.** 원문 형태로 적으면 이 파일 자신이 좌변에 앉아
/// 자기가 세는 수를 움직인다 — 같은 형태의 사고가 이 레포에서 두 번 났다.
fn marker(variant: &str) -> String {
    format!("Counted{}::{}", "On", variant)
}

fn sources(root: &Path) -> Vec<Walked> {
    walk_with_floor(
        root,
        root,
        &SOURCE_FLOOR,
        Descend::SkipBuildCachesAndDotDirs,
        &|w: &Walked| w.rel.ends_with(".rs"),
    )
    .unwrap_or_else(|why| panic!("{why}"))
}

/// 갈래별 자리 수. **하한 순회 라이브러리 자신은 뺀다** — 거기에는 갈래의 정의와 그것을
/// 가르는 `match` 가 있고, 그 둘은 소비자가 아니다. 그리고 문서 주석 줄도 뺀다.
fn sites(root: &Path, variant: &str) -> Vec<String> {
    let needle = marker(variant);
    let mut out = Vec::new();
    for w in sources(root) {
        if w.rel.ends_with("src/floored_walk.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&w.path) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("///") || line.trim_start().starts_with("//!") {
                continue;
            }
            for _ in 0..line.matches(&needle).count() {
                out.push(format!("{}:{}", w.rel, i + 1));
            }
        }
    }
    out
}

#[test]
fn temporary_coordinates_do_not_spread() {
    let root = &tasty_doc_guards::repo_root();
    let found = sites(root, "LaneTip");
    assert_eq!(
        found.len(),
        LANE_TIP_SITES,
        "임시 좌표를 적은 자리가 {} 개다(고정 {LANE_TIP_SITES}).\n{}\n\
         늘었으면 규약을 어긴 것이다 — 값 옆에 적을 좌표는 **회차 기점의 base 해시**이고, \
         내 커밋이 얹힌 뒤의 값이면 `base + 내 커밋 N 개` 로 적는다. lane tip 은 체리픽 \
         착지로 사라지므로 영구한 이름이 아니다.\n\
         줄었으면 그 자리가 다시 재어진 것이니 위 고정값을 함께 내려라 — 남는 여유는 \
         아무도 안 보는 구간이다.\n\
         ★ 이 수를 올려서 통과시키지 마라.",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn the_census_reads_real_sites_not_prose() {
    let root = &tasty_doc_guards::repo_root();
    // 좌변이 0 이면 위 단정은 "고정값과 같은가" 만 보고 통과할 수 있다. 갈래 넷 중 셋이
    // 실재해야 이 census 가 소스를 실제로 읽은 것이다.
    for variant in ["Tree", "SyntheticTree", "NEVER_COUNTED"] {
        assert!(
            !sites(root, variant).is_empty(),
            "`{variant}` 자리가 하나도 안 잡혔다 — 세는 쪽이 고장났거나 갈래 이름이 바뀌었다. \
             이 상태에서 위 단정의 초록은 '임시 좌표가 안 늘었다' 가 아니라 '아무것도 안 \
             봤다' 는 뜻이다."
        );
    }
    // 이 파일 자신의 산문은 안 세어야 한다 — 위 모듈 주석이 갈래 이름을 여러 번 말한다.
    let mine: Vec<_> = sites(root, "LaneTip")
        .into_iter()
        .filter(|s| s.contains("floor_coordinates_are_permanent.rs"))
        .collect();
    assert!(
        mine.is_empty(),
        "이 파일 자신이 좌변에 앉았다: {mine:?}. 세는 문장이 세는 수를 움직이면 그 수는 \
         모수가 아니라 이 파일의 길이가 된다."
    );
}

#[test]
fn the_population_that_this_file_moves_is_current() {
    // 이 파일이 생기면서 크레이트 통합 타깃이 하나 늘었다. 그 사실을 여기서 단정하는 이유는,
    // 하한을 세우는 파일이 자기가 움직인 모수를 안 갱신하면 그 다음 사람이 낡은 값을 실측으로
    // 읽기 때문이다.
    let root = &tasty_doc_guards::repo_root();
    let mut n = 0;
    for w in sources(root) {
        let parts: Vec<&str> = w.rel.split('/').collect();
        if parts.len() == 4 && parts[0] == "crates" && parts[2] == "tests" {
            n += 1;
        }
    }
    assert_eq!(
        n,
        populations::CRATE_TEST_TARGETS.measured,
        "`crates/*/tests/*.rs` 가 {} 개인데 선언은 {} 이다. 이 파일이 그 모수를 움직였다면 \
         같은 커밋에서 갱신해라 — 값과 잰 트리를 함께.",
        n,
        populations::CRATE_TEST_TARGETS.measured
    );
}
