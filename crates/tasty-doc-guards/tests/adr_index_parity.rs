//! ADR 헤더와 생성 인덱스의 번호·내용·배치를 대조한다.
//!
//! 모든 ADR은 정확히 한 그룹에 속하고 생성 행도 한 번만 나와야 한다.
//! 머리말의 번호와 충돌 표지도 검사한다. 생성기는 마커 안만 바꾸므로
//! 마커 밖의 표 행과 머리말 오류는 별도로 확인해야 한다.
//!
//! 현재 문서는 기존 기록을 통합해 새로 썼으므로 대체·개정 사슬이 없을 수 있다.
//! 관계가 있으면 양방향 기록과 대상 존재를 검사하고, 관계 해석의 검출력은
//! 아래 합성 정상·오류 입력으로 확인한다. 과거 사슬 개수를 현재 문서에 요구하지 않는다.
//! Tags의 주제 관계나 편집 판단은 문자열만으로 검증하지 않는다.
//! 작성 규칙: docs/adr/0649-architecture-decision-records.md.

// 이유: 이 타깃은 시험 범위다. `let _` 로 값을 버리는 자리를 여기서 명부에 올리면
//       그 명부가 프로덕션 자리를 가리키는 뜻을 잃는다 —
//       `crates/tasty-doc-guards/tests/let_underscore_documented.rs` 의 명부 순수성 판정이 그것을 막는다.
#![allow(clippy::let_underscore_must_use)]

use std::path::{Path, PathBuf};

use tasty_doc_guards::adr_index::{
    ADR_DIR, Adr, INDEX, chain_violations, collect, conflict_marker_lines, duplicate_numbers,
    group_slugs, incomplete_rows, parse_adr, placement_violations, preamble_violations,
    region_row_count, render_index, stray_table_lines, superseder,
};
use tasty_doc_guards::temp_scratch::Scratch;

/// 통합 트리 e87dcea71에서 ADR 51편을 확인했다. 하한 50은 빈 수집과
/// 대규모 누락을 발견하기 위한 보조 검사다. 개별 누락·중복은 헤더와 생성 행을 대조한다.
/// 문서를 의도적으로 통폐합해 이보다 줄이면 실제 파일 목록을 다시 확인해 갱신한다.
const MIN_ADRS: usize = 50;

/// 여섯 주제 머리말에 대표 ADR을 하나씩 연결한다. 없는 번호의 검출은
/// 합성 입력으로도 확인하므로 과거 머리말의 긴 번호 목록을 유지할 필요는 없다.
const MIN_PREAMBLE_NUMBERS: usize = 6;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn adrs(root: &Path) -> Vec<Adr> {
    collect(root).unwrap_or_else(|e| panic!("{e}"))
}

fn index_text(root: &Path) -> String {
    std::fs::read_to_string(root.join(INDEX))
        .unwrap_or_else(|e| panic!("{INDEX} 를 못 읽었다: {e}"))
        .replace("\r\n", "\n")
}

/// 한 번호는 한 ADR 만 가리킨다.
#[test]
fn an_adr_number_names_exactly_one_document() {
    let all = adrs(&repo_root());
    println!("[ADR 인덱스] ADR 파일 {} · 하한 {MIN_ADRS}", all.len());
    assert!(
        all.len() >= MIN_ADRS,
        "ADR 파일을 {} 개밖에 못 셌다(하한 {MIN_ADRS}) — 수집이 죽었다",
        all.len()
    );
    let dupes = duplicate_numbers(&all);
    assert!(
        dupes.is_empty(),
        "같은 ADR 번호가 서로 다른 문서를 가리킨다. 번호는 소스 주석·다른 ADR·커밋 \
         메시지가 인용하는 식별자라, 겹치면 그 인용이 전부 모호해진다. 나중에 얹은 쪽이 \
         현재 최대 번호 + 1 로 옮긴다(파일명·본문 제목·참조 링크 셋 다, 그리고 인덱스를 \
         다시 생성한다). 빈 번호는 재사용하지 않는다\
         (docs/adr/0649-architecture-decision-records.md).\n  {}",
        dupes.join("\n  ")
    );
}

/// 문서 안의 `# ADR-NNNN` 제목이 자기 파일명 번호와 같다. 번호를 옮길 때 파일명만 바꾸고
/// 본문을 안 고치면 문서를 **열어서** 번호를 읽은 사람만 틀린 값을 갖는다.
#[test]
fn the_heading_number_matches_the_file_name() {
    let mut wrong = Vec::new();
    for a in adrs(&repo_root()) {
        match &a.heading_num {
            None => wrong.push(format!("{} — `# ADR-…` 제목 줄이 없다", a.file)),
            Some(h) if *h != a.num => {
                wrong.push(format!("{} — 제목은 ADR-{h} 라고 말한다", a.file))
            }
            Some(_) => {}
        }
    }
    assert!(
        wrong.is_empty(),
        "파일명 번호와 본문 제목 번호가 다르다:\n  {}",
        wrong.join("\n  ")
    );
}

/// 행을 만드는 네 헤더 값이 다 있다. 없으면 생성된 행에 빈 칸이 생기고, 빈 칸은 읽는
/// 사람에게 "상태 없음" 이라 통과시킬 값이 아니다.
#[test]
fn every_adr_header_carries_the_row_fields() {
    let all = adrs(&repo_root());
    let mut missing = Vec::new();
    for a in &all {
        for (name, v) in [
            ("제목", &a.title),
            ("Status", &a.status),
            ("Date", &a.date),
            ("Tags", &a.tags),
        ] {
            if v.as_deref().is_none_or(|s| s.trim().is_empty()) {
                missing.push(format!("{} — {name} 이 없다", a.file));
            }
        }
    }
    assert!(
        all.len() >= MIN_ADRS,
        "ADR {} 개 — 수집이 죽었다",
        all.len()
    );
    assert!(
        missing.is_empty(),
        "헤더 값이 빠져 생성된 행에 빈 칸이 생긴다:\n  {}",
        missing.join("\n  ")
    );
}

/// 앞 줄과 뒤 줄이 처음 갈리는 자리를 몇 줄 보여 준다 — 전문을 찍으면 실패문이 묻힌다.
fn first_difference(want: &str, got: &str) -> String {
    let (w, g): (Vec<&str>, Vec<&str>) = (want.lines().collect(), got.lines().collect());
    let at = w
        .iter()
        .zip(&g)
        .position(|(a, b)| a != b)
        .unwrap_or(w.len().min(g.len()));
    format!(
        "{}행부터 다르다\n  생성: {:?}\n  파일: {:?}",
        at + 1,
        w.get(at).copied().unwrap_or("<끝>"),
        g.get(at).copied().unwrap_or("<끝>")
    )
}

/// **생성 구역이 생성 결과와 같은가** — 행을 손으로 고쳤거나, ADR 헤더를 고치고 다시
/// 안 만들었거나, ADR 을 더하고 안 만들었으면 여기서 빨개진다.
#[test]
fn the_index_rows_are_what_the_generator_renders() {
    let root = repo_root();
    let all = adrs(&root);
    let current = index_text(&root);
    let rendered = render_index(&current, &all)
        .unwrap_or_else(|e| panic!("인덱스의 마커 구조가 깨졌다:\n  {}", e.join("\n  ")));
    // 파일 전체가 아니라 **마커 안**만 센다 — 마커 밖에 적힌 행은 생성물이 아니다.
    let rows = region_row_count(&current);
    println!(
        "[adr-index-parity] ADR {} · 그룹 {} · 생성 구역 행 {rows}",
        all.len(),
        group_slugs(&current).len()
    );
    assert!(
        rows >= MIN_ADRS,
        "생성 구역 행이 {rows} 개다(하한 {MIN_ADRS})"
    );
    assert!(
        rendered == current,
        "{INDEX} 의 생성 구역(`adr-rows:begin` ~ `adr-rows:end`)이 ADR 헤더에서 만든 결과와 \
         다르다. ★ 생성 구역을 손으로 고치지 마라 — ADR 헤더(제목 · Status · Date · Tags · \
         Group)가 정본이다. 헤더를 고친 뒤 \
         `cargo run -p tasty-doc-guards --bin adr-index -- --write` 로 다시 만든다.\n{}",
        first_difference(&rendered, &current)
    );
    assert_eq!(
        rows,
        all.len(),
        "생성 구역 행 수가 ADR 수와 다르다 — 생성 결과와 같은데 이렇다면 생성기가 ADR 을 \
         빠뜨리거나 두 번 싣는다"
    );
}

/// **행이 생성 구역에만 있는가** — 마커 밖의 표 줄은 생성 결과와의 대조에 안 걸린다
/// (생성기가 마커 밖을 안 건드리므로 파일은 생성 결과와 같다). 그래서 따로 센다.
#[test]
fn no_table_line_sits_outside_the_generated_regions() {
    let current = index_text(&repo_root());
    let stray = stray_table_lines(&current);
    println!(
        "[adr-stray] 생성 구역 {} · 구역 밖 표 줄 {}",
        group_slugs(&current).len(),
        stray.len()
    );
    assert!(
        !group_slugs(&current).is_empty(),
        "생성 구역을 하나도 못 읽었다 — 구역 밖이 파일 전체라 이 판정은 미측정이다"
    );
    assert!(
        stray.is_empty(),
        "{INDEX} 의 생성 구역(`adr-rows:begin` ~ `adr-rows:end`) 밖에 표 줄이 있다 {} 건. \
         생성기는 마커 밖을 안 건드리므로 이 줄은 `--write` 로 안 없어지고 생성 결과와의 대조에도 \
         안 걸린다. 머리말에 행을 끼웠거나 `adr-rows:end` 아래에 행을 덧붙인 것이다 — 그 줄을 \
         지우고, 그 ADR 이 빠진 그룹에 들어가야 한다면 ADR 헤더의 `- **Group**:` 을 고친 뒤 \
         생성기를 돌린다. 머리말에서 ADR 을 부를 때는 표가 아니라 산문 속 번호로 쓴다:\n  {}",
        stray.len(),
        stray.join("\n  ")
    );
}

/// **그룹 배치** — 모든 ADR 이 정확히 한 그룹에 있고, 그 그룹이 인덱스에 실재한다.
#[test]
fn every_adr_sits_in_exactly_one_known_group() {
    let root = repo_root();
    let all = adrs(&root);
    let slugs = group_slugs(&index_text(&root));
    let placed = all.iter().filter(|a| a.groups.len() == 1).count();
    println!(
        "[adr-group] ADR {} · 그룹 하나에 든 ADR {placed} · 인덱스 그룹 {}",
        all.len(),
        slugs.len()
    );
    assert!(
        !slugs.is_empty(),
        "인덱스에서 `adr-rows:begin` 마커를 하나도 못 읽었다"
    );
    let v = placement_violations(&all, &slugs);
    assert!(
        v.is_empty(),
        "그룹 배치가 깨졌다 {} 건. 그룹 고르는 기준은 docs/adr/template.md 의 \
         \"인덱스에 행을 추가할 때\" 다:\n  {}",
        v.len(),
        v.join("\n  ")
    );
}

/// **결정 사슬이 양 끝에 적혔는가.**
#[test]
fn decision_chains_are_written_on_both_ends() {
    let all = adrs(&repo_root());
    let superseded = all
        .iter()
        .filter(|a| a.status.as_deref().and_then(superseder).is_some())
        .count();
    let (v, ends) = chain_violations(&all);
    println!(
        "[adr-chain] ADR {} · 대체된 ADR {superseded} · 사슬 끝 {ends} · 위반 {}",
        all.len(),
        v.len()
    );
    assert!(all.len() >= MIN_ADRS, "ADR 수집 결과가 너무 작다");
    assert!(
        v.is_empty(),
        "결정 사슬이 한쪽에만 적혔다 {} 건. 작성 규칙은 docs/adr/template.md를 따른다:\n  {}",
        v.len(),
        v.join("\n  ")
    );
}

/// **머리말이 부르는 번호가 실재하는가.**
#[test]
fn group_preambles_cite_only_existing_adrs() {
    let root = repo_root();
    let (v, seen) = preamble_violations(&index_text(&root), &adrs(&root));
    println!("[adr-preamble] 번호 토큰 {seen} · 위반 {}", v.len());
    assert!(
        seen >= MIN_PREAMBLE_NUMBERS,
        "머리말에서 번호를 {seen} 개밖에 못 읽었다(하한 {MIN_PREAMBLE_NUMBERS}) — 판독이 죽었다"
    );
    assert!(
        v.is_empty(),
        "머리말이 없는 ADR 번호를 부른다:\n  {}",
        v.join("\n  ")
    );
}

/// 생성기 bin 이 라이브러리와 같은 답을 낸다 — 레포에서 `--check` 가 0 으로 끝난다.
#[test]
fn the_generator_bin_agrees_with_the_repo() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_adr-index"))
        .arg("--check")
        .arg(repo_root())
        .output()
        .expect("adr-index 를 못 돌렸다");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "adr-index --check 가 {:?} 로 끝났다:\n{err}",
        out.status.code()
    );
    assert!(
        err.contains("[adr-index] ADR "),
        "모수 줄이 안 나왔다:\n{err}"
    );
}

// ── 합성 코퍼스 — 레포 자신만 읽으면 위 판정이 빨개지는 상태가 한 번도 안 만들어진다 ──

fn adr(num: &str, status: &str, group: &str, refs: &str) -> Adr {
    let group_line = if group.is_empty() {
        String::new()
    } else {
        format!("- **Group**: {group}\n")
    };
    parse_adr(
        &format!("{num}-x.md"),
        format!(
            "# ADR-{num}: 제목 {num}\n\n- **Status**: {status}\n- **Date**: 2026-09-23\n\
             - **Tags**: t\n{group_line}\n## References\n\n{refs}\n"
        ),
    )
}

const TWO_GROUPS: &str = "머리\n\n## A\n\n0001 을 부른다.\n\n<!-- adr-rows:begin a -->\n\
<!-- adr-rows:end a -->\n\n## B\n\n<!-- adr-rows:begin b -->\n<!-- adr-rows:end b -->\n";

#[test]
fn rendering_touches_only_the_inside_of_the_markers() {
    let all = vec![
        adr("0001", "Accepted", "a", ""),
        adr("0002", "Accepted — 사유", "b", ""),
    ];
    let once = render_index(TWO_GROUPS, &all).expect("구조가 맞다");
    assert!(once.contains("| 0002 | [제목 0002](0002-x.md) | Accepted | 2026-09-23 | t |"));
    assert!(
        once.starts_with("머리\n\n## A\n\n0001 을 부른다.\n"),
        "마커 밖을 바꿨다"
    );
    // 다시 돌려도 같다 — 생성 결과가 고정점이 아니면 가드가 영영 초록이 안 된다.
    assert_eq!(render_index(&once, &all).expect("구조가 맞다"), once);
    // 생성 구역 행을 손으로 고치면 결과와 달라진다.
    let hand = once.replace("| Accepted — 사유 |", "| Accepted |").replace(
        "| 0002 | [제목 0002](0002-x.md) | Accepted |",
        "| 0002 | [손으로 고친 제목](0002-x.md) | Accepted |",
    );
    assert_ne!(render_index(&hand, &all).expect("구조가 맞다"), hand);
}

/// **머리말에 git 충돌 표지가 안 남았는가** — `--write` 는 머리말을 안 건드리므로, 병합
/// 충돌을 생성기로 풀고 머리말의 충돌을 잊으면 표지가 그대로 커밋된다.
#[test]
fn no_conflict_marker_is_left_in_the_preambles() {
    let current = index_text(&repo_root());
    let found = conflict_marker_lines(&current);
    println!("[adr-conflict] 구역 밖 충돌 표지 {}", found.len());
    assert!(
        found.is_empty(),
        "{INDEX} 의 머리말(생성 구역 밖)에 git 충돌 표지가 남았다 {} 건. 생성기는 머리말을 안 \
         건드린다 — 양쪽 산문을 사람이 합친 뒤 표지를 지운다(docs/dev-guide/adr-index.md \
         \"언제 돌리나\"):\n  {}",
        found.len(),
        found.join("\n  ")
    );
}

/// 충돌 표지 판정의 **양성 대조** — 머리말 충돌은 `render_index` 가 그대로 두고(그래서 이
/// 판정이 따로 필요하다), 생성 구역 안의 충돌은 생성기가 덮는다.
#[test]
fn a_conflict_left_in_a_preamble_is_caught() {
    let all = vec![
        adr("0001", "Accepted", "a", ""),
        adr("0002", "Accepted", "b", ""),
    ];
    let clean = render_index(TWO_GROUPS, &all).expect("구조가 맞다");
    assert!(
        conflict_marker_lines(&clean).is_empty(),
        "정상 인덱스를 위반으로 셌다"
    );
    let preamble = clean.replace(
        "0001 을 부른다.\n",
        "<<<<<<< HEAD\n0001 을 부른다.\n=======\n0001 을 이어 부른다.\n>>>>>>> theirs\n",
    );
    let regenerated = render_index(&preamble, &all).expect("구조가 맞다");
    assert_eq!(regenerated, preamble, "생성기가 머리말을 건드렸다");
    assert_eq!(
        conflict_marker_lines(&regenerated).len(),
        3,
        "머리말 충돌을 못 잡았다"
    );
    let in_region = clean.replace(
        "<!-- adr-rows:end a -->\n",
        "<<<<<<< HEAD\n| 0009 | x |\n=======\n| 0008 | y |\n>>>>>>> theirs\n<!-- adr-rows:end a -->\n",
    );
    assert_eq!(
        render_index(&in_region, &all).expect("구조가 맞다"),
        clean,
        "생성 구역 안의 충돌은 생성기가 덮어야 한다"
    );
}

/// 생성 결과의 **완전성** — `Group` 이 없거나 인덱스에 없는 slug 를 가리키는 ADR 은 행 없이
/// 빠지는데, 그래도 파일은 생성 결과와 같다(생성 대조가 못 잡는 것을 먼저 보인다). 그 빈칸을
/// 생성기 bin 이 rc 1 로 내는 근거가 이 판정이다.
#[test]
fn an_adr_left_out_of_the_rendered_rows_is_caught() {
    let all = vec![
        adr("0001", "Accepted", "a", ""),
        adr("0002", "Accepted", "b", ""),
    ];
    let clean = render_index(TWO_GROUPS, &all).expect("구조가 맞다");
    assert!(
        incomplete_rows(&clean, &all).is_empty(),
        "완전한 결과를 불완전으로 셌다"
    );
    for (bad, why) in [
        (adr("0003", "Accepted", "", ""), "Group 없음"),
        (adr("0003", "Accepted", "nowhere", ""), "인덱스에 없는 slug"),
    ] {
        let with = [all.clone(), vec![bad]].concat();
        let rendered = render_index(&clean, &with).expect("구조가 맞다");
        assert_eq!(
            rendered, clean,
            "{why}: 생성 대조가 이미 잡는다면 이 판정의 전제가 틀렸다"
        );
        assert_eq!(
            incomplete_rows(&rendered, &with),
            vec!["0003-x.md — 생성 구역에 행이 없다".to_string()],
            "{why}: 빠진 ADR 을 못 잡았다"
        );
    }
    let twice = adr("0003", "Accepted", "a, b", "");
    let with = [all.clone(), vec![twice]].concat();
    let rendered = render_index(&clean, &with).expect("구조가 맞다");
    assert_eq!(
        incomplete_rows(&rendered, &with),
        vec!["0003-x.md — 생성 구역에 행이 2 개다".to_string()],
        "두 그룹에 실린 ADR 을 못 잡았다"
    );
}

/// 마커 밖 행의 **양성 대조** — 머리말에 끼운 표(B2)와 `adr-rows:end` 바로 아래 덧붙인
/// 행(B2c) 둘 다 생성 결과와의 대조를 통과하는 것을 먼저 보이고(그래서 이 판정이 따로
/// 필요하다), 그 둘을 이 판정이 잡는지 묻는다.
#[test]
fn a_row_outside_the_markers_is_caught() {
    let all = vec![
        adr("0001", "Accepted", "a", ""),
        adr("0002", "Accepted", "b", ""),
    ];
    let clean = render_index(TWO_GROUPS, &all).expect("구조가 맞다");
    assert!(
        stray_table_lines(&clean).is_empty(),
        "정상 인덱스를 위반으로 셌다"
    );
    assert_eq!(region_row_count(&clean), 2, "구역 안 행 수");
    let row = "| 0002 | [제목 0002](0002-x.md) | Accepted | 2026-09-23 | t |\n";
    let in_preamble = clean.replace(
        "0001 을 부른다.\n",
        &format!(
            "0001 을 부른다.\n\n| # | Title | Status | Date | Tags |\n|---|---|---|---|---|\n{row}"
        ),
    );
    let after_end = clean.replace(
        "<!-- adr-rows:end b -->\n",
        &format!("<!-- adr-rows:end b -->\n{row}"),
    );
    for (bad, why, want) in [
        (in_preamble, "머리말에 끼운 표", 3),
        (after_end, "end 아래 덧붙인 행", 1),
    ] {
        assert_eq!(
            render_index(&bad, &all).expect("구조가 맞다"),
            bad,
            "{why}: 생성 대조가 이미 잡는다면 이 판정의 전제가 틀렸다"
        );
        assert_eq!(
            region_row_count(&bad),
            2,
            "{why}: 구역 밖 행을 구역 행으로 셌다"
        );
        assert_eq!(stray_table_lines(&bad).len(), want, "{why} 를 못 잡았다");
    }
}

/// 생성기 bin 의 **rc 배선** — 위 합성 시험들은 판정 함수를 부르고,
/// `the_generator_bin_agrees_with_the_repo` 는 완전한 레포에서 rc 0 만 본다. 그래서 판정이
/// 옳아도 그 결과를 rc 로 바꾸는 줄이 빠지면 아무 시험도 안 빨개진다. 여기서는 합성
/// 코퍼스를 디스크에 쓰고 `--check` 를 돌려, 생성 결과와 **같은** 파일에서도 rc 1 이 나는
/// 세 갈래(불완전 · 구역 밖 표 줄 · 머리말 충돌)와, 파일이 **다른** 갈래를 함께 묻는다.
#[test]
fn the_generator_bin_fails_on_a_synthetic_corpus() {
    fn adr_file(num: &str, group: &str) -> (String, String) {
        let body = if group.is_empty() {
            String::new()
        } else {
            format!("- **Group**: {group}\n")
        };
        (
            format!("{num}-x.md"),
            format!(
                "# ADR-{num}: 제목 {num}\n\n- **Status**: Accepted\n- **Date**: 2026-09-23\n\
                 - **Tags**: t\n{body}\n## References\n\n"
            ),
        )
    }
    /// `mode` 로 돌리고 (rc, stderr, 돌린 뒤의 인덱스) 를 돌려준다.
    fn run(
        what: &str,
        mode: &str,
        files: &[(String, String)],
        index: &str,
    ) -> (Option<i32>, String, String) {
        let root = Scratch::new(what);
        let dir = root.path().join(ADR_DIR);
        std::fs::create_dir_all(&dir).expect("임시 디렉토리를 못 만들었다");
        for (name, body) in files {
            std::fs::write(dir.join(name), body).expect("쓰기 실패");
        }
        std::fs::write(root.path().join(INDEX), index).expect("쓰기 실패");
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_adr-index"))
            .arg(mode)
            .arg(root.path())
            .output()
            .expect("adr-index 를 못 돌렸다");
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).to_string(),
            std::fs::read_to_string(root.path().join(INDEX)).expect("다시 읽기 실패"),
        )
    }

    let two = vec![adr_file("0001", "a"), adr_file("0002", "b")];
    let parsed: Vec<Adr> = two.iter().map(|(n, b)| parse_adr(n, b.clone())).collect();
    let clean = render_index(TWO_GROUPS, &parsed).expect("구조가 맞다");

    // 음성 대조 — 이 픽스처가 그 자체로 초록이 아니면 아래 rc 1 은 아무것도 안 잰다.
    let (rc, err, _) = run("adrbin-clean", "--check", &two, &clean);
    assert_eq!(rc, Some(0), "완전한 합성 코퍼스가 초록이 아니다:\n{err}");
    assert!(
        err.contains("생성 구역 행 현재 2 / 생성 결과 2"),
        "모수 줄:\n{err}"
    );

    let missing = [two.clone(), vec![adr_file("0003", "")]].concat();
    let row = "| 0002 | [제목 0002](0002-x.md) | Accepted | 2026-09-23 | t |\n";
    let hand_deleted = clean.replace(row, "");
    assert_ne!(hand_deleted, clean, "지울 행을 못 찾았다");
    // 넷째 칸: `--write` 가 파일을 생성 결과로 맞춘 뒤의 rc. 앞의 셋은 파일이 이미 생성 결과와
    // 같은데도 빨간 갈래라 `--write` 로도 안 풀린다 — 종료코드 문서의 "두 모드 모두" 가 그것이다.
    for (what, files, index, want, write_rc) in [
        (
            "adrbin-incomplete",
            &missing,
            clean.clone(),
            "0003-x.md — 생성 구역에 행이 없다",
            1,
        ),
        (
            "adrbin-stray",
            &two,
            clean.replace(
                "<!-- adr-rows:end b -->\n",
                &format!("<!-- adr-rows:end b -->\n{row}"),
            ),
            "구역 밖 표 줄 1",
            1,
        ),
        (
            "adrbin-conflict",
            &two,
            clean.replace(
                "0001 을 부른다.\n",
                "<<<<<<< HEAD\n0001 을 부른다.\n=======\n0001 을 이어 부른다.\n>>>>>>> theirs\n",
            ),
            "구역 밖 충돌 표지 3",
            1,
        ),
        (
            "adrbin-differs",
            &two,
            hand_deleted,
            "생성 구역 행 현재 1 / 생성 결과 2",
            0,
        ),
    ] {
        let (rc, err, _) = run(what, "--check", files, &index);
        assert_eq!(rc, Some(1), "{what} --check: rc 가 1 이 아니다:\n{err}");
        assert!(err.contains(want), "{what}: `{want}` 가 안 찍혔다:\n{err}");
        let (rc, err, after) = run(what, "--write", files, &index);
        assert_eq!(
            rc,
            Some(write_rc),
            "{what} --write: rc 가 {write_rc} 가 아니다:\n{err}"
        );
        let want_after = if write_rc == 0 { &clean } else { &index };
        assert_eq!(&after, want_after, "{what} --write: 쓴 결과가 다르다");
    }
}

#[test]
fn a_broken_marker_structure_is_refused() {
    let all = vec![adr("0001", "Accepted", "a", "")];
    for broken in [
        "<!-- adr-rows:begin a -->\n",
        "<!-- adr-rows:begin a -->\n<!-- adr-rows:end b -->\n",
        "<!-- adr-rows:end a -->\n",
        "<!-- adr-rows:begin a -->\n<!-- adr-rows:end a -->\n<!-- adr-rows:begin a -->\n<!-- adr-rows:end a -->\n",
    ] {
        assert!(
            render_index(broken, &all).is_err(),
            "깨진 구조를 받아들였다: {broken:?}"
        );
    }
}

#[test]
fn placement_catches_none_two_and_unknown_groups() {
    let slugs = vec!["a".to_string(), "b".to_string()];
    let ok = vec![
        adr("0001", "Accepted", "a", ""),
        adr("0002", "Accepted", "b", ""),
    ];
    assert!(
        placement_violations(&ok, &slugs).is_empty(),
        "정상을 위반으로 셌다"
    );
    for (bad, why) in [
        (adr("0003", "Accepted", "", ""), "Group 없음"),
        (adr("0003", "Accepted", "a, b", ""), "두 그룹"),
        (adr("0003", "Accepted", "c", ""), "없는 그룹"),
    ] {
        let mut all = ok.clone();
        all.push(bad);
        assert_eq!(
            placement_violations(&all, &slugs).len(),
            1,
            "{why} 를 못 잡았다"
        );
    }
    let only_a = vec![adr("0001", "Accepted", "a", "")];
    assert_eq!(
        placement_violations(&only_a, &slugs).len(),
        1,
        "빈 그룹을 못 잡았다"
    );
}

#[test]
fn chains_must_be_written_on_both_ends() {
    let amend = "- 개정 대상: [ADR-0001](0001-x.md) (조항)";
    let back = "- 부분 개정: [0002](0002-x.md) (조항 개정)";
    let both = vec![
        adr("0001", "Accepted", "a", back),
        adr("0002", "Accepted", "a", amend),
    ];
    assert!(
        chain_violations(&both).0.is_empty(),
        "양쪽에 적힌 사슬을 위반으로 셌다"
    );
    let fwd_only = vec![
        adr("0001", "Accepted", "a", ""),
        adr("0002", "Accepted", "a", amend),
    ];
    assert_eq!(
        chain_violations(&fwd_only).0.len(),
        1,
        "개정 대상만 적힌 사슬"
    );
    let back_only = vec![
        adr("0001", "Accepted", "a", back),
        adr("0002", "Accepted", "a", ""),
    ];
    assert_eq!(
        chain_violations(&back_only).0.len(),
        1,
        "부분 개정만 적힌 사슬"
    );

    // 대체 — 새 ADR 이 옛 ADR 을 부르고, 같은 그룹이어야 한다.
    let sup = "Superseded by [ADR-0002](0002-x.md) — 사유";
    let cited = "- 대체: [ADR-0001](0001-x.md)";
    let good = vec![
        adr("0001", sup, "a", ""),
        adr("0002", "Accepted", "a", cited),
    ];
    assert!(
        chain_violations(&good).0.is_empty(),
        "정상 대체를 위반으로 셌다"
    );
    let silent = vec![adr("0001", sup, "a", ""), adr("0002", "Accepted", "a", "")];
    assert_eq!(
        chain_violations(&silent).0.len(),
        1,
        "옛 ADR 을 안 부르는 대체"
    );
    let moved = vec![
        adr("0001", sup, "a", ""),
        adr("0002", "Accepted", "b", cited),
    ];
    assert_eq!(chain_violations(&moved).0.len(), 1, "그룹이 갈린 대체");
    let dangling = vec![adr("0001", "Superseded by 0009", "a", "")];
    assert_eq!(chain_violations(&dangling).0.len(), 1, "없는 ADR 로의 대체");
    // 개정 대상이 이후 대체됐으면 그 Status 가 끝을 대신한다.
    let amend_sup = vec![
        adr("0001", sup, "a", ""),
        adr("0002", "Accepted", "a", amend),
    ];
    assert!(
        chain_violations(&amend_sup).0.is_empty(),
        "대체가 끝을 대신하지 못했다"
    );
}

#[test]
fn a_catalogue_without_historical_chains_is_valid() {
    let all = vec![
        adr("0601", "Accepted", "foundation", ""),
        adr("0625", "Deferred", "plugins", ""),
    ];
    let (violations, ends) = chain_violations(&all);
    assert!(violations.is_empty());
    assert_eq!(ends, 0);
}

#[test]
fn a_preamble_number_without_an_adr_is_caught() {
    let all = vec![adr("0001", "Accepted", "a", "")];
    let (v, seen) = preamble_violations(TWO_GROUPS, &all);
    assert_eq!((v.len(), seen), (0, 1), "정상 머리말");
    let bad = TWO_GROUPS.replace("0001 을", "0001 → 0613 을");
    let (v, seen) = preamble_violations(&bad, &all);
    assert_eq!((v.len(), seen), (1, 2), "없는 번호를 못 잡았다");
}

/// `MIN_ADRS` 의 **양성 대조** — 모수가 하한 아래로 떨어지는 코퍼스를 만들고 수집기가
/// 그것을 말하는지 묻는다. 위 시험들은 레포 자신을 읽으므로 수집이 죽는 상황이 한 번도
/// 안 만들어진다.
#[test]
fn the_adr_floor_sees_a_collapsed_collection() {
    let root =
        std::env::temp_dir().join(format!("tasty-adrfloor-{}-{}", std::process::id(), line!()));
    // 앞선 실행의 잔여를 치운다 — 없는 것이 정상이라 실패가 정보가 아니다.
    let _ = std::fs::remove_dir_all(&root);
    let dir = root.join(ADR_DIR);
    std::fs::create_dir_all(&dir).expect("임시 디렉토리를 못 만들었다");
    assert_eq!(
        adrs(&root).len(),
        0,
        "빈 뿌리에서 0 이 아니면 인자를 안 보고 레포를 읽는 것이다"
    );
    for (num, slug) in [("0001", "a"), ("0002", "b"), ("0003", "c")] {
        std::fs::write(dir.join(format!("{num}-{slug}.md")), "").expect("쓰기 실패");
    }
    std::fs::write(dir.join("template.md"), "").expect("쓰기 실패");
    std::fs::write(dir.join("index.md"), "").expect("쓰기 실패");
    assert_eq!(
        adrs(&root).len(),
        3,
        "번호 아닌 `.md` 를 세거나 디렉터리를 안 읽었다"
    );
    assert!(adrs(&root).len() < MIN_ADRS, "이 대조는 하한 아래여야 한다");
    // 뒷정리 실패는 무시한다 — 임시 디렉토리라 다음 실행이 먼저 지운다.
    let _ = std::fs::remove_dir_all(&root);
}
