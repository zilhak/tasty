//! 공유 임시 디렉터리의 경로가 실행마다 구분되거나 의도한 공유 사유가 있는지 확인한다(ADR-0045).
//! 판정 규칙과 수신자 추적의 한계는 temp_path 모듈에 있다.
//! 사용자 설정의 홈 미해결 폴백처럼 의도한 공유는 해당 위치에 이유를 적는다.
//! 디렉터리를 공유하고 파일명으로 구분하는 경우도 있다.
//! 파일·경로·구분 성분·사유의 수를 각각 확인해 수집·분류 실패를 찾는다.

use tasty_doc_guards::repo_root;
use tasty_doc_guards::source_text::mask_non_code;
use tasty_doc_guards::temp_path::{Census, census, discriminator_chains};

// src・crates 외에 루트 통합 테스트의 임시 경로도 검사한다.
const SCAN_ROOTS: &[&str] = &["src", "crates", "tests"];

// 2026-09-05 측정: files=1256, sites=55, uniquified=49, reasoned=6. 정상적인 감소를 허용하도록 하한에 여유를 뒀다.
const MIN_FILES: usize = 1100;
const MIN_SITES: usize = 40;
const MIN_UNIQUIFIED: usize = 35;
const MIN_REASONED: usize = 4;

/// 경로 조립의 수신자로 연결하지 못한 temp_dir 호출 수의 양방향 래칫이다.
/// 2026-09-06 측정은 4개였다. 읽기 전용 전달과 파일 밖에서의 경로 생성이 섞일 수 있어
/// 위반으로 단정하지 않고 증가·감소를 모두 검토한다. 수신자 추적 범위는 temp_path::path_building_lines에 있다.
const UNPAIRED_RATCHET: usize = 4;

/// 프로세스 ID는 있으나 같은 프로세스의 재호출을 구분할 성분·사유가 없는 경로 수다.
/// 2026-09-08의 2b0176eef에서 38개였다. 프로세스별 한 번만 실행하거나 호출자가 서로 다른
/// 판별자를 주는 경우도 포함돼 수 자체가 결함 개수는 아니다. 호출부 조건을 검토해야 한다.
/// 스레드 ID는 같은 스레드의 재호출을 구분하지 못한다. 사유가 있으면 이 수에서 제외한다.
const RECALL_BLIND_RATCHET: usize = 38;

/// 실제 검사와 작은 합성 입력이 같은 실패 판정을 쓰도록 하한·래칫을 인자로 받는다.
struct Floors {
    files: usize,
    sites: usize,
    uniquified: usize,
    unpaired: usize,
    reasoned: usize,
    recall_blind: usize,
}

const FLOORS: Floors = Floors {
    files: MIN_FILES,
    sites: MIN_SITES,
    uniquified: MIN_UNIQUIFIED,
    unpaired: UNPAIRED_RATCHET,
    recall_blind: RECALL_BLIND_RATCHET,
    reasoned: MIN_REASONED,
};

/// 모든 실패를 모아 한 판정이 실패해도 나머지 결과를 볼 수 있게 한다.
fn verdicts(c: &Census, f: &Floors) -> Vec<String> {
    let mut out = Vec::new();

    if c.files_scanned < f.files {
        out.push(format!(
            "훑은 파일이 {}개뿐이다(하한 {}). git ls-files의 추적 Rust 목록과 SCAN_ROOTS를 대조한다. 추적되지 않은 파일과 제외 범위 때문에 두 수는 다를 수 있다. 경로 읽기 실패는 panic하고, 빈 루트·루트 누락은 여기서 확인한다. 실제 감소라면 근거와 함께 하한을 갱신한다.",
            c.files_scanned, f.files
        ));
    }
    if c.sites < f.sites {
        out.push(format!(
            "경로 짓는 temp_dir 자리를 {}곳만 찾았다(하한 {}). temp_dir 총 등장 수 및 path_building_lines의 수신자 추적 결과를 대조한다. 경로로 연결하지 못한 수인 UNPAIRED_RATCHET도 함께 확인한다. 실제 감소와 인식 실패를 구별한 뒤 하한을 갱신한다.",
            c.sites, f.sites
        ));
    }
    if c.uniquified < f.uniquified {
        out.push(format!(
            "유니크화된 자리가 {}곳뿐이다(하한 {}). every_recognized_uniquifier_is_actually_recognized 시험과 실제 코드 변경을 대조해 인식 실패인지 구분 성분 삭제인지 확인한다. 하한 변경에는 측정 근거를 남긴다.",
            c.uniquified, f.uniquified
        ));
    }
    if c.unpaired != f.unpaired {
        out.push(format!(
            "경로 생성 수신자로 연결되지 않아 검사에서 빠지는 temp_dir 호출이 {}개다(래칫 {}). 이 값만 올려서 통과시키지 마라. 읽기 전용인지 파일 밖에서 경로를 만드는지 확인한다. 경로를 만든다면 현재 수신자 추적 범위에서 확인할 수 있게 고친다. 줄었으면 래칫도 낮춰 새 누락이 허용되지 않게 한다.",
            c.unpaired, f.unpaired
        ));
    }
    // 시계만으로 재호출을 구분하면 플랫폼의 해상도에 따라 경로가 겹칠 수 있다.
    if !c.weak_only.is_empty() {
        out.push(format!(
            "시계가 프로세스-내 재호출 구분을 홀로 맡는 temp 경로가 {}곳이다. 해상도에 따라 같은 경로가 나올 수 있다. 단조 카운터(fetch_add)나 TempDir을 쓰거나, 시계를 쓰기로 한 이유를 적는다. process::id를 더하는 것은 처방이 아니다. 프로세스 ID는 같은 프로세스의 재호출을 구분하지 못한다. CLOCK_TOKENS를 비워 우회하지 않는다:\n{}",
            c.weak_only.len(),
            c.weak_only.join("\n")
        ));
    }

    // 프로세스당 한 번만 쓸 수도 있어 모두 위반으로 단정하지 않고 개수 변화를 검사한다.
    if c.recall_blind.len() != f.recall_blind {
        out.push(format!(
            "같은 프로세스의 재호출을 못 가르는 temp 경로가 {}곳이다(래칫 {}). 단조 카운터를 넣거나 의도한 프로세스당 공유라면 사유를 적어라. 이 값을 올려서 통과시키지 마라. 줄었으면 값을 같이 내려라. thread::current().id()를 더해도 같은 스레드의 재호출은 구분되지 않아 이 수는 안 줄어든다.\n[스레드 ID도 쓰는 경로] {}\n[전체 경로]\n{}",
            c.recall_blind.len(),
            f.recall_blind,
            if c.recall_blind_with_thread.is_empty() {
                "없다".to_string()
            } else {
                format!(
                    "{} 곳\n{}",
                    c.recall_blind_with_thread.len(),
                    c.recall_blind_with_thread.join("\n")
                )
            },
            c.recall_blind.join("\n")
        ));
    }
    if c.reasoned < f.reasoned {
        out.push(format!(
            "사유로 통과한 자리가 {}곳뿐이다(하한 {}). temp_path::tests의 a_korean_sayu_marker_also_passes 등 사유 인식 시험과 실제 주석 변경을 확인한다. 구분 성분으로 대체했는지, 사유가 삭제됐는지를 구별하고 하한을 갱신한다.",
            c.reasoned, f.reasoned
        ));
    }

    if !c.silent.is_empty() {
        out.push(format!(
            "공유 temp 아래 고정 이름 임시 경로가 {}곳 있다. 동시 실행이 같은 파일이나 디렉터리를 덮어쓸 수 있다(ADR-0045). 프로세스·호출을 구분하는 성분이나 TempDir을 사용하고, 의도한 공유라면 그 위치에 이유를 적는다:\n{}",
            c.silent.len(),
            c.silent
                .iter()
                .map(|s| format!("  {s}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    out
}

#[test]
fn every_temp_path_is_uniquified_or_reasoned() {
    let root = repo_root();
    let c = census(&root, SCAN_ROOTS);
    let v = verdicts(&c, &FLOORS);
    assert!(v.is_empty(), "{}", v.join("\n\n"));
}

/// 같은 파일에서 호출을 찾지 못한 판별자 전달 경로의 수다.
/// 2026-09-08의 0911d0113에서 10개 중 1개였다(DiskScrollback::new의 호출자는 다른 파일).
/// 호출은 마스킹한 코드에서, 판별자 문자열은 원문에서 읽는다. 호출 미발견은 안전 판정이 아니다.
const CHAINS_NOT_SEEN_RATCHET: usize = 1;

/// 같은 파일에서 찾은 임시 경로 헬퍼 호출들이 같은 판별자를 전달하는지 확인한다.
#[test]
fn no_two_callers_hand_the_same_discriminator_to_a_temp_path_helper() {
    let root = repo_root();
    let mut hits = Vec::new();
    let mut not_seen = Vec::new();
    for (rel, raw) in tasty_doc_guards::source_text::rust_sources(&root, SCAN_ROOTS) {
        let masked = mask_non_code(&raw);
        let code: Vec<&str> = masked.lines().collect();
        let rawl: Vec<&str> = raw.lines().collect();
        if code.len() != rawl.len() {
            continue;
        }
        for ch in discriminator_chains(&code, &rawl) {
            if ch.no_call_seen {
                not_seen.push(format!("{}: {}", rel.display(), ch.callee));
            }
            if !ch.duplicates.is_empty() {
                hits.push(format!(
                    "{}: {} 에 같은 판별자가 두 번 넘어간다 — {}",
                    rel.display(),
                    ch.callee,
                    ch.duplicates.join(" · ")
                ));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "임시 경로 헬퍼에 같은 판별자를 전달한 호출이 있다. 다른 호출 위치에는 다른 판별자를 쓰고, 같은 위치를 반복 호출한다면 경로에 단조 카운터를 넣는다. PID나 스레드 ID만으로 재호출을 구분할 수 없다. 이 검사는 실제 중복 인자를 찾았으므로 사유만 붙여 우회하지 않는다:\n{}",
        hits.join("\n")
    );
    assert_eq!(
        not_seen.len(),
        CHAINS_NOT_SEEN_RATCHET,
        "같은 파일에서 호출자를 찾지 못한 판별자 전달 경로가 {}개다(래칫 {}). 이 검사는 해당 호출 인자를 확인하지 못했다. 호출을 같은 파일에서 추적할 수 있게 하거나 헬퍼가 호출별 고유 성분을 직접 만들도록 검토한다. 수가 줄면 래칫도 함께 낮춘다:\n{}",
        not_seen.len(),
        CHAINS_NOT_SEEN_RATCHET,
        not_seen.join("\n")
    );
}

#[test]
fn a_repeated_discriminator_is_caught() {
    let src = "fn tmp_pack(tag: &str) -> PathBuf {\n    \
               let dir = std::env::temp_dir().join(format!(\"tasty-fontpack-{tag}-{}\", std::process::id()));\n    \
               dir\n}\n\
               #[test]\nfn a() { let dir = tmp_pack(\"valid\"); }\n\
               #[test]\nfn b() { let dir = tmp_pack(\"valid\"); }\n";
    let masked = mask_non_code(src);
    let code: Vec<&str> = masked.lines().collect();
    let rawl: Vec<&str> = src.lines().collect();
    let chains = discriminator_chains(&code, &rawl);
    let dups: Vec<&String> = chains.iter().flat_map(|c| &c.duplicates).collect();
    assert_eq!(dups.len(), 1, "겹치는 판별자를 못 잡았다 — 사슬 {chains:?}");
    assert!(dups[0].contains("valid"), "{dups:?}");
}

#[test]
fn distinct_discriminators_are_not_caught() {
    let src = "fn tmp_pack(tag: &str) -> PathBuf {\n    \
               let dir = std::env::temp_dir().join(format!(\"tasty-fontpack-{tag}-{}\", std::process::id()));\n    \
               dir\n}\n\
               #[test]\nfn a() { let dir = tmp_pack(\"valid\"); }\n\
               #[test]\nfn b() { let dir = tmp_pack(\"builtin\"); }\n";
    let masked = mask_non_code(src);
    let code: Vec<&str> = masked.lines().collect();
    let rawl: Vec<&str> = src.lines().collect();
    let chains = discriminator_chains(&code, &rawl);
    assert!(
        chains.iter().all(|c| c.duplicates.is_empty()),
        "다른 판별자를 겹친다고 했다 — {chains:?}"
    );
    assert!(
        !chains.is_empty(),
        "메서드 호출 체인을 찾지 못해 경로 판정을 확인할 수 없다"
    );
}

/// 실패 종류마다 올바른 조치와 경로를 안내하는지 검사한다.
/// 운영 하한과 독립된 합성 값을 써서 상수를 바꿔도 입력이 함께 바뀌지 않게 한다.
/// 진단 전체 대신 의미를 구분하는 문구를 검사한다.
#[cfg(test)]
mod wording {
    use super::{Floors, verdicts};
    use tasty_doc_guards::temp_path::Census;

    const F: Floors = Floors {
        files: 10,
        sites: 5,
        uniquified: 4,
        unpaired: 2,
        reasoned: 3,
        recall_blind: 1,
    };

    fn healthy() -> Census {
        Census {
            files_scanned: 10,
            sites: 5,
            uniquified: 4,
            reasoned: 3,
            unpaired: 2,
            silent: Vec::new(),
            weak_only: Vec::new(),
            recall_blind: vec![
                "a/b.rs:9: let p = temp_dir().join(format!(\"x-{}\", process::id()));".into(),
            ],
            recall_blind_with_thread: Vec::new(),
        }
    }

    fn only(c: &Census) -> String {
        let v = verdicts(c, &F);
        assert_eq!(v.len(), 1, "문구가 하나가 아니다: {v:#?}");
        v.into_iter()
            .next()
            .expect("바로 위에서 길이 1 을 단정했다")
    }

    #[test]
    fn a_census_at_the_floor_says_nothing() {
        assert!(verdicts(&healthy(), &F).is_empty());
    }

    #[test]
    fn a_short_file_count_hands_over_the_ls_files_discriminator() {
        let c = Census {
            files_scanned: 9,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("훑은 파일이"), "{m}");
        assert!(m.contains("git ls-files"), "{m}");
    }

    #[test]
    fn a_short_site_count_points_at_the_join_recognition_not_at_the_repo() {
        let c = Census {
            sites: 4,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("경로 짓는 temp_dir 자리를"), "{m}");
        assert!(m.contains("총 등장 수"), "{m}");
    }

    #[test]
    fn a_short_uniquified_count_names_the_token_fixture_to_run() {
        let c = Census {
            uniquified: 3,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("유니크화된 자리가"), "{m}");
        assert!(
            m.contains("every_recognized_uniquifier_is_actually_recognized"),
            "{m}"
        );
    }

    #[test]
    fn a_grown_unpaired_count_forbids_raising_the_ratchet() {
        let c = Census {
            unpaired: 3,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("검사에서 빠지는"), "{m}");
        assert!(m.contains("이 값만 올려서 통과시키지 마라"), "{m}");
    }

    /// 재호출 구분이 필요하면 카운터를, 의도한 공유라면 사유를 요구하는 두 조치를 모두 안내해야 한다.
    #[test]
    fn a_grown_recall_blind_count_offers_both_branches_and_forbids_raising() {
        let c = Census {
            recall_blind: vec!["a/b.rs:9: x".into(), "c/d.rs:2: y".into()],
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("재호출을 못 가르는"), "{m}");
        assert!(m.contains("단조 카운터"), "처방 (1) 이 없다: {m}");
        assert!(m.contains("사유를 적어라"), "처방 (2) 가 없다: {m}");
        assert!(
            m.contains("이 값을 올려서 통과시키지 마라"),
            "래칫을 푸는 것을 금지하는 문장이 없다: {m}"
        );
        assert!(m.contains("c/d.rs:2"), "어느 자리인지 안 적었다: {m}");
    }

    #[test]
    fn the_message_names_the_thread_split_and_denies_it_as_the_cure() {
        let c = Census {
            recall_blind: vec!["a/b.rs:9: x".into(), "c/d.rs:2: y".into()],
            recall_blind_with_thread: vec!["c/d.rs:2: y".into()],
            ..healthy()
        };
        let m = only(&c);
        assert!(
            m.contains("thread::current().id()") && m.contains("이 수는 안 줄어든다"),
            "스레드 ID로 재호출을 구분할 수 없다는 설명이 없다: {m}"
        );
        assert!(
            m.contains("[스레드 ID도 쓰는 경로]"),
            "스레드 ID를 쓰는 경로 목록이 없다: {m}"
        );
        assert!(
            m.contains("1 곳"),
            "스레드 ID도 쓰는 경로 수가 출력되지 않았다: {m}"
        );
    }

    #[test]
    fn the_thread_split_line_says_none_when_there_is_none() {
        let c = Census {
            recall_blind: vec!["a/b.rs:9: x".into(), "c/d.rs:2: y".into()],
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("[스레드 ID도 쓰는 경로] 없다"), "{m}");
    }

    #[test]
    fn a_shrunk_recall_blind_count_also_fires() {
        let c = Census {
            recall_blind: Vec::new(),
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("값을 같이 내려라"), "{m}");
    }

    #[test]
    fn a_shrunk_unpaired_count_also_fires() {
        let c = Census {
            unpaired: 1,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("검사에서 빠지는"), "{m}");
        assert!(m.contains("래칫도 낮춰 새 누락이 허용되지 않게"), "{m}");
    }

    #[test]
    fn a_clock_only_site_is_told_to_use_a_counter_not_to_add_a_pid() {
        let c = Census {
            weak_only: vec!["a/b.rs:7: let p = temp_dir().join(format!(\"x-{nanos}\"));".into()],
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("시계가 프로세스-내"), "{m}");
        assert!(m.contains("fetch_add"), "카운터 처방이 없다: {m}");
        assert!(
            m.contains("더하는 것은 처방이 아니다"),
            "pid 를 더하는 것이 처방이 아니라는 말이 없다: {m}"
        );
        assert!(m.contains("a/b.rs:7"), "좌표가 문구에 안 실렸다: {m}");
    }

    #[test]
    fn a_short_reasoned_count_names_the_marker_fixtures_to_run() {
        let c = Census {
            reasoned: 2,
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("사유로 통과한 자리가"), "{m}");
        assert!(m.contains("a_korean_sayu_marker_also_passes"), "{m}");
    }

    #[test]
    fn a_silent_fixed_name_gets_the_two_way_prescription_and_its_coordinate() {
        let c = Census {
            silent: vec!["c/d.rs:3: let p = temp_dir().join(\"tasty-cache\");".into()],
            ..healthy()
        };
        let m = only(&c);
        assert!(m.contains("고정 이름 임시 경로가"), "{m}");
        assert!(m.contains("ADR-0045"), "{m}");
        assert!(m.contains("c/d.rs:3"), "좌표가 문구에 안 실렸다: {m}");
    }
}

/// 소켓 키의 PID·카운터·시계 성분을 확인한다. PID는 프로세스를, 카운터는 같은 프로세스의 호출을 구분한다.
/// 시계는 재사용된 PID와 이전 프로세스의 잔여 경로를 구분하는 데 쓰인다. sticky /tmp에서는
/// 다른 사용자의 잔여 파일을 지우지 못할 수 있다. 이 검사는 성분의 존재만 보며 실제 충돌을 실행하지 않는다.
/// 성분을 없앨 때는 그 역할이 더 이상 필요 없는 이유를 검토해야 한다.
/// 주석에 쓴 성분 이름이 코드를 대신하지 않도록 마스킹한 코드만 검사한다.
#[test]
fn the_handle_socket_key_still_carries_all_three_components() {
    // 검사할 코드 위치와 범위는 대상 구현의 상수에서 가져오지 않는다.
    const OWNER: &str = "crates/tasty-host-plugin/src/handle_channel.rs";
    let path = repo_root().join(OWNER);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("읽을 수 없다: {} — {e}", path.display()));
    let masked = mask_non_code(&text);
    let lines: Vec<&str> = masked.lines().collect();
    let at = lines
        .iter()
        .position(|l| l.contains("let socket_path"))
        .unwrap_or_else(|| {
            panic!(
                "{OWNER}에서 let socket_path를 찾지 못했다. 함수 이동·개명을 확인하고 검사 위치를 갱신한다."
            )
        });
    let lo = at.saturating_sub(16);
    let hi = (at + 4).min(lines.len());
    let win = &lines[lo..hi];
    let has = |t: &str| win.iter().any(|l| l.contains(t));

    assert!(
        has("process::id"),
        "{OWNER}: socket 키에서 프로세스 ID가 사라졌다"
    );
    assert!(
        has("fetch_add"),
        "{OWNER}: socket 키에서 호출별 단조 카운터가 사라졌다. 시계만으로는 해상도 때문에 경로가 겹칠 수 있다."
    );
    assert!(
        has("SystemTime"),
        "{OWNER}: socket 키에서 시계 성분이 사라졌다. 재사용된 PID와 이전 프로세스의 잔여 경로를 구분하기 위한 값이다. sticky /tmp에서는 다른 사용자의 파일을 지우지 못할 수 있다. 실수라면 복원하고, 의도한 변경이면 이 역할이 불필요한 근거와 함께 검사를 갱신한다."
    );
}
