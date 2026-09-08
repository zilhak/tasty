//! `docs/dev-guide/clippy-policy.md` 의 표 `현재 워크스페이스 설정` 이 루트 `Cargo.toml`
//! 의 `[workspace.lints]` 와 **같은 짝**을 드는지 본다.
//!
//! 그 표의 각 행은 lint 이름 하나와 레벨 하나를 들고, 매니페스트가 같은 짝을 든다.
//! 어디서도 파생되지 않는 **손으로 베낀 사본**이라 한쪽만 고쳐도 아무 채널이 안 운다 —
//! 매니페스트는 컴파일에 먹고 문서는 안 먹으므로 빌드도 테스트도 초록이다.
//!
//! 실측: `036cc51a9`(2026-09-07) 가 `multiple_unsafe_ops_per_block` 을 warn → deny 로
//! 승격하면서 이 문서를 안 건드렸다. 그 커밋의 `--stat` 다섯 파일에 `docs/` 가 하나도
//! 없다. 값이 갈린 채로 하루가 지났고, 그 사실은 어디에도 값으로 안 남았다.
//!
//! ## 왜 양방향인가
//!
//! 한 방향만 보는 가드는 **틀린 방향 하나를 굳힌다.** 표에 있는데 매니페스트에 없는
//! lint 는 문서가 없는 정책을 주장하는 것이고, 매니페스트에 있는데 표에 없는 lint 는
//! 표가 "현재 워크스페이스 설정" 이라는 제목으로 부분 사본을 내미는 것이다. 뒤엣것이
//! 특히 조용하다 — 빠진 행은 **틀린 값이 아니라 없는 값**이라 읽는 사람이 못 본다.
//! 실측: 이 가드를 처음 돌린 자리에서 표는 3 행, 매니페스트는 12 항목이었다.
//!
//! ## 모수 — 우변을 `[workspace.lints]` **전체**로 잡는다
//!
//! `.clippy` 만 볼 수도 있었다. 그러지 않은 기준은 **그 표에 오는 사람이 무엇을 물으러
//! 오는가** 다 — 물음은 "이 워크스페이스에서 무엇이 막혀 있나" 이고, 그 물음은
//! clippy/rustc 로 갈리지 않는다. 집행 경로가 같기 때문이다: 멤버가
//! `[lints] workspace = true` 로 상속하고, 같은 컴파일 단계에서 발동하고, 같은
//! `#[allow]` 로 빠져나간다. lint 이름의 소유 도구는 답의 구현 세부이지 물음의 축이 아니다.
//!
//! 좁히는 쪽을 **절 제목이 거짓이 된다**는 이유만으로 기각하지는 않았다 — 절 제목도
//! 함께 좁히면("현재 clippy 설정") 거짓은 면한다. 그러니 그 제약은 답을 정하지 않고
//! **비용을 옮길 뿐**이다. 좁히는 쪽을 실제로 죽이는 것은 그 다음이다: 그러면
//! `[workspace.lints.rust]` 다섯이 적힌 유일한 자리가 매니페스트 주석이 되고,
//! `docs/` 인덱스에서 도달할 수 없게 된다. 대신 문서 제목 쪽을 넓혔다.
//!
//! ## 무엇을 안 보는가 (사전 등록)
//!
//! 세는 척하지 않으려고 적어 둔다. 나중에 "이건 왜 안 잡혔나" 가 나왔을 때 범위 밖인지
//! 결함인지 그 자리에서 갈리게 하려는 것이다.
//!
//! - **`clippy.toml` 의 설정값**(`disallowed-methods` · `cognitive-complexity-threshold`).
//!   레벨이 아니라 그 레벨이 무는 대상·임계값이라 짝지을 우변이 없다. 한때 같은 표의
//!   마지막 행이었고 그때는 이 판독기가 그 행을 **조용히 흘렸다** — 그 skip 이 구멍이었다:
//!   매니페스트를 안 가리키는 행이면 무엇이든 같은 자리로 빠져나갔다. 그 행을 문서의
//!   별도 소절로 옮겨 표를 (lint, 레벨) 짝만 들게 만들고, 판독기는 이제 **모수 밖 행을
//!   만나면 죽는다.** 예외 갈래를 없애는 쪽이 예외를 정확히 좁히는 쪽보다 싸다.
//! - **줄바꿈에 쪼개진 셀.** `markdown_tables_render_whole` 이 열 수 술어로 이미 본다.
//!   같은 물음에 답을 둘로 만들지 않으려고 여기서 다시 안 센다. 다만 그 가드가 죽으면
//!   이쪽은 쪼개진 행을 **행 아님**으로 흘려보내므로, 그때 이 가드의 침묵은 근거가 아니다.
//! - **표 밖에서 같은 lint 를 언급하는 산문.** 같은 문서의 복잡도 게이트 문단이
//!   `cognitive_complexity` 를 언급하지만 레벨을 **주장하지 않는다**. 주장하는 자리만
//!   좌변이다. 산문이 나중에 레벨을 주장하기 시작하면 그것은 이 가드 밖의 새 사본이다.
//! - **멤버 크레이트가 그 정책을 상속하는가.** `workspace_lints_are_inherited` 의 물음이다.
//! - **위치 단위 `#[allow]` 면제의 수.** `check-allow-reason.sh` 의 래칫과
//!   `complexity_allowlist_docs_parity` 의 몫이다.
//!
//! ## 판독기는 모르는 형태를 만나면 죽는다
//!
//! cargo 는 레벨을 표 형태(`{ level = "deny", priority = -1 }`)로도 받는다. 지금 이
//! 레포는 안 쓰지만, 만나면 **조용히 건너뛰지 않고 panic 한다.** 건너뛰면 그 항목이
//! 우변에서 사라지고 좌변에도 없으면 양쪽이 맞아 통과한다 — 안 본 것이 초록이 된다.
//!
//! 선례: `crates/tasty-doc-guards/tests/complexity_allowlist_docs_parity.rs`
//! (문서가 손으로 베낀 값을 기계가 대조한다 · 안 세는 것을 모듈 주석에 적는다).

use std::collections::BTreeMap;

const DOC: &str = "docs/dev-guide/clippy-policy.md";
const MANIFEST: &str = "Cargo.toml";

/// 좌변이 있는 절. 이 제목이 바뀌면 판독 불가로 죽어야 한다.
const SECTION: &str = "## 현재 워크스페이스 설정";

/// `[workspace.lints.<절>]` 의 절 머리 접두.
const LINTS_PREFIX: &str = "[workspace.lints.";

/// 표의 위치 칸이 매니페스트 절을 가리킬 때의 형태 — `` `Cargo.toml [workspace.lints.rust]` ``.
const LOCATION_PREFIX: &str = "Cargo.toml [workspace.lints.";

/// 우변이 이 아래로 떨어지면 수집이 죽은 것이다.
///
/// 값의 근거: 2026-09-08 실측 **12**(clippy 7 + rust 5). 하한을 실측보다 낮게 잡는 것은
/// lint 가 정당하게 줄 수 있기 때문이고, 그 아래는 감소가 아니라 파서가 깨진 것이다.
///
/// **이 수를 내려서 초록을 만들지 마라.** 양쪽이 다 비면 아래 집합 비교는 전부 통과한다.
const MIN_LINTS: usize = 8;

/// `(절, lint 이름)` → 레벨.
type Levels = BTreeMap<(String, String), String>;

fn read(rel: &str) -> String {
    let path = tasty_doc_guards::repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("읽을 수 없다: {} — {e}", path.display()))
}

/// cargo 가 받는 레벨 이름. 그 밖의 값은 판독 실패다.
fn is_level(v: &str) -> bool {
    matches!(v, "allow" | "warn" | "deny" | "forbid")
}

/// 루트 `Cargo.toml` 의 `[workspace.lints.*]` 를 읽는다.
///
/// 순수 함수다 — 변이 테스트가 파일을 안 고치고 찌를 수 있어야 한다.
fn manifest_levels(toml: &str) -> Levels {
    let mut out = Levels::new();
    let mut section: Option<String> = None;
    for line in toml.lines() {
        // TOML 주석은 `#` 로 시작하므로 절 머리는 **줄 맨 앞**에서만 찾는다.
        // 이 레포는 규칙 본문을 주석에 적어서, 주석 안의 `[...]` 언급이 흔하다.
        if line.starts_with('[') {
            section = line
                .trim_end()
                .strip_prefix(LINTS_PREFIX)
                .and_then(|s| s.strip_suffix(']'))
                .map(str::to_string);
            continue;
        }
        let Some(sec) = section.clone() else { continue };
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((name, value)) = trimmed.split_once('=') else {
            panic!("`[workspace.lints.{sec}]` 안에서 모르는 줄을 만났다: `{trimmed}`");
        };
        let level = value
            .split('#')
            .next()
            .unwrap_or_default()
            .trim()
            .trim_matches('"')
            .to_string();
        assert!(
            is_level(&level),
            "`[workspace.lints.{sec}]` 의 `{}` 레벨을 못 읽었다 — 읽은 값 `{level}`. \
             표 형태(`{{ level = \"deny\", priority = -1 }}`)라면 이 판독기를 고쳐라. \
             건너뛰면 그 항목이 우변에서 사라지고, 좌변에도 없으면 양쪽이 맞아 통과한다",
            name.trim()
        );
        out.insert((sec, name.trim().to_string()), level);
    }
    out
}

/// 문서 표 `현재 워크스페이스 설정` 에서 매니페스트 절을 가리키는 행만 읽는다.
///
/// 순수 함수다 — 변이 테스트가 파일을 안 고치고 찌를 수 있어야 한다.
fn doc_levels(md: &str) -> Levels {
    let body = md
        .split_once(SECTION)
        .unwrap_or_else(|| panic!("`{SECTION}` 절을 못 찾았다 — 제목이 바뀌었으면 여기를 고쳐라"))
        .1;
    // 다음 `## ` 제목 전까지가 이 절이다.
    let body = body.split("\n## ").next().unwrap_or(body);

    let mut out = Levels::new();
    // `〃` 는 바로 위 행의 위치를 잇는다. 이어받을 것이 없으면 판독 실패다.
    let mut location: Option<String> = None;
    // 좌변은 절의 **첫 연속 표 블록** 하나다. 절 전체를 훑으면 나중에 그 절에 생기는
    // 다른 표가 조용히 좌변으로 빨려 들어온다 — 아래의 "모수 밖 행은 위반" 판정과
    // 겹치면 그건 결함이 아니라 오탐이 된다. 표는 `|` 로 시작하는 줄이 끊기지 않고
    // 이어지는 덩이라, 그 덩이를 만나면 시작하고 끊기면 멈춘다.
    let mut started = false;
    for line in body.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            if started {
                break;
            }
            continue;
        }
        started = true;
        let cells: Vec<&str> = trimmed
            .trim_matches('|')
            .split('|')
            .map(str::trim)
            .collect();
        // 헤더행과 구분행을 뺀다. 열 수 정합은 `markdown_tables_render_whole` 의 몫이라
        // 여기서는 세 칸 미만만 흘린다.
        if cells.len() < 3 || cells[0] == "위치" || cells[0].starts_with("---") {
            continue;
        }
        let raw_location = cells[0].trim_matches('`');
        if raw_location != "〃" {
            location = Some(raw_location.to_string());
        }
        let here = location
            .clone()
            .unwrap_or_else(|| panic!("표 첫 행이 `〃` 다 — 이어받을 위치가 없다: `{trimmed}`"));

        // 이 표의 모든 행은 매니페스트 절을 가리켜야 한다. 아닌 행을 흘리면 그 자리가
        // 무엇이든 빠져나가는 구멍이 된다 — 레벨이 아닌 설정값은 문서의 별도 소절이 든다.
        let Some(sec) = here
            .strip_prefix(LOCATION_PREFIX)
            .and_then(|s| s.strip_suffix(']'))
        else {
            panic!(
                "표 `현재 워크스페이스 설정` 에 매니페스트 절을 안 가리키는 행이 있다: \
                 위치 `{here}`. 이 표는 (lint, 레벨) 짝만 든다 — 레벨이 아닌 설정값은 \
                 `### clippy.toml` 소절로 옮겨라"
            );
        };

        let setting = cells[1].trim_matches('`');
        let Some((name, level)) = setting.split_once('=') else {
            panic!(
                "`{here}` 를 가리키는 행인데 설정 칸에 레벨이 없다: `{setting}`. \
                 레벨 없는 행을 조용히 흘리면 그 lint 가 좌변에서 사라진다"
            );
        };
        out.insert(
            (sec.to_string(), name.trim().to_string()),
            level.trim().trim_matches('"').to_string(),
        );
    }
    out
}

/// 양쪽을 맞대고 어긋난 것을 사람이 읽는 줄로 만든다.
///
/// 순수 함수다 — 판정 자체를 파일 없이 변이로 찌를 수 있어야 한다.
fn mismatches(doc: &Levels, manifest: &Levels) -> Vec<String> {
    let mut out = Vec::new();
    for (key, level) in doc {
        match manifest.get(key) {
            None => out.push(format!(
                "  표에만 있다: `[workspace.lints.{}]` 의 `{}` — 문서가 없는 정책을 주장한다",
                key.0, key.1
            )),
            Some(actual) if actual != level => out.push(format!(
                "  레벨이 다르다: `[workspace.lints.{}]` 의 `{}` — 표는 \"{level}\", \
                 매니페스트는 \"{actual}\" (정본은 매니페스트다)",
                key.0, key.1
            )),
            Some(_) => {}
        }
    }
    for (key, level) in manifest {
        if !doc.contains_key(key) {
            out.push(format!(
                "  매니페스트에만 있다: `[workspace.lints.{}]` 의 `{}` = \"{level}\" — \
                 표가 부분 사본이다",
                key.0, key.1
            ));
        }
    }
    out
}

#[test]
fn the_table_and_the_manifest_hold_the_same_lint_levels() {
    let manifest = manifest_levels(&read(MANIFEST));
    let doc = doc_levels(&read(DOC));

    // 비영 대조를 판정 앞에 둔다 — 양쪽이 다 비면 아래 집합 비교는 전부 통과한다.
    assert!(
        manifest.len() >= MIN_LINTS,
        "`[workspace.lints.*]` 에서 {} 개만 읽었다(하한 {MIN_LINTS}) — 파서가 깨졌는지 확인해라. \
         이 하한을 내려서 초록을 만들지 마라",
        manifest.len()
    );

    let wrong = mismatches(&doc, &manifest);
    assert!(
        wrong.is_empty(),
        "`{DOC}` 의 표 `현재 워크스페이스 설정` 과 루트 `{MANIFEST}` 의 `[workspace.lints]` 가 \
         어긋났다. 표는 손으로 베낀 사본이고 컴파일에 안 먹으므로, 레벨을 고치는 커밋에서 \
         표도 같이 고쳐라.\n실측: 표 {} 행 · 매니페스트 {} 항목\n{}",
        doc.len(),
        manifest.len(),
        wrong.join("\n")
    );
}

/// 판정기가 실제로 무는지 확인하는 변이 — 파일은 안 고친다.
mod parity_mutations {
    use super::*;

    /// 실제 매니페스트를 읽어 온 뒤 한 값을 흔든다.
    fn real() -> (Levels, Levels) {
        (doc_levels(&read(DOC)), manifest_levels(&read(MANIFEST)))
    }

    #[test]
    fn the_real_pair_agrees() {
        let (doc, manifest) = real();
        assert_eq!(mismatches(&doc, &manifest), Vec::<String>::new());
    }

    #[test]
    fn a_changed_level_is_caught() {
        let (doc, manifest) = real();
        let mut mutated = doc.clone();
        let key = manifest
            .keys()
            .find(|k| manifest[*k] != "warn")
            .expect("모든 레벨이 warn 일 리 없다")
            .clone();
        mutated.insert(key, "warn".into());
        let found = mismatches(&mutated, &manifest);
        assert_eq!(found.len(), 1, "레벨 변이를 못 물었다: {found:?}");
        assert!(found[0].contains("레벨이 다르다"), "{found:?}");
    }

    #[test]
    fn a_row_deleted_from_the_table_is_caught() {
        let (doc, manifest) = real();
        let mut mutated = doc.clone();
        let key = doc.keys().next().expect("표가 비었다").clone();
        mutated.remove(&key);
        let found = mismatches(&mutated, &manifest);
        assert_eq!(found.len(), 1, "행 삭제를 못 물었다: {found:?}");
        assert!(found[0].contains("매니페스트에만 있다"), "{found:?}");
    }

    #[test]
    fn a_lint_removed_from_the_manifest_is_caught() {
        let (doc, manifest) = real();
        let mut mutated = manifest.clone();
        let key = manifest.keys().next().expect("매니페스트가 비었다").clone();
        mutated.remove(&key);
        let found = mismatches(&doc, &mutated);
        assert_eq!(found.len(), 1, "우변 삭제를 못 물었다: {found:?}");
        assert!(found[0].contains("표에만 있다"), "{found:?}");
    }

    // ── 실패문의 양성 대조 ──────────────────────────────────────────────────
    //
    // 아래 넷은 **처방이 서로 다른** 문구다(판독기를 고쳐라 / 제목을 고쳐라 / 표 구조를
    // 고쳐라 / 그 행을 소절로 옮겨라). 합성 입력으로 실제로 발화시켜 **처방을 가르는
    // 낱말**만 단정한다 — 문구 전체를 단정하면 문장을 다듬을 때마다 죽는다.
    //
    // 가드가 초록인 동안 이 문구들은 아무도 안 읽는다. 그래서 여기서 읽는다.

    #[test]
    #[should_panic(expected = "모르는 줄을 만났다")]
    fn an_unparsable_line_in_the_lints_section_says_so() {
        manifest_levels("[workspace.lints.clippy]\nbogus\n");
    }

    #[test]
    #[should_panic(expected = "절을 못 찾았다")]
    fn a_missing_section_heading_says_the_heading_moved() {
        doc_levels("## 다른 절\n\n| 위치 | 설정 | 의미 |\n");
    }

    #[test]
    #[should_panic(expected = "이어받을 위치가 없다")]
    fn a_ditto_in_the_first_row_says_there_is_nothing_to_carry() {
        doc_levels(
            "## 현재 워크스페이스 설정\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
             | 〃 | `a = \"deny\"` | x |\n\n## 다음\n",
        );
    }

    #[test]
    #[should_panic(expected = "설정 칸에 레벨이 없다")]
    fn a_manifest_row_without_a_level_is_not_silently_skipped() {
        doc_levels(
            "## 현재 워크스페이스 설정\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
             | `Cargo.toml [workspace.lints.rust]` | `a` | x |\n\n## 다음\n",
        );
    }

    #[test]
    fn the_manifest_reader_does_not_read_commented_out_lints() {
        let toml = "[workspace.lints.clippy]\n# dead_code = \"deny\"\nfoo = \"warn\"\n";
        let got = manifest_levels(toml);
        assert_eq!(got.len(), 1, "주석을 항목으로 셌다: {got:?}");
    }

    #[test]
    fn the_manifest_reader_stops_at_the_next_section() {
        let toml = "[workspace.lints.rust]\nfoo = \"deny\"\n[package]\nname = \"x\"\n";
        let got = manifest_levels(toml);
        assert_eq!(got.len(), 1, "절 밖의 키를 읽었다: {got:?}");
    }

    #[test]
    #[should_panic(expected = "레벨을 못 읽었다")]
    fn a_table_form_level_is_not_silently_skipped() {
        manifest_levels("[workspace.lints.rust]\ndead_code = { level = \"deny\" }\n");
    }

    #[test]
    fn the_doc_reader_carries_the_ditto_mark() {
        let md = "## 현재 워크스페이스 설정\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
                  | `Cargo.toml [workspace.lints.rust]` | `a = \"deny\"` | x |\n\
                  | 〃 | `b = \"warn\"` | y |\n\n## 다음\n";
        let got = doc_levels(md);
        assert_eq!(got.len(), 2, "`〃` 를 못 이어받았다: {got:?}");
        assert_eq!(got[&("rust".into(), "b".into())], "warn");
    }

    #[test]
    #[should_panic(expected = "매니페스트 절을 안 가리키는 행")]
    fn a_row_outside_the_modulus_is_not_silently_skipped() {
        let md = "## 현재 워크스페이스 설정\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
                  | `clippy.toml` | `disallowed-methods` | x |\n\n## 다음\n";
        // `let _ =` 로 안 받는다 — 그 형태는 `let_underscore_documented` 의 프로덕션
        // 명부에 섞이고, 그것을 덮으려면 파일 전체 allow 가 필요해진다. 반환값은 그냥 버린다.
        doc_levels(md);
    }

    #[test]
    fn the_doc_reader_stops_at_the_end_of_the_first_table() {
        let md = "## 현재 워크스페이스 설정\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
                  | `Cargo.toml [workspace.lints.rust]` | `a = \"deny\"` | x |\n\n\
                  본문 한 줄.\n\n| 위치 | 설정 | 의미 |\n|---|---|---|\n\
                  | `clippy.toml` | `b` | y |\n\n## 다음\n";
        let got = doc_levels(md);
        assert_eq!(got.len(), 1, "둘째 표까지 빨아들였다: {got:?}");
    }
}
