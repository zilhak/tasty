//! 공용 순회 소비자가 경로 구분자를 다시 정규화하는 알려진 문자열 패턴을 찾는다.
//! Windows 경로를 슬래시 경로와 비교하려면 정규화가 필요하지만 소비자마다 구현하면 규칙이 달라질 수 있다.
//! walk_with_floor의 Walked::rel은 이미 정규화돼 있다. 개별 경로는 normalized_rel을 사용한다.
//! 이 검사는 재정규화 여부만 확인하며 정규화를 아예 생략한 경우나 모든 다른 구현을 검출하지는 못한다.

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

/// 정규화를 구현하거나 검사 문자열을 보유해야 하는 예외와 사유.
const NORMALIZERS: &[(&str, &str)] = &[
    (
        "crates/tasty-doc-guards/src/floored_walk.rs",
        "공용 normalized_rel 구현이 있는 파일이다. 소비자는 여기서 만든 경로를 사용한다.",
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
    counted_on: tasty_doc_guards::floored_walk::CountedOn::Tree(
        "b134d28e3 — 이력 재작성으로 현재 main에서 도달할 수 없는 과거 측정이다.",
    ),
    why_this_gap: "점 디렉터리와 빌드 캐시 표식을 제외한 Rust 파일 수다. 2026-09-08 b134d28e3에서 1339개를 측정했고 미추적 파일 기여는 없었다. 당시 1215커밋에서 1229→1339로 증가하고 한 커밋의 최대 변화는 3이었다. 하한 1000의 여유 339는 일부 누락을 허용하므로 전수 수집의 증거는 아니다. 옛 측정 SHA는 현재 이력에서 도달할 수 없다.",
};

fn is_rust_source(found: &Walked) -> bool {
    found.rel.ends_with(".rs")
}

/// 주석을 제거한 소스에 floored_walk 문자열이 있는지 확인한다.
fn is_consumer(code: &str) -> bool {
    code.contains("floored_walk")
}

fn renormalizes(code: &str) -> bool {
    code.contains(r#"replace('\\', "/")"#) || code.contains(r#"replace("\\", "/")"#)
}

/// 파일 경로와 원문. 디스크 없이 같은 판정을 시험하도록 합성 입력도 받는다.
type Source = (String, String);

/// 읽기 실패한 파일은 제외된다. 소비자 수가 줄어도 0보다 크면 개별 누락은 드러나지 않을 수 있다.
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

struct Scanned {
    consumers: usize,
    offenders: Vec<String>,
}

/// 실제 예외 목록과 무관한 합성 입력으로 판정할 수 있도록 명부를 인자로 받는다.
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

    assert!(
        scanned.consumers > 0,
        "공용 순회 소비자를 찾지 못했다(수집 {}파일). 모듈 이름과 is_consumer의 판독을 확인한다.",
        files.len()
    );

    // 점 없는 미추적 폴더도 수집될 수 있어 오류 위치의 추적 여부를 알린다.
    let outside = if scanned.offenders.is_empty() {
        String::new()
    } else {
        tasty_doc_guards::tracked_scope::outside_repo_note(&root, &scanned.offenders)
    };
    assert!(
        scanned.offenders.is_empty(),
        "공용 순회 소비자가 경로를 다시 정규화한다({}곳):\n  {}\nWalked::rel을 그대로 사용하고 개별 경로는 normalized_rel로 변환한다. 별도 구현이 필요하다면 NORMALIZERS에 한곳으로 합칠 수 없는 이유를 적는다.{}",
        scanned.offenders.len(),
        scanned.offenders.join("\n  "),
        outside
    );
}

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

/// Floor 상수 선언에 대응하는 &이름 문자열이 같은 파일에 있는지 확인한다.
/// 실제 순회 인자로 전달되는지, 하한이 검사 대상에 적합한지는 이 조건만으로 증명할 수 없다.
/// 검증 범위는 docs/dev-guide/guard-verification.md를 따른다.
#[test]
fn every_floor_declaration_reaches_a_walk() {
    let root = &tasty_doc_guards::repo_root();
    // 검사 문자열 자체가 선언으로 수집되지 않도록 조각으로 둔다.
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
        "Floor 선언을 {total}개만 찾았다. 소스 수집과 선언 판독을 확인한다."
    );
    assert!(
        orphans.is_empty(),
        "같은 파일에서 참조 표지를 찾지 못한 Floor 상수다: {orphans:?}. 실제 순회에 전달하는지 확인한다. 검사의 대상과 맞지 않는 기준이라면 단순히 예외로 등록하지 말고 필요한 검증 방식을 다시 정한다."
    );
}

fn corpus(pairs: &[(&str, &str)]) -> Vec<Source> {
    pairs
        .iter()
        .map(|(rel, src)| ((*rel).to_string(), (*src).to_string()))
        .collect()
}

/// 예외를 위반 목록에서만 제거하면 소비자 하한을 부풀릴 수 있어 소비자 수에서도 제외해야 한다.
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

/// 공용 순회 소비자가 아닌 코드의 정규화는 이 검사의 대상이 아니다.
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

/// 파일 순회 순서가 달라도 진단 목록이 같도록 정렬한다.
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
