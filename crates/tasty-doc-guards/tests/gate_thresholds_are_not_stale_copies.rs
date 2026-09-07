//! 게이트 임계 둘(cognitive **20** · 파일 SLOC **1000**)을 주장하는 자리가 소스의 값과
//! 같은지 본다.
//!
//! 두 값의 정본은 각각 `clippy.toml` 의 `cognitive-complexity-threshold` 와
//! `scripts/check-file-size.sh` 의 `THRESHOLD` 다. 둘 다 기계가 읽는 값인데, 그것을
//! 평문으로 다시 적은 자리가 여럿이고 어느 것도 파생되지 않는다. 한쪽만 고치면
//! 게이트는 새 값으로 돌고 문서는 옛 값을 말한다 — **둘 다 초록이다.**
//!
//! ## 좌변이 집합이 아니라 스칼라다 — 그래서 "양방향" 의 형태가 다르다
//!
//! `workspace_lint_table_matches_the_manifest` 의 좌변은 (lint, 레벨) **집합**이라
//! 양방향이 곧 집합 상등이었고, 거기서 값진 것은 **부분 사본**(빠진 행)을 잡는 방향이었다.
//! 여기는 우변이 스칼라 하나라 "빠진 행" 이라는 것이 없다. 대신 실패 형태가 둘이다.
//!
//! - **낡은 사본** — 주장 자리가 옛 값을 든다. 소스 값으로 각 자리를 물어 잡는다.
//! - **미분류 신규 자리** — 새 문서가 그 값을 주장하기 시작하는데 아무도 모른다.
//!   이쪽이 조용하다. 그래서 **명부 완전성** 축을 둔다: 레포 전체를 과포함으로 훑어
//!   그 값을 게이트 어휘와 함께 든 파일을 모으고, **전부 명부 아니면 제외목록 안**이어야
//!   한다. 분류되지 않은 파일이 하나라도 있으면 실패다.
//!
//! ## 좌변 선별 — "현재 상태를 주장하는 수" 만 든다
//!
//! `complexity_allowlist_docs_parity` 가 이미 그은 선을 그대로 쓴다: **현재 상태를
//! 주장하는 수**와 **시점 측정·결정 근거·예시·인용**은 다르다. 뒤엣것은 지금 값과
//! 갈라지는 것이 정상이라 대조 대상이 아니다.
//!
//! 실측 선별(2026-09-08). 낱말 경계 토큰 전수 → 게이트 어휘 ±2 줄 창 통과 → 손 분류:
//!
//! ```text
//!            전수    어휘창    주장(명부)   제외
//!   20        443       21          5        15   (+ 정본 1)
//!   1000      320       54          4        49   (+ 정본 1)
//! ```
//!
//! 제외의 갈래는 셋이고, 각 항목에 사유를 붙여 아래 `EXCLUDED` 에 적어 둔다 —
//! ① 결정 기록·인용(ADR 본문은 결정 시점의 값을 남긴다) ② 예시로 든 수
//! ③ 같은 토큰의 다른 뜻(webhook `threshold: 20` · `wal_autocheckpoint 1000` 등).
//!
//! ## 명부는 줄 번호가 아니라 파일이다
//!
//! 좌표를 줄로 잡으면 문단이 하나 늘 때마다 명부가 낡는다(`line_number_citations_do_not_grow`
//! 가 같은 이유로 줄 인용을 래칫한다). 파일 단위로 잡고, 그 파일이 **현재 값을 게이트
//! 어휘와 함께 들고 있는가**로 판정한다.
//!
//! ## 못 잡는 것 (사전 등록)
//!
//! - 한 파일이 그 값을 **두 번** 주장하는데 한 자리만 낡은 경우. 이 판정은 파일 단위라
//!   나머지 한 자리가 현재 값이면 통과한다. 줄 단위로 내리려면 명부가 줄이 되고, 그러면
//!   위의 낡음 문제가 돌아온다 — 의도적 교환이다.
//! - 게이트 어휘 없이 그 값만 적은 주장. 그런 자리는 읽는 사람도 무엇의 임계인지 모른다.
//! - 값을 **범위**로 적은 서술("1000 안팎"). 토큰 일치만 본다.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// 정본 — (파일, 값이 뒤에 붙는 접두).
const COGNITIVE_SOURCE: (&str, &str) = ("clippy.toml", "cognitive-complexity-threshold = ");
const SLOC_SOURCE: (&str, &str) = ("scripts/check-file-size.sh", "THRESHOLD=");

/// 게이트 어휘 — 이 중 하나가 ±2 줄 창에 있어야 그 자리를 "이 게이트에 대한 언급" 으로 센다.
const COGNITIVE_VOCAB: &[&str] = &[
    "cognitive",
    "복잡도",
    "threshold",
    "임계",
    "clippy.toml",
    "complexity",
];
const SLOC_VOCAB: &[&str] = &[
    "SLOC",
    "sloc",
    "tokei",
    "check-file-size",
    "상한",
    "임계",
    "복잡도",
    "complexity",
    "allowlist",
    "THRESHOLD",
];

/// cognitive 임계를 **현재 상태로 주장하는** 파일.
const COGNITIVE_CLAIMS: &[&str] = &[
    "Cargo.toml",
    "clippy.toml",
    "crates/tasty-cli/src/request/agent.rs",
    "docs/dev-guide/clippy-policy.md",
    "docs/dev-guide/complexity-gate.md",
];

/// 파일 SLOC 임계를 **현재 상태로 주장하는** 파일.
const SLOC_CLAIMS: &[&str] = &[
    ".github/workflows/complexity-check.yml",
    "docs/dev-guide/clippy-policy.md",
    "docs/dev-guide/complexity-gate.md",
    "scripts/check-file-size.sh",
];

/// 어휘창은 통과하지만 **현재 상태 주장이 아닌** 자리 — 사유를 함께 적는다.
///
/// 사유가 없으면 다음 사람이 "왜 여기만 빠졌나" 를 다시 판정해야 하고, 그 재판정은
/// 매번 같은 답을 안 낸다.
const EXCLUDED: &[(&str, &str)] = &[
    // ── ① 결정 기록·인용 — ADR 본문은 결정 시점의 값을 남긴다.
    (
        "docs/adr/0037-complexity-gate.md",
        "두 임계를 결정한 ADR. 결정문의 값이라 현재 값과 갈리면 새 ADR 을 쓴다",
    ),
    (
        "docs/adr/0131-file-sloc-gate-needs-a-firing-trigger.md",
        "도입 시점 측정(초과 0 건)과 대안 D 의 인용",
    ),
    (
        "docs/adr/0165-the-file-sloc-gate-measures-shipped-lines.md",
        "대안 A 기각 사유에서 임계를 인용",
    ),
    (
        "docs/adr/0168-the-file-sloc-threshold-is-not-derived-and-the-freeze-ratchets-one-way.md",
        "임계의 유도 부재를 다루는 ADR — 분포표·발화율 곡선의 축 값이라 시점 측정이다",
    ),
    ("docs/adr/index.md", "ADR 제목의 인용"),
    (
        "docs/adr/0124-blank-value-rule-is-load-path-independent.md",
        "과거 사건 서술 — 그때 게이트를 넘었다",
    ),
    (
        "crates/tasty-design-tokens/src/dtcg/duration_accessor.rs",
        "과거 사건 서술 — 파일을 가른 이유",
    ),
    (
        "src/source_guards/sloc_gate_skip_proxy.rs",
        "ADR-0168 의 결론을 비유로 인용",
    ),
    (
        "scripts/check-frozen-sum-ratchet.sh",
        "옛 어긋남 사건의 기록",
    ),
    // ── ② 예시로 든 수.
    (
        "docs/adr/0139-numbers-in-docs-are-classified-by-lineage-not-by-name.md",
        "수의 계보 분류 ADR 이 두 임계를 예시로 든다",
    ),
    // ── ③ 같은 토큰의 다른 뜻.
    (
        "docs/adr/0119-agent-semaphore-resize-and-holder-expiry.md",
        "20분 — 시간",
    ),
    (
        "docs/features/webhook/index.md",
        "webhook 남용 차단 threshold 기본 20",
    ),
    (
        "src/webhook/abuse.rs",
        "webhook 남용 차단 threshold 기본 20",
    ),
    ("src/app/event_handler.rs", "20여개 variant — 개수"),
    (
        "docs/adr/0091-render-stall-watchdog-observation-only.md",
        "wgpu FRAME_TIMEOUT_MS = 1000",
    ),
    (
        "docs/adr/0108-egui-mesh-scroll-delivered-in-one-pass.md",
        "egui scroll points_per_second 1000",
    ),
    (
        "docs/design/systems/memory.md",
        "SQLite wal_autocheckpoint 1000 페이지",
    ),
    (
        "docs/design/systems/storage.md",
        "SQLite wal_autocheckpoint 1000 페이지",
    ),
    ("src/db.rs", "SQLite wal_autocheckpoint 1000 페이지"),
    (
        "crates/tasty-memory/src/lib.rs",
        "SQLite wal_autocheckpoint 1000 페이지",
    ),
    ("crates/tasty-memory/src/tests.rs", "테스트 픽스처의 행 수"),
    (
        "crates/tasty-plugin-agent-stream/src/handlers.rs",
        "POLL_MAX_LIMIT = 1000",
    ),
    (
        "crates/tasty-plugin-manifest/src/types.rs",
        "HOOK_TIMEOUT_MS_MAX = 1000",
    ),
    (
        "crates/tasty-telemetry/src/anomaly.rs",
        "CALL_BURST_THRESHOLD = 1000",
    ),
    ("src/view/main/redraw.rs", "재시도 상한 1000"),
    // ── 이 가드 자신. 명부와 사유를 본문에 들고 있어 스캔에 걸린다.
    (
        "crates/tasty-doc-guards/tests/gate_thresholds_are_not_stale_copies.rs",
        "이 가드 자신의 명부·사유·모듈 주석",
    ),
];

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("읽을 수 없다: {} — {e}", path.display()))
}

/// 정본에서 값을 읽는다. 못 읽으면 **판정 불가로 죽는다** — 0 이나 빈 값을 돌려주면
/// 아래 모든 대조가 조용히 통과한다.
fn source_value(source: (&str, &str)) -> String {
    let (file, prefix) = source;
    let text = read(file);
    let hits = text.match_indices(prefix).count();
    assert_eq!(
        hits, 1,
        "`{file}` 에서 `{prefix}` 가 {hits} 번 나온다 — 정확히 1 번이어야 정본을 읽을 수 있다"
    );
    let rest = &text[text.find(prefix).expect("위에서 1 번을 확인했다") + prefix.len()..];
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    assert!(
        !digits.is_empty(),
        "`{file}` 의 `{prefix}` 뒤에 숫자가 없다 — 형태가 바뀌었으면 이 판독기를 고쳐라"
    );
    digits
}

/// 낱말 경계 토큰인가 — `2026` 이나 `1000ms` 를 `20`/`1000` 으로 세지 않기 위해서다.
///
/// 순수 함수다 — 파일 없이 변이로 찌를 수 있어야 한다.
fn claims_value(text: &str, value: &str, vocab: &[&str]) -> bool {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !has_bare_token(line, value) {
            continue;
        }
        let lo = i.saturating_sub(2);
        let hi = (i + 3).min(lines.len());
        if lines[lo..hi]
            .iter()
            .any(|w| vocab.iter().any(|v| w.contains(v)))
        {
            return true;
        }
    }
    false
}

/// `line` 안에 `token` 이 낱말 경계로 나오는가.
///
/// `.` 을 무조건 경계 밖으로 두면 **문장 끝의 `1000.` 을 못 센다** — 이 판독기가 처음
/// 그랬고 변이 `the_vocabulary_window_reaches_two_lines` 가 그것을 물었다. 그렇다고
/// 경계로 두면 `1.1000.0` 을 센다. 그래서 `.` 은 **숫자에 붙어 있을 때만** 경계 밖이다.
fn has_bare_token(line: &str, token: &str) -> bool {
    let alnum = |c: char| c.is_ascii_alphanumeric() || c == '_';
    line.match_indices(token).any(|(at, _)| {
        let head = &line[..at];
        let tail = &line[at + token.len()..];
        let before = match head.chars().next_back() {
            Some(c) if alnum(c) => true,
            // `1.1000` — 앞의 `.` 이 숫자에 붙어 있으면 그 수의 일부다.
            Some('.') => head[..head.len() - 1]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_digit()),
            _ => false,
        };
        let after = match tail.chars().next() {
            Some(c) if alnum(c) => true,
            Some('.') => tail[1..].chars().next().is_some_and(|c| c.is_ascii_digit()),
            _ => false,
        };
        !before && !after
    })
}

/// 추적 파일 중 텍스트로 훑을 것.
fn tracked_text_files() -> Vec<String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo_root())
        .args(["ls-files"])
        .output()
        .unwrap_or_else(|e| panic!("`git ls-files` 를 실행할 수 없다 — {e}"));
    assert!(out.status.success(), "`git ls-files` 가 실패했다");
    let exts = [".md", ".rs", ".toml", ".sh", ".yml", ".yaml"];
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|f| exts.iter().any(|e| f.ends_with(e)))
        .map(str::to_string)
        .collect()
}

/// 어휘창을 통과한 파일 전체(과포함 스캔).
fn mentioning_files(value: &str, vocab: &[&str]) -> BTreeSet<String> {
    let root = repo_root();
    let mut out = BTreeSet::new();
    let files = tracked_text_files();
    assert!(
        files.len() > 500,
        "추적 텍스트 파일을 {} 개만 읽었다 — 모수가 무너지면 미분류 0 도 초록이 된다",
        files.len()
    );
    for rel in files {
        let path = root.join(rel.split('/').collect::<PathBuf>());
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        if claims_value(&text, value, vocab) {
            out.insert(rel);
        }
    }
    out
}

fn check(label: &str, source: (&str, &str), claims: &[&str], vocab: &[&str]) -> Vec<String> {
    let value = source_value(source);
    let mut wrong = Vec::new();

    // ① 낡은 사본 — 명부의 각 자리가 지금 값을 드는가.
    for rel in claims {
        if !claims_value(&read(rel), &value, vocab) {
            wrong.push(format!(
                "  {rel}: {label} 임계를 주장하는 자리인데 지금 값 `{value}` 을 안 든다 \
                 — 정본은 `{}` 이다. 그 자리를 고쳐라",
                source.0
            ));
        }
    }

    // ② 미분류 신규 자리 — 스캔 생존자가 전부 명부/제외 안인가.
    let known: BTreeSet<&str> = claims
        .iter()
        .copied()
        .chain(EXCLUDED.iter().map(|(f, _)| *f))
        .collect();
    for rel in mentioning_files(&value, vocab) {
        if !known.contains(rel.as_str()) {
            wrong.push(format!(
                "  {rel}: {label} 임계값 `{value}` 을 게이트 어휘와 함께 드는데 분류가 없다. \
                 현재 상태를 주장하면 명부(`*_CLAIMS`)에, 시점 측정·인용·다른 뜻이면 \
                 사유와 함께 `EXCLUDED` 에 넣어라"
            ));
        }
    }
    wrong
}

#[test]
fn the_two_gate_thresholds_have_no_stale_copies() {
    let mut wrong = check(
        "cognitive",
        COGNITIVE_SOURCE,
        COGNITIVE_CLAIMS,
        COGNITIVE_VOCAB,
    );
    wrong.extend(check("파일 SLOC", SLOC_SOURCE, SLOC_CLAIMS, SLOC_VOCAB));
    assert!(
        wrong.is_empty(),
        "게이트 임계값의 사본이 정본과 갈렸거나, 분류되지 않은 주장 자리가 생겼다.\n\
         정본: `{}` 의 `{}` = {} · `{}` 의 `{}` = {}\n{}",
        COGNITIVE_SOURCE.0,
        COGNITIVE_SOURCE.1.trim_end_matches(" = "),
        source_value(COGNITIVE_SOURCE),
        SLOC_SOURCE.0,
        SLOC_SOURCE.1.trim_end_matches('='),
        source_value(SLOC_SOURCE),
        wrong.join("\n")
    );
}

/// 판정기가 실제로 무는지 확인하는 변이 — 파일은 안 고친다.
mod threshold_mutations {
    use super::*;

    #[test]
    fn a_stale_copy_is_caught() {
        let doc = "임계 20 이다.\n";
        assert!(claims_value(doc, "20", COGNITIVE_VOCAB));
        assert!(
            !claims_value(doc, "25", COGNITIVE_VOCAB),
            "옛 값을 새 값으로 셌다"
        );
    }

    #[test]
    fn a_number_without_gate_vocabulary_is_not_a_claim() {
        assert!(!claims_value("포트 1000 을 연다.\n", "1000", SLOC_VOCAB));
    }

    #[test]
    fn the_vocabulary_window_reaches_two_lines() {
        let md = "파일 SLOC 상한을 말한다.\n중간 줄.\n값은 1000.\n";
        assert!(claims_value(md, "1000", SLOC_VOCAB));
        let far = "파일 SLOC 상한을 말한다.\n한 줄.\n두 줄.\n세 줄.\n값은 1000.\n";
        assert!(!claims_value(far, "1000", SLOC_VOCAB), "창 밖까지 셌다");
    }

    #[test]
    fn a_glued_token_is_not_counted() {
        assert!(!has_bare_token("연도는 2026 이다", "20"));
        assert!(
            has_bare_token("임계는 1000.", "1000"),
            "문장 끝의 마침표를 경계로 안 봤다"
        );
        assert!(!has_bare_token("타임아웃 1000ms", "1000"));
        assert!(!has_bare_token("버전 1.1000.0", "1000"));
        assert!(has_bare_token("임계 1000 이다", "1000"));
    }

    #[test]
    fn every_excluded_entry_carries_a_reason() {
        for (file, why) in EXCLUDED {
            assert!(!why.trim().is_empty(), "{file} 의 제외 사유가 비었다");
        }
    }

    #[test]
    fn no_file_is_both_claimed_and_excluded() {
        let excluded: BTreeSet<&str> = EXCLUDED.iter().map(|(f, _)| *f).collect();
        for rel in COGNITIVE_CLAIMS.iter().chain(SLOC_CLAIMS) {
            assert!(
                !excluded.contains(rel),
                "{rel} 가 명부와 제외목록에 둘 다 있다"
            );
        }
    }
}
