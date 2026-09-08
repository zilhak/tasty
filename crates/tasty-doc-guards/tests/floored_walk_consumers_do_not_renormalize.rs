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
    min: 1000,
    measured: 1339,
    measured_on: "2026-09-08",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::Tree("b134d28e3"),
    why_this_gap: "이 모수는 저장소 전체의 `.rs` 파일 수다 — 점으로 시작하는 디렉토리와 \
                   `CACHEDIR.TAG` 가 있는 디렉토리 아래는 빼고 센 값이고, 그 술어를 안 \
                   적으면 다음 사람이 다른 수를 얻는다. 2026-09-08 에 `b134d28e3` 에서 \
                   1339 였다. 값의 계보: 2026-09-06 에 이 순회가 1286 을 냈다. 같은 날 \
                   `find . -name '*.rs' -not -path './target/*'` 는 1289 를 내는데, 차 3 은 \
                   `site/target/` 안의 것이다 — 이름이 아니라 `CACHEDIR.TAG` 로 가지치는 \
                   순회는 최상위가 아닌 빌드 디렉토리도 건너뛴다. 그 뒤 한 번 1399 로 \
                   적혔는데 그것은 **안 잰 값**이었다: 그날 이 좌변은 1321..1339 였고 \
                   1399 는 이 레포에서 이 술어로 나온 적이 없다. 여유를 339 로 좁힌 근거: \
                   1215 커밋(2026-09-05~09-08)에서 이 모수는 1229 에서 1339 로 **단조 \
                   증가**했고 한 커밋 최대 이동이 3 이었다. 여유 599 는 움직임을 견디는 \
                   폭이 아니라 안 보는 구간이었다 — 절반이 사라져도 초록이다. 미추적 \
                   기여분은 0 이다(같은 날 작업 트리를 실제로 순회한 값과 추적 트리 \
                   계수가 같았다). 그 확인이 필요한 이유는 커밋되지 않는 로컬 작업 폴더가 \
                   clone·CI 에는 없고 개발자의 트리에는 있어서, 그것을 세면 같은 커밋이 \
                   기계마다 다른 수를 내기 때문이다.",
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

/// 판정에 들어가는 한 파일 — repo-relative 경로와 그 **원문**.
///
/// 디스크에서 읽어도 되고 손으로 지어도 된다. 짝으로 만든 이유가 그것이다: 아래 훑기는
/// 파일이 실재하는지 안 묻는다. 그래서 합성 말뭉치가 갈래를 태울 수 있고, 여기 갈래는
/// 실물 트리에서 안 태워진다 — 이 저장소에는 지금 위반이 0 이라 위반 갈래가 한 번도
/// 안 돌고, 명부 면제 갈래는 두 줄뿐인 명부가 늘 참이라 갈라지지 않는다.
type Source = (String, String);

/// 순회 결과를 짝으로 읽는다. 읽기에 실패한 것은 좌변에서 뺀다 — 여기서 세지 않은 것은
/// 아래 계수에도 안 들어가므로 그 사실이 `consumers` 로 드러난다.
fn read_sources(files: &[Walked]) -> Vec<Source> {
    let mut out = Vec::new();
    for file in files {
        let Ok(src) = std::fs::read_to_string(&file.path) else {
            continue;
        };
        out.push((file.rel.clone(), src));
    }
    out
}

/// 훑은 결과. 위반 목록만 내면 "위반 0" 과 "아무것도 안 봤다" 가 같은 값이 된다.
struct Scanned {
    consumers: usize,
    offenders: Vec<String>,
}

/// 명부는 **인자**다. 이 판정이 지키는 성질은 명부의 내용이 아니라 "명부에 없으면
/// 위반" 이라는 모양이고, 그것은 명부와 무관하게 성립해야 한다. 상수를 직접 읽으면
/// 그 모양을 시험하는 픽스처가 명부 두 줄에 매여 자기 자신을 지키게 된다.
fn scan(sources: &[Source], normalizers: &[(&str, &str)]) -> Scanned {
    let mut consumers = 0usize;
    let mut offenders = Vec::new();
    for (rel, src) in sources {
        if normalizers.iter().any(|(path, _)| *path == rel.as_str()) {
            continue;
        }
        let code = tasty_doc_guards::strip_line_comments(src);
        if !is_consumer(&code) {
            continue;
        }
        consumers += 1;
        if renormalizes(&code) {
            offenders.push(rel.clone());
        }
    }
    offenders.sort();
    Scanned {
        consumers,
        offenders,
    }
}

#[test]
fn no_consumer_of_the_shared_walk_normalizes_paths_itself() {
    let root = tasty_doc_guards::repo_root();
    let files = walk_with_floor(
        &root,
        &root,
        &SCAN_FLOOR,
        Descend::SkipBuildCachesAndDotDirs,
        &is_rust_source,
    )
    .unwrap_or_else(|why| panic!("{why}"));

    let scanned = scan(&read_sources(&files), NORMALIZERS);

    // 부정 단정은 혼자 서면 안 된다. 소비자를 하나도 못 찾았다면 아래의 "위반 0" 은
    // 위반이 없다는 뜻이 아니라 아무것도 안 봤다는 뜻이다.
    assert!(
        scanned.consumers > 0,
        "공용 순회를 쓰는 자리를 하나도 못 찾았다({} 개 파일을 훑었다) — 판정이 죽었다. \
         모듈 이름이 바뀌었으면 `is_consumer` 를 따라 고쳐라.",
        files.len()
    );

    // 이 순회도 레포 루트에서 시작한다 — 점 없는 이름의 미추적 폴더 아래 `.rs` 는 그대로
    // 좌변에 들어온다(실측 2026-09-08). 아래 처방이 `NORMALIZERS` 등록이라 그것이 레포 밖
    // 경로에 붙으면 실재하지 않는 위반이 영구 면제된다 — 실패할 때만 출신을 묻는다.
    // 좌변을 git 으로 안 바꾼 근거는 [`tasty_doc_guards::tracked_scope`] 에 있다.
    let outside = if scanned.offenders.is_empty() {
        String::new()
    } else {
        tasty_doc_guards::tracked_scope::outside_repo_note(&root, &scanned.offenders)
    };
    assert!(
        scanned.offenders.is_empty(),
        "공용 순회를 쓰면서 경로 정규화를 자기 손으로 하는 자리다 ({} 곳):\n  {}\n\n\
         `walk_with_floor` 는 정규화된 repo-relative 를 `Walked::rel` 에 담아 낸다 — \
         그것을 써라. 순회를 안 거치는 자리(디렉토리 하나, 경로 하나)라면 \
         `floored_walk::normalized_rel` 을 불러라.\n\
         ★ 여기 한 줄을 더해서 통과시키지 마라. 정규화하는 자리가 둘이 되는 순간 \
         언젠가 한쪽이 빠뜨리고, 빠뜨린 쪽은 빨강이 아니라 조용한 0 을 낸다 — \
         명부 조회가 전부 빗나가고 가드는 \"명부에 없다\" 고 보고한다. \
         정말 정본이 하나 더 필요하면 `NORMALIZERS` 에 사유와 함께 등록하고, \
         그 사유는 왜 한 곳으로 못 모으는지를 말해야 한다.{}",
        scanned.offenders.len(),
        scanned.offenders.join("\n  "),
        outside
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

/// 하한 선언이 **순회에 실제로 넘어가는가.**
///
/// 회차 94 에 하한 하나(`guard_test_channels_stay_split` 의 잡 하한)가 없어졌다. 값이
/// 낡아서가 아니라 **겨냥이 어긋나서**였다 — 그 하한의 모수는 레포 전체의 자동 잡인데
/// 그 파일이 판정하는 것은 이름 붙은 두 채널이었고, 그래서 하한이 홀로 잡는 진짜 결함은
/// 없고 홀로 내는 거짓 경보는 있었다. 근본 원인은 그 상수가 **순회 함수에 한 번도 안
/// 넘어간 것**이다. 넘어가지 않으면 `Floor::validate` 조차 안 돌고, 그 구조체는 하한이
/// 아니라 필드 둘을 읽는 상수가 된다 — 그중 하나(`measured`)는 실패문에만 쓰여 낡아도
/// 초록이고 고쳐도 초록이었다.
///
/// 그래서 여기서 묻는 것은 값이 아니라 **연결**이다. 하한이 순회에 넘어가면 세는 것과
/// 판정하는 것이 같은 순회에서 나오므로 겨냥이 자동으로 맞는다.
///
/// 실측 2026-09-08(`12bc0f4b2` + 이 회차의 내 커밋): 선언 28 중 안 넘어가는 것 **0**.
/// 그 형태는 회차 94 에 한 자리 있었고 그 자리가 없어지며 닫혔다 — 이 시험은 그것이
/// 다시 열리는 것을 막는다.
///
/// ★ **이 시험이 답하지 않는 것.** "넘어간다" 는 겨냥이 맞다는 필요조건이지 충분조건이
/// 아니다. 넘어가되 그 결과의 일부만 판정하는 자리는 여전히 하한 쪽이 넓을 수 있고,
/// 그것은 자리마다 배타적 변이로만 갈린다. 그 판정은 여기서 **안 한다** — ADR-0243 의
/// (ㄷ) 채널 칸이고, 되돌아올 조건은 그 변이를 자동으로 만드는 자리가 생길 때다.
#[test]
fn every_floor_declaration_reaches_a_walk() {
    let root = &tasty_doc_guards::repo_root();
    // 선언 형태를 원문으로 적으면 이 파일이 자기가 세는 좌변에 앉는다. 조각으로 조립한다.
    let head = "const ";
    let mid = ": Floor = Floor {";
    let mut orphans = Vec::new();
    let mut total = 0usize;
    for w in walk_with_floor(
        root,
        root,
        &SCAN_FLOOR,
        Descend::SkipBuildCachesAndDotDirs,
        &is_rust_source,
    )
    .unwrap_or_else(|why| panic!("{why}"))
    {
        // 하한 순회 라이브러리 자신에는 타입 정의와 그 시험의 픽스처가 있다.
        if w.rel.ends_with("src/floored_walk.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&w.path) else {
            continue;
        };
        let code: String = text
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                !t.starts_with("///") && !t.starts_with("//!")
            })
            .collect::<Vec<_>>()
            .join("\n");
        for line in code.lines() {
            let Some(rest) = line.trim_start().strip_prefix(head) else {
                continue;
            };
            let Some(name) = rest.split(mid).next().filter(|_| rest.contains(mid)) else {
                continue;
            };
            total += 1;
            if !code.contains(&format!("&{name}")) {
                orphans.push(format!("{}::{name}", w.rel));
            }
        }
    }
    assert!(
        total >= 20,
        "하한 선언이 {total} 개만 잡혔다 — 세는 쪽이 고장났다. 이 수가 작으면 아래 초록은 \
         '고아가 없다' 가 아니라 '아무것도 안 봤다' 는 뜻이다."
    );
    assert!(
        orphans.is_empty(),
        "순회에 한 번도 안 넘어가는 하한이 있다: {orphans:?}.\n\
         그 상수는 하한이 아니라 필드 둘을 읽는 값이고, `Floor::validate` 조차 안 돈다. \
         `measured` 는 실패문에만 쓰여 낡아도 초록이고 고쳐도 초록이 된다.\n\
         둘 중 하나를 해라. (1) 그 하한을 실제 순회에 넘겨 세는 것과 판정하는 것을 같은 \
         순회에서 나오게 한다. (2) 겨냥이 애초에 어긋난 것이면 하한을 없애고 그 자리에 \
         0 과 1 만 가르는 갈래 확인을 둔다 — 회차 94 에 그렇게 닫은 자리가 하나 있다.\n\
         ★ 이 목록에 예외를 더해서 통과시키지 마라."
    );
}

/// 합성 말뭉치. 디스크를 안 탄다 — 경로는 실재하지 않는 `zone/` 접두를 쓴다.
///
/// 실재하는 경로를 쓰면 이 파일이 저장소의 지금 내용에 매여, 그 파일이 옮겨지거나
/// 내용이 바뀌는 것만으로 아래 갈래의 답이 달라진다.
fn corpus(pairs: &[(&str, &str)]) -> Vec<Source> {
    pairs
        .iter()
        .map(|(rel, src)| ((*rel).to_string(), (*src).to_string()))
        .collect()
}

/// 명부에 든 자리는 위반으로도 안 세고 **소비자로도 안 센다.**
///
/// 두 방향을 한 시험에서 본다. 위반만 보면 명부가 `offenders.retain` 처럼 뒤에서 걸러도
/// 통과하는데, 그러면 면제된 자리가 `consumers` 를 부풀려 "판정이 죽었다" 단정을 조용히
/// 살려낸다 — 소비자가 명부밖에 없는 트리에서 그 단정은 아무것도 안 지킨다.
#[test]
fn a_roster_line_removes_a_place_from_both_counts() {
    let roster = &[("zone/canon.rs", "합성 정본. 이 시험 안에서만 쓰인다.")];
    let listed = corpus(&[(
        "zone/canon.rs",
        r#"use zonebox::floored_walk; let rel = p.to_string_lossy().replace('\\', "/");"#,
    )]);
    let same_but_unlisted = corpus(&[(
        "zone/other.rs",
        r#"use zonebox::floored_walk; let rel = p.to_string_lossy().replace('\\', "/");"#,
    )]);

    let exempt = scan(&listed, roster);
    assert_eq!(exempt.offenders, Vec::<String>::new(), "명부를 안 본다");
    assert_eq!(exempt.consumers, 0, "면제된 자리를 소비자로 센다");

    let caught = scan(&same_but_unlisted, roster);
    assert_eq!(caught.offenders, vec!["zone/other.rs".to_string()]);
    assert_eq!(caught.consumers, 1);
}

/// 소비자가 아닌 자리의 정규화는 위반이 아니다.
///
/// 이 판정의 물음은 "정규화하는가" 가 아니라 "**공용 순회를 쓰면서** 정규화하는가" 다.
/// 공용 순회를 안 쓰는 자리는 펼 경로를 자기가 만들었으므로 자기가 펴는 것이 맞다.
#[test]
fn the_consumer_gate_is_what_makes_a_normalization_a_violation() {
    let roster: &[(&str, &str)] = &[];
    let both = corpus(&[
        (
            "zone/uses.rs",
            r#"use zonebox::floored_walk::walk_with_floor; let s = raw.replace("\\", "/");"#,
        ),
        (
            "zone/standalone.rs",
            r#"let s = raw.replace("\\", "/"); // 공용 순회를 안 쓴다"#,
        ),
    ]);

    let scanned = scan(&both, roster);
    assert_eq!(scanned.offenders, vec!["zone/uses.rs".to_string()]);
    assert_eq!(scanned.consumers, 1, "소비자가 아닌 자리까지 소비자로 센다");
}

/// 주석 안의 언급은 양쪽 다 안 센다 — 소비자 판정에서도, 위반 판정에서도.
///
/// 마스킹이 빠지면 두 오진이 함께 난다: 이 가드를 **설명하는** 문서 주석이 그 파일을
/// 소비자로 만들고, 처방을 인용한 주석이 그 자리를 위반으로 만든다. 그 둘은 실재하지
/// 않는 위반이라 처방(`NORMALIZERS` 등록)을 따르면 그 자리가 영구 면제된다.
#[test]
fn a_mention_inside_a_comment_is_not_a_use() {
    let roster: &[(&str, &str)] = &[];

    let mentions_only = corpus(&[(
        "zone/prose.rs",
        "// floored_walk 을 쓰는 자리는 정규화를 하지 않는다\nfn f() {}",
    )]);
    assert_eq!(
        scan(&mentions_only, roster).consumers,
        0,
        "주석의 언급을 사용으로 센다"
    );

    let quotes_the_fix = corpus(&[(
        "zone/quoted.rs",
        "use zonebox::floored_walk;\n// 예전엔 replace('\\\\', \"/\") 를 손으로 붙였다\nfn f() {}",
    )]);
    let scanned = scan(&quotes_the_fix, roster);
    assert_eq!(scanned.consumers, 1, "소비자를 못 알아본다");
    assert_eq!(
        scanned.offenders,
        Vec::<String>::new(),
        "주석에 인용된 처방을 위반으로 센다"
    );
}

/// 나오는 목록은 정렬돼 있다 — 입력 순서와 무관하게.
///
/// 실패문에 그대로 실리는 값이라, 순서가 순회 순서를 따라가면 같은 트리의 같은 위반이
/// 기계마다 다른 줄로 나온다.
#[test]
fn the_offender_list_does_not_carry_the_input_order() {
    let roster: &[(&str, &str)] = &[];
    let hit = r#"use zonebox::floored_walk; let s = raw.replace("\\", "/");"#;
    let scanned = scan(
        &corpus(&[("zone/c.rs", hit), ("zone/a.rs", hit), ("zone/b.rs", hit)]),
        roster,
    );
    assert_eq!(
        scanned.offenders,
        vec![
            "zone/a.rs".to_string(),
            "zone/b.rs".to_string(),
            "zone/c.rs".to_string()
        ]
    );
}

/// 소비자가 0 인 말뭉치에서 위반도 0 이다 — 그리고 그 0 은 다른 뜻이다.
///
/// 이 짝이 `consumers > 0` 단정이 존재하는 이유다. 두 값이 함께 0 으로 나오는 상태가
/// 실재하고, 위반만 보면 그 상태가 초록으로 보인다.
#[test]
fn no_consumers_and_no_violations_are_two_different_zeros() {
    let roster: &[(&str, &str)] = &[];

    let empty = scan(&corpus(&[]), roster);
    assert_eq!((empty.consumers, empty.offenders.len()), (0, 0));

    let nothing_relevant = scan(
        &corpus(&[("zone/plain.rs", "fn f() -> usize { 1 }")]),
        roster,
    );
    assert_eq!(
        (nothing_relevant.consumers, nothing_relevant.offenders.len()),
        (0, 0)
    );

    let clean_consumer = scan(
        &corpus(&[(
            "zone/clean.rs",
            "use zonebox::floored_walk; let r = w.rel.clone();",
        )]),
        roster,
    );
    assert_eq!(
        (clean_consumer.consumers, clean_consumer.offenders.len()),
        (1, 0),
        "본 자리가 있는데도 소비자 계수가 0 이면 두 0 이 안 갈린다"
    );
}
