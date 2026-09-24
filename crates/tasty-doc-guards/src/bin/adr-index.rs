//! ADR 헤더로 docs/adr/index.md의 생성 구역만 갱신한다.
//! 생성 규칙은 tasty_doc_guards::adr_index에서 공유한다.
//!
//! ```text
//! cargo run -p tasty-doc-guards --bin adr-index -- --write
//! cargo run -p tasty-doc-guards --bin adr-index -- --check
//! ```
//!
//! 마지막 인자는 저장소 루트이며 생략하면 현재 디렉터리다.
//! 종료코드: 0은 일치/갱신 완료, 1은 차이·생성 구역 밖 표/충돌·행 누락/중복이다.
//! 마커 밖 표/충돌과 잘못된 Group은 --write로 해결되지 않아 직접 고쳐야 한다.
//! 인자·읽기·쓰기·마커 오류나 ADR이 없는 경우는 2로 끝난다.

use std::path::PathBuf;

use tasty_doc_guards::adr_index::{
    INDEX, collect, conflict_marker_lines, group_slugs, incomplete_rows, region_row_count,
    render_index, stray_table_lines,
};

fn fail(msg: &str) -> ! {
    eprintln!("[adr-index] {msg}");
    std::process::exit(2)
}

fn main() {
    let mut write = false;
    let mut root = PathBuf::from(".");
    for a in std::env::args().skip(1) {
        match a.as_str() {
            "--write" => write = true,
            "--check" => write = false,
            other if other.starts_with("--") => fail(&format!("모르는 인자: {other}")),
            other => root = PathBuf::from(other),
        }
    }
    let adrs = collect(&root).unwrap_or_else(|e| fail(&e));
    if adrs.is_empty() {
        fail("ADR 을 하나도 못 찾았다 — 루트가 틀렸거나 수집이 죽었다");
    }
    let path = root.join(INDEX);
    let current = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| fail(&format!("{INDEX} 를 못 읽었다: {e}")))
        .replace("\r\n", "\n");
    let rendered = render_index(&current, &adrs)
        .unwrap_or_else(|errs| fail(&format!("마커 구조가 깨졌다:\n  {}", errs.join("\n  "))));
    let stray = stray_table_lines(&current);
    let conflicts = conflict_marker_lines(&current);
    let incomplete = incomplete_rows(&rendered, &adrs);
    eprintln!(
        "[adr-index] ADR {} · 그룹 {} · 생성 구역 행 현재 {} / 생성 결과 {} · 구역 밖 표 줄 {} · \
         구역 밖 충돌 표지 {}",
        adrs.len(),
        group_slugs(&current).len(),
        region_row_count(&current),
        region_row_count(&rendered),
        stray.len(),
        conflicts.len()
    );
    if !conflicts.is_empty() {
        eprintln!(
            "[adr-index] {INDEX} 의 생성 구역 밖(머리말)에 git 충돌 표지가 남아 있다. 생성기는 \
             머리말을 안 건드린다 — 양쪽 산문을 사람이 합친 뒤 표지를 지운다:\n  {}",
            conflicts.join("\n  ")
        );
    }
    if !incomplete.is_empty() {
        eprintln!(
            "[adr-index] 생성 결과가 불완전하다 — ADR {} 개 중 {} 개가 생성 구역에 정확히 한 행을 \
             갖지 않는다. 행은 헤더의 `- **Group**: <slug>` 가 가리키는 구역에만 생긴다 — 그 ADR \
             헤더에 인덱스에 있는 slug 하나를 적고 다시 돌린다(고르는 기준은 template 의 \
             \"인덱스에 행을 추가할 때\"):\n  {}",
            adrs.len(),
            incomplete.len(),
            incomplete.join("\n  ")
        );
    }
    if !stray.is_empty() {
        eprintln!(
            "[adr-index] {INDEX} 의 생성 구역(`adr-rows:begin` ~ `adr-rows:end`) 밖에 표 줄이 있다. \
             생성기는 마커 밖을 안 건드리므로 이 줄은 `--write` 로 안 없어지고 생성 결과와의 대조에도 \
             안 걸린다. 행이면 지우고 그 ADR 헤더의 Group 을 확인한다 — 행은 생성 구역에만 산다:\n  {}",
            stray.join("\n  ")
        );
    }
    if rendered != current && write {
        std::fs::write(&path, &rendered)
            .unwrap_or_else(|e| fail(&format!("{INDEX} 를 못 썼다: {e}")));
        eprintln!("[adr-index] {INDEX} 를 다시 썼다");
    }
    if rendered == current || write {
        if !stray.is_empty() || !conflicts.is_empty() || !incomplete.is_empty() {
            std::process::exit(1)
        }
        return;
    }
    eprintln!(
        "[adr-index] {INDEX} 의 생성 구역이 ADR 헤더와 다르다. 생성 구역은 손으로 고치지 않는다 — \
         ADR 헤더(제목 · Status · Date · Tags · Group)를 고친 뒤 `cargo run -p tasty-doc-guards \
         --bin adr-index -- --write` 로 다시 만든다"
    );
    std::process::exit(1)
}
