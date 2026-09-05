//! 공용 순회를 쓰는 자리가 경로 정규화를 **자기 손으로 다시 하지 않는지** 본다.
//!
//! `strip_prefix` 의 결과는 그 플랫폼의 구분자를 그대로 물고 나온다. 그것을
//! `to_string_lossy()` 로 펴서 소스에 박힌 `/` 리터럴과 비교하면 Windows 에서 전부
//! 빗나가는데, **그 어긋남은 예외가 아니라 조용한 0** 이다 — 명부 조회가 모조리 실패하고
//! 가드는 "명부에 없다" 고 보고한다. 실제로 그 형태로 한 번 터졌다.
//!
//! 처방은 소비자마다 `replace` 를 붙이는 것이 아니라 **경로를 만드는 한 곳**에서 펴는
//! 것이다. `tasty_doc_guards::floored_walk::normalized_rel` 이 그 한 곳이고,
//! [`walk_with_floor`](tasty_doc_guards::floored_walk::walk_with_floor) 는 파일마다
//! 그것을 불러 [`Walked::rel`](tasty_doc_guards::floored_walk::Walked) 에 담아 낸다.
//!
//! **이 성질 자체는 Linux 에서 못 잰다** — 고치기 전에도 Linux 는 `/` 를 내므로 Linux
//! 에서 도는 단정은 처방 전후로 똑같이 초록이다. 그것을 재는 채널은 `crossplatform-check`
//! 의 `check-windows` 하나다. 여기서 재는 것은 다른 것이다: **정규화하는 자리가 저장소에
//! 하나로 남아 있는가.** 그건 텍스트로 셀 수 있고, 하나로 남아 있기만 하면 그 하나의
//! 옳고 그름을 저 채널이 답한다.

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

/// 정규화를 해도 되는 자리와 그 이유. 여기 없는 자리가 정규화를 하면 실패한다.
const NORMALIZERS: &[(&str, &str)] = &[
    (
        "crates/tasty-doc-guards/src/floored_walk.rs",
        "정본. `normalized_rel` 이 여기 있고 이 가드가 지키려는 것이 바로 \
         '그 함수가 유일하다' 는 사실이다.",
    ),
    (
        "crates/tasty-doc-guards/tests/floored_walk_consumers_do_not_renormalize.rs",
        "이 가드 자신. 판별 문자열을 담는 것이 본질이라 자기 판정에 걸린다.",
    ),
];

/// 저장소 `.rs` 순회의 하한.
const SCAN_FLOOR: Floor = Floor {
    min: 800,
    measured: 1301,
    measured_on: "2026-09-06",
    why_this_gap: "이 모수는 저장소 전체의 `.rs` 파일 수다. 크레이트가 늘거나 갈리면 \
                   한 번에 수십 개가 움직이고, 순회가 심볼릭 링크를 따라가므로 레포 밖을 \
                   가리키는 링크가 있는 작업 트리에서는 더 세어진다(실측 2026-09-06: \
                   `find` 로는 1288, 이 순회로는 1301) — 그래서 여유를 넓게 둔다. 여기서 잡으려는 사고는 순회가 \
                   통째로 죽는 것이지 파일 수의 변동이 아니다.",
};

fn is_rust_source(found: &Walked) -> bool {
    found.rel.ends_with(".rs")
}

/// 공용 순회를 쓰는 자리인가 — **코드에서** 그 모듈을 언급하는가. 주석의 언급은 세지
/// 않는다. 이 판정의 물음은 "그것을 쓰는가" 이지 "그 낱말이 있는가" 가 아니다.
fn is_consumer(code: &str) -> bool {
    code.contains("floored_walk")
}

/// 경로 구분자를 자기 손으로 펴는가.
fn renormalizes(code: &str) -> bool {
    code.contains(r#"replace('\\', "/")"#) || code.contains(r#"replace("\\", "/")"#)
}

#[test]
fn no_consumer_of_the_shared_walk_normalizes_paths_itself() {
    let root = tasty_doc_guards::repo_root();
    let files = walk_with_floor(
        &root,
        &root,
        &SCAN_FLOOR,
        Descend::SkipBuildCaches,
        &is_rust_source,
    )
    .unwrap_or_else(|why| panic!("{why}"));

    let mut consumers = 0usize;
    let mut offenders = Vec::new();
    for file in &files {
        if NORMALIZERS.iter().any(|(path, _)| *path == file.rel) {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&file.path) else {
            continue;
        };
        let code = tasty_doc_guards::strip_line_comments(&src);
        if !is_consumer(&code) {
            continue;
        }
        consumers += 1;
        if renormalizes(&code) {
            offenders.push(file.rel.clone());
        }
    }

    // 부정 단정은 혼자 서면 안 된다. 소비자를 하나도 못 찾았다면 아래의 "위반 0" 은
    // 위반이 없다는 뜻이 아니라 아무것도 안 봤다는 뜻이다.
    assert!(
        consumers > 0,
        "공용 순회를 쓰는 자리를 하나도 못 찾았다({} 개 파일을 훑었다) — 판정이 죽었다. \
         모듈 이름이 바뀌었으면 `is_consumer` 를 따라 고쳐라.",
        files.len()
    );

    offenders.sort();
    assert!(
        offenders.is_empty(),
        "공용 순회를 쓰면서 경로 정규화를 자기 손으로 하는 자리다 ({} 곳):\n  {}\n\n\
         `walk_with_floor` 는 정규화된 repo-relative 를 `Walked::rel` 에 담아 낸다 — \
         그것을 써라. 순회를 안 거치는 자리(디렉토리 하나, 경로 하나)라면 \
         `floored_walk::normalized_rel` 을 불러라.\n\
         ★ 여기 한 줄을 더해서 통과시키지 마라. 정규화하는 자리가 둘이 되는 순간 \
         언젠가 한쪽이 빠뜨리고, 빠뜨린 쪽은 빨강이 아니라 조용한 0 을 낸다 — \
         명부 조회가 전부 빗나가고 가드는 \"명부에 없다\" 고 보고한다. \
         정말 정본이 하나 더 필요하면 `NORMALIZERS` 에 사유와 함께 등록하고, \
         그 사유는 왜 한 곳으로 못 모으는지를 말해야 한다.",
        offenders.len(),
        offenders.join("\n  ")
    );
}

/// 위 판정이 무엇이든 잡을 수 있는지 같은 함수로 확인한다. 한 방향만 재면 "위반 0" 과
/// "판정이 죽었다" 가 구별되지 않는다.
#[test]
fn the_detector_separates_a_hand_rolled_normalization_from_using_the_shared_one() {
    assert!(
        renormalizes(r#"let rel = p.to_string_lossy().replace('\\', "/");"#),
        "자기 손 정규화를 안 잡는다"
    );
    assert!(
        renormalizes(r#"s.replace("\\", "/")"#),
        "따옴표 형태를 안 잡는다"
    );
    assert!(
        !renormalizes("let rel = found.rel.clone();"),
        "공용 순회의 결과를 쓰는 것을 위반으로 잡는다"
    );
    assert!(
        !renormalizes(r#"s.replace('/', "-")"#),
        "다른 replace 를 정규화로 잡는다"
    );
    assert!(
        is_consumer("use tasty_doc_guards::floored_walk::walk_with_floor;"),
        "소비자를 못 알아본다"
    );
    assert!(
        !is_consumer("use std::fs::read_dir;"),
        "소비자가 아닌 것을 소비자로 센다"
    );
}
