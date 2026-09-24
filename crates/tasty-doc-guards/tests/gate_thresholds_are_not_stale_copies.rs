//! 설정과 스크립트에 정의한 검사 임계값을 문서·주석의 현재 값과 대조한다.
//!
//! 등록한 파일에서는 현재 값을 적은 줄 수까지 비교해 한 사본만 낡는 경우를 잡는다.
//! 저장소 전체에서 값과 관련 어휘를 함께 찾고 새로 발견한 파일도 분류하도록 요구한다.
//! 과거 측정, 예시, 같은 숫자의 다른 의미는 이유를 적어 제외한다.
//!
//! 어휘 검색은 의미를 완전히 해석하지 못하므로 넓은 검색어와 앞뒤 두 줄을 유지한다.
//! 예외 파일이 사라지거나 분류 이유가 맞지 않게 되면 예외 목록도 수정해야 한다.
//! 수치 기록 원칙은 `docs/documentation-model.md`, 운영 기준은
//! `docs/dev-guide/complexity-gate.md`를 따른다.

use std::collections::BTreeSet;
use std::path::PathBuf;

/// 정본 — (파일, 값이 뒤에 붙는 접두).
const COGNITIVE_SOURCE: (&str, &str) = ("clippy.toml", "cognitive-complexity-threshold = ");
const SLOC_SOURCE: (&str, &str) = ("scripts/check-file-size.sh", "THRESHOLD=");
const SHARED_WALK_SOURCE: (&str, &str) = ("scripts/check-shared-walk-ratchet.sh", "CAP=");
const ALLOW_REASON_SOURCE: (&str, &str) = ("scripts/check-allow-reason.sh", "CAP=");
const FROZEN_SUM_SOURCE: (&str, &str) = (".complexity-file-allowlist", "# frozen-sum-budget: ");
const DOC_BIAS_SOURCE: (&str, &str) = (".complexity-file-allowlist", "# doc-comment-bias: ");

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
const SHARED_WALK_VOCAB: &[&str] = &[
    "read_dir",
    "순회",
    "래칫",
    "상한",
    "임계",
    "shared-walk",
    "shared_walk",
    "CAP",
];
const ALLOW_REASON_VOCAB: &[&str] = &[
    "allow",
    "억제",
    "사유",
    "래칫",
    "상한",
    "임계",
    "allow-reason",
    "allow_reason",
    "CAP",
];
const FROZEN_SUM_VOCAB: &[&str] = &[
    "frozen-sum-budget",
    "frozen_sum",
    "예산",
    "동결",
    "총합",
    "래칫",
    "budget",
];
/// 계측 편향의 어휘.
///
/// ★ **`tokei` 를 일부러 안 넣었다** — 다른 게이트에서는 그 낱말이 recall 을 벌지만
/// 여기서는 값이 `25` 라 흔하고, 그 낱말 하나로 `complexity-gate.md` 의 "25% 큰 파일"
/// 이 창을 통과한다(실측 2026-09-09). 그 파일은 다른 두 게이트의 **명부**에 있어
/// `EXCLUDED` 로 뺄 수가 없고(`no_file_is_both_claimed_and_excluded`), 명부에 넣으면
/// 편향이 움직인 날 "지금 값이 아예 없다 — 그 자리를 고쳐라" 가 **백분율 문장**을
/// 가리킨다. 값이 흔할수록 어휘는 그 개념에 붙은 낱말만 든다.
const DOC_BIAS_VOCAB: &[&str] = &["doc-comment-bias", "doc 주석", "누락", "편향", "bias"];

/// cognitive 임계를 **현재 상태로 주장하는** 파일과, 그 파일이 값을 든 **줄 수**.
///
/// 둘째 칸이 이 명부의 요점이다 — 자세한 것은 [`Claim`].
const COGNITIVE_CLAIMS: &[Claim] = &[
    ("Cargo.toml", 1),
    // 7 = rca 등가(시점 서술) · 10 = 정본
    ("clippy.toml", 2),
    ("docs/dev-guide/clippy-policy.md", 1),
    // 검사 기준 표의 현재 임계값.
    ("docs/dev-guide/complexity-gate.md", 1),
];

/// 파일 SLOC 임계를 **현재 상태로 주장하는** 파일과, 그 파일이 값을 든 **줄 수**.
const SLOC_CLAIMS: &[Claim] = &[
    (".github/workflows/complexity-check.yml", 1),
    // 두 lane 이 이 게이트의 시험을 넓히며 임계를 주석으로 인용했다.
    // 인용이 아니라 **주장**으로 분류한다 — 그 시험은 500 줄 프로브가 임계 아래라는
    // 전제로 초록을 단정하므로, 임계가 500 아래로 내려가면 주석만이 아니라 시험이
    // 틀린다. 그러니 정본이 움직일 때 이 자리가 함께 빨개지는 것이 옳다.
    ("tests/file_sloc_gate_fails_loudly.rs", 1),
    ("docs/dev-guide/clippy-policy.md", 1),
    // 기준 표, 임계값 선택 설명, 재검토 조건.
    ("docs/dev-guide/complexity-gate.md", 3),
    // 3 = 머리 주석 · 14 = ADR 가리킴 · 27 = 정본
    ("scripts/check-file-size.sh", 3),
];

/// 공용 순회 래칫의 상한을 **현재 상태로 주장하는** 파일과, 그 파일이 값을 든 **줄 수**.
const SHARED_WALK_CLAIMS: &[Claim] = &[
    // 이력의 마지막 줄(시점 서술) + 정본
    ("scripts/check-shared-walk-ratchet.sh", 2),
    // 상한을 바꿔치기하려고 정본 줄의 철자를 든다 — 값이 움직이면 그 치환이 죽는다
    ("tests/shared_walk_gate.rs", 2),
];

/// 사유 없는 `#[allow]` 래칫의 상한을 **현재 상태로 주장하는** 파일과 그 **줄 수**.
const ALLOW_REASON_CLAIMS: &[Claim] = &[
    // 231 = 정본 하나뿐이다. 48·198·199·226·228 도 이 수를 들지만 전부 **날짜와 트리를
    // 박은 계보/시점 서술**이라(`실측 2026-09-08 … base 12bc0f4b2`, `184 → 183 → 182`)
    // 상한이 182 → 181 로 내려가도 그 줄들은 그대로 참이다 — 지금 값으로 고치면 없던
    // 거짓이 새로 생긴다(이 가드의 `TIME_NOTE` 가 시키는 처방). 그래서 좌변이 6 → 1 이다.
    ("scripts/check-allow-reason.sh", 1),
    ("tests/allow_reason_gate.rs", 2),
];

/// 동결 총합 래칫의 **예산**을 현재 상태로 주장하는 파일과 그 **줄 수**.
///
/// 지금 하나뿐이다 — 정본 그 자신. 그 사실이 이 항목을 등록하는 이유다: 사본이 0 인
/// 지금이야말로 명부를 세울 수 있는 때이고, 사본이 생긴 뒤에는 그것이 정본인지 사본인지를
/// 다시 판정해야 한다. 예산은 되돌아 올라가지 않으므로 낡은 사본이 가장 비싼 값이다.
const FROZEN_SUM_CLAIMS: &[Claim] = &[(".complexity-file-allowlist", 1)];

/// 계측 편향의 **고정값**을 현재 상태로 주장하는 파일과 그 **줄 수**.
///
/// 정본 그 자신뿐이다. 이 값은 게이트가 매번 재어 여유 0 으로 고정하므로 산문이 그것을
/// 복제하면 편향이 움직인 날 그 산문이 조용히 거짓이 된다 — 그래서 산문 쪽은 값을 지우고
/// 정본을 가리키게 고쳤다(`complexity-gate.md`). 남은 인용은 전부 시점 측정이라
/// `EXCLUDED` 에 있다.
const DOC_BIAS_CLAIMS: &[Claim] = &[(".complexity-file-allowlist", 1)];

/// 명부 한 항목 — (레포 상대 경로, 그 파일이 임계값을 든 **줄 수**).
///
/// ## 왜 줄 수인가 (2026-09-08)
///
/// 이 판정은 오래 **파일 단위**였다. "그 파일이 현재 값을 게이트 어휘와 함께 드는가"
/// 하나만 물었고, 그래서 **한 파일이 값을 두 번 드는데 한 자리만 낡은 경우**를 못
/// 잡았다. 그 구멍은 모듈 주석에 "못 잡는 것" 으로 **사전 등록돼 있었고**, 교환의
/// 근거는 "줄 단위로 내리면 명부가 줄이 되고 줄 번호는 낡는다" 였다.
///
/// **그 교환은 '줄 단위 = 줄 번호' 라는 전제 위에 있었다.** 줄 수는 그 전제 밖이다 —
/// 좌표를 안 들어서 줄이 밀려도 안 낡고, 그러면서 파일 안의 사본을 전부 센다.
///
/// 구멍은 실측으로 증명했다(변이 E): 정본을 안 건드리고
/// `docs/dev-guide/complexity-gate.md:24` 한 줄만 `1000` → `1500` 으로 바꿨더니
/// 나머지 세 줄이 `1000` 을 들어 가드가 **rc 0** 으로 통과했다. 그 rc 0 이 결과였다.
///
/// ## 비용
///
/// 그 파일에 임계 사본이 늘거나 줄면 이 수를 갱신해야 한다. 그것이 이 설계의 값이다 —
/// 갱신하려면 그 파일의 임계 사본을 **전부 다시 봐야** 하고, 이 가드가 사려는 것이
/// 바로 그 행동이다(`EXCLUDED` 가 자라는 것을 비용으로 받아들인 것과 같은 성격).
type Claim = (&'static str, usize);

/// 제외의 갈래. **주석 절이 아니라 값이다.**
///
/// 한때 이 갈래는 `EXCLUDED` 안의 `// ── ①` 주석 절로만 있었다. 그러면 갈래를 읽으려면
/// 사람이 목록을 눈으로 갈라야 하고, **갈래마다 다른 판정을 붙일 수가 없다** — 판정이
/// 읽을 수 있는 것은 값뿐이다. 항목을 더하는 사람이 갈래를 고르게 만드는 효과도 있다:
/// 주석 절은 새 항목을 아무 절 끝에나 붙여도 조용하지만, 값은 하나를 고르게 한다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    /// 결정 기록·인용 — ADR 본문·CHANGELOG 는 **결정 시점의 값**을 남긴다. 정본이
    /// 바뀌어도 안 고치는 자리라 낡은 사본이 남는 것이 정상이다.
    Dated,
    /// 같은 토큰의 다른 뜻 — webhook `threshold: 20` · `wal_autocheckpoint 1000` 등.
    OtherMeaning,
    /// 이 가드 자신 — 명부와 사유를 본문에 들고 있어 스캔에 걸린다.
    SelfRef,
}

/// 어휘창은 통과하지만 **현재 상태 주장이 아닌** 자리 — 사유를 함께 적는다.
///
/// 사유가 없으면 다음 사람이 "왜 여기만 빠졌나" 를 다시 판정해야 하고, 그 재판정은
/// 매번 같은 답을 안 낸다.
const EXCLUDED: &[(&str, Kind, &str)] = &[
    (
        "docs/adr/0047-ci-and-complexity-checks.md",
        Kind::Dated,
        "임계값을 품질 공식에서 도출하지 않았다는 결정 당시의 선택 근거",
    ),
    (
        "crates/tasty-design-tokens/src/dtcg/duration_accessor.rs",
        Kind::Dated,
        "과거 사건 서술 — 파일을 가른 이유",
    ),
    (
        "src/source_guards/sloc_gate_skip_proxy.rs",
        Kind::Dated,
        "ADR-0047 의 결론을 비유로 인용",
    ),
    (
        "scripts/check-frozen-sum-ratchet.sh",
        Kind::Dated,
        "옛 어긋남 사건의 기록 · 계측 편향의 시점 재측정",
    ),
    (
        "CHANGELOG.md",
        Kind::Dated,
        "릴리스 시점의 서술 — 파일 전체가 \"그때 무엇이 바뀌었나\" 라서 현재 상태 주장이 아니다. 그래서 파일 단위로 뺀다",
    ),
    (
        "docs/features/webhook/index.md",
        Kind::OtherMeaning,
        "webhook 남용 차단 threshold 기본 20",
    ),
    (
        "src/webhook/abuse.rs",
        Kind::OtherMeaning,
        "webhook 남용 차단 threshold 기본 20",
    ),
    (
        "src/app/event_handler.rs",
        Kind::OtherMeaning,
        "20여개 variant — 개수",
    ),
    (
        "src/adapters/ui/popup/command_palette.rs",
        Kind::OtherMeaning,
        "목록이 상한에 걸린 뒤 높이가 안 움직이는지 보는 시험의 항목 수 1000 — 줄 수가 아니다",
    ),
    (
        "docs/design/systems/memory.md",
        Kind::OtherMeaning,
        "SQLite wal_autocheckpoint 1000 페이지",
    ),
    (
        "crates/tasty-memory/src/lib.rs",
        Kind::OtherMeaning,
        "SQLite wal_autocheckpoint 1000 페이지",
    ),
    (
        "crates/tasty-memory/src/tests.rs",
        Kind::OtherMeaning,
        "테스트 픽스처의 행 수",
    ),
    (
        "crates/tasty-plugin-agent-stream/src/handlers.rs",
        Kind::OtherMeaning,
        "POLL_MAX_LIMIT = 1000",
    ),
    (
        "crates/tasty-plugin-manifest/src/types.rs",
        Kind::OtherMeaning,
        "HOOK_TIMEOUT_MS_MAX = 1000",
    ),
    (
        "crates/tasty-telemetry/src/anomaly.rs",
        Kind::OtherMeaning,
        "CALL_BURST_THRESHOLD = 1000",
    ),
    (
        "src/view/main/redraw.rs",
        Kind::OtherMeaning,
        "재시도 상한 1000",
    ),
    (
        "crates/tasty-doc-guards/tests/gate_thresholds_are_not_stale_copies.rs",
        Kind::SelfRef,
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
    parse_source_value(file, prefix, &read(file))
}

/// 이미 읽어 온 정본 텍스트에서 값을 판독한다 — 파일시스템을 안 읽는다.
///
/// `source_value` 에서 갈라 둔 이유는 이 두 실패문(`정확히 1 번` · `숫자가 없다`)이
/// **합성 입력으로 재질 수 있어야** 하기 때문이다. 정본을 실제로 망가뜨려
/// 문구를 읽어 볼 수는 없다 — 그러면 레포의 게이트가 통째로 멈춘다.
///
/// ## 이 판독기 하나에 판정 셋이 매달려 있다 (실측 2026-09-08, 병합 트리 592/0 기준)
///
/// 변이 넷으로 갈랐다. ✕ 가 죽은 것이다.
///
/// | 변이 | ① 파서 픽스처 | ② 사본 판정 | ③ 죽은 면제 |
/// |---|---|---|---|
/// | 이 함수의 값 추출을 한 글자 민다 | ✕ | ✕ | ✕ |
/// | 정본(`clippy.toml`) 값만 20 → 21 | 산다 | ✕ | ✕ |
/// | `EXCLUDED` 에 값을 안 드는 파일 추가 | 산다 | 산다 | ✕ |
/// | 명부의 사본 수만 2 → 3 | 산다 | ✕ | 산다 |
///
/// ① `judgment_wording::a_well_formed_source_line_yields_the_value`
/// ② `the_two_gate_thresholds_have_no_stale_copies`
/// ③ `no_exclusion_is_dead_weight` (병합 트리에만 있다)
///
/// **셋은 곁가지가 아니다** — 뒤 두 변이가 ②③ 을 하나씩만 죽인다. 서로 다른 물음이다.
/// 그런데 **셋 다 이 함수 하나를 거쳐 값을 얻는다.** 그래서 읽는 법이 이렇다:
///
/// - 셋이 **함께** 빨개지면 원인은 이 판독기 하나다. 세 자리를 각각 고치려 들지 마라.
/// - 하나만 빨개지면 그 자리 고유의 회귀다.
/// - ① 은 디스크를 안 읽는다(합성 리터럴이 입력이다). 정본이 사라져도 답하는 **유일한**
///   판사라, "판독기가 옳은데 좌변이 비었다" 와 "판독기가 틀렸다" 를 갈라 준다.
fn parse_source_value(file: &str, prefix: &str, text: &str) -> String {
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
    assert!(
        out.status.success(),
        "`git ls-files` 가 실패했다: {}",
        String::from_utf8_lossy(&out.stderr).trim()
    );
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
    if let Some(note) = scan_floor_note(files.len()) {
        panic!("{note}");
    }
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

/// 게이트의 **좁은 주제어** — 이 게이트 말고 다른 뜻으로 읽힐 수 없는 낱말.
///
/// 어휘(`*_VOCAB`)와 목적이 반대다. 어휘는 **recall** 을 위해 넓다(`threshold`·`임계`)
/// — 그래서 webhook 의 `threshold: 20` 도 어휘창을 통과하고, 그것이 `EXCLUDED` 가
/// 필요한 이유다. 주제어는 **precision** 을 위해 좁다: 이 낱말이 값 옆에 있으면 그
/// 자리는 다른 뜻일 수가 없다.
/// ★ **린트 이름은 주제어가 아니다.** 첫 판에 넣었다가 실측으로 뺐다:
/// `src/app/event_handler.rs` 의 `App::user_event` 는 그 린트를 그 자리에서 끄면서 같은
/// 줄 사유 주석에 "20여개 variant" 를 달고 있어, 린트 이름을 주제어로 두면 값 옆에
/// 주제어가 선다. 그 자리는 임계에 대한 주장이 아니라 **린트를 끄는 것**이고, 처방을
/// 따라가 보면 둘 다 나쁘다 — 명부로 옮기면 variant 개수를 임계로 세게 되고
/// (임계가 움직이면 거짓 위반), 문장을 고치면 멀쩡한 억제 사유를 낱말 때문에 다시 쓰게
/// 된다. 그래서 주제어는 린트 이름이 아니라 **임계가 사는 자리의 이름**만 든다.
///
/// 이 doc 이 린트 억제의 **형태**(대괄호 속성)를 그대로 적지 않는 것도 그래서다 —
/// 억제를 세는 게이트가 문서 안의 인용까지 세고, 그 좌변은 여유가 0 이다.
const COGNITIVE_SUBJECT: &[&str] = &["cognitive-complexity-threshold", "clippy.toml"];
const SLOC_SUBJECT: &[&str] = &["check-file-size", "THRESHOLD=", "tokei"];
const SHARED_WALK_SUBJECT: &[&str] = &["check-shared-walk-ratchet", "shared_walk_gate"];
const ALLOW_REASON_SUBJECT: &[&str] = &["check-allow-reason", "allow_reason_gate"];
const FROZEN_SUM_SUBJECT: &[&str] = &[
    "frozen-sum-budget",
    "check-frozen-sum-ratchet",
    "frozen_sum_ratchet_gate",
];
const DOC_BIAS_SUBJECT: &[&str] = &["doc-comment-bias", "계측 편향"];

/// 게이트 하나 — 정본·명부·어휘·주제어를 한 값으로 묶는다.
///
/// 한때 이 넷은 각각 상수 짝이었고, 그것을 소비하는 판정 셋이 **손으로 짝지은 배열**을
/// 각자 들고 있었다. 게이트를 하나 더할 때 그 배열 셋을 전부 고쳐야 하고, 하나를
/// 빠뜨리면 그 판정만 새 게이트를 안 본다 — **그 누락은 초록이다.** 값으로 묶으면
/// 좌변이 하나라 빠뜨릴 자리가 없다.
struct Gate {
    label: &'static str,
    source: (&'static str, &'static str),
    claims: &'static [Claim],
    vocab: &'static [&'static str],
    subject: &'static [&'static str],
}

/// 값이 **리터럴로 사는** 게이트 전부.
///
/// ## 좌변이 `scripts/` 만이 아닌 이유 (2026-09-09)
///
/// 한때 이 목록은 `scripts/` 에서 `^[A-Z_]+=<숫자>` 로 세어 넷이었고, 나머지는 "값을 다른
/// 자리에서 유도하니 복제할 수가 없다" 로 넘겼다. **그 서술이 동결 총합 래칫에서 틀렸다.**
/// `check-frozen-sum-ratchet.sh` 가 유도하는 것은 띠(`SLACK` ← `THRESHOLD`)뿐이고,
/// **예산은 유도되지 않는다** — `.complexity-file-allowlist` 의 `# frozen-sum-budget:` 줄에
/// 리터럴로 산다. 사는 파일이 `scripts/` 밖이고 문법이 `NAME=<수>` 가 아니라서 그 세는 법에
/// 안 잡혔을 뿐이다. 즉 그 값은 다른 넷과 똑같이 복제될 수 있다.
///
/// 등록 시점의 사본 수는 **0 이다**(전수 2026-09-09: 값 `35504` 를 드는 자리는 정본과
/// **이 문장 자신** 둘이고 — 이 파일은 `EXCLUDED` 에 `Kind::SelfRef` 로 있어 자기 명부·사유·
/// 모듈 주석이 스캔에서 빠진다 — 옛 값 `36374` 를 드는 자리도 **둘**이다: 스크립트 머리의
/// 시점 서술과 **이 문장 자신**. 둘 다 낡는 것이 정상이다). 처음 적을 때 "정본 하나뿐" 이라고
/// 쓴 것은 세는 자리에서 **세는 문장 자신을 빼먹은 것**이고, 그것을 고친 회차가 **같은 문장
/// 안 옆의 옛 값에 같은 누락을 남겼다** — 이 파일이 `SelfRef` 라 가드는 두 번 다 조용했다.
/// 자기참조를 세는 자리에서는 문장이 값마다 자기를 다시 세야 한다.
///
/// **사본이 아직 0 인 지금** 등록하는 것이 미루지 않는 이유다 — 사본이 생긴 뒤에 등록하면
/// 어느 쪽이 정본인지를 사람이 다시 판정해야 한다. 그리고 예산은 **한 방향으로만 움직여** 되돌아 올라가지 않으므로
/// (ADR-0047), 낡은 사본이 가장 비싼 값이다.
///
/// 자기참조를 세는 자리에서는 이 형태가 반복된다 — 전수를 적는 문장은 자기가 그 전수의
/// 한 항목이 된다. `SelfRef` 가 있는 이유가 그것이고, 그래서 이 자리의 수는 "정본 + 자기" 로
/// 적는다.
const GATES: &[Gate] = &[
    Gate {
        label: "cognitive",
        source: COGNITIVE_SOURCE,
        claims: COGNITIVE_CLAIMS,
        vocab: COGNITIVE_VOCAB,
        subject: COGNITIVE_SUBJECT,
    },
    Gate {
        label: "파일 SLOC",
        source: SLOC_SOURCE,
        claims: SLOC_CLAIMS,
        vocab: SLOC_VOCAB,
        subject: SLOC_SUBJECT,
    },
    Gate {
        label: "공용 순회 래칫",
        source: SHARED_WALK_SOURCE,
        claims: SHARED_WALK_CLAIMS,
        vocab: SHARED_WALK_VOCAB,
        subject: SHARED_WALK_SUBJECT,
    },
    Gate {
        label: "사유 없는 allow 래칫",
        source: ALLOW_REASON_SOURCE,
        claims: ALLOW_REASON_CLAIMS,
        vocab: ALLOW_REASON_VOCAB,
        subject: ALLOW_REASON_SUBJECT,
    },
    Gate {
        label: "동결 총합 예산",
        source: FROZEN_SUM_SOURCE,
        claims: FROZEN_SUM_CLAIMS,
        vocab: FROZEN_SUM_VOCAB,
        subject: FROZEN_SUM_SUBJECT,
    },
    Gate {
        label: "계측 편향",
        source: DOC_BIAS_SOURCE,
        claims: DOC_BIAS_CLAIMS,
        vocab: DOC_BIAS_VOCAB,
        subject: DOC_BIAS_SUBJECT,
    },
];

/// 값 옆(±2 줄, [`claims_value`] 와 **같은 창**)에 좁은 주제어가 있으면 그 자리를 돌려준다.
///
/// 창을 어휘 판정과 같게 두는 것이 중요하다 — 창이 다르면 "어휘창은 통과했는데 주제어
/// 창은 안 통과" 같은 자리가 생기고, 그 차이는 규칙이 아니라 상수의 우연이 된다.
fn subject_near_value(text: &str, value: &str, subject: &[&str]) -> Option<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !has_bare_token(line, value) {
            continue;
        }
        let lo = i.saturating_sub(2);
        let hi = (i + 3).min(lines.len());
        for w in &lines[lo..hi] {
            if let Some(s) = subject.iter().find(|s| w.contains(**s)) {
                return Some((i + 1, (*s).to_string()));
            }
        }
    }
    None
}

/// "다른 뜻" 이라는 주장이 틀렸을 때의 판정문.
///
/// 파일시스템을 안 읽는다.
fn mislabeled_meaning_note(
    file: &str,
    label: &str,
    subject: &str,
    line: usize,
    why: &str,
) -> String {
    format!(
        "`{file}:{line}` 는 갈래 `OtherMeaning`(사유 \"{why}\") 인데 {label} 게이트의 \
         주제어 `{subject}` 를 값 바로 옆에 두고 있다 — 그 자리는 **다른 뜻일 수가 없다**.\n  \
         [왜 이것이 위반인가] 미분류 자리의 처방은 \"명부에 넣거나 사유와 함께 제외에 \
         넣어라\" 로 **두 통을 함께 내민다.** 어느 통이 맞는지는 아무것도 안 보고 있었다. \
         잘못 든 통은 조용하다 — 명부였어야 할 자리가 제외에 앉으면 그 값이 낡아도 \
         아무 말이 없다.\n  \
         [처방] 이 자리가 현재 상태를 주장하면 `*_CLAIMS` 로 옮겨라. 정말 다른 뜻이면 \
         주제어가 값 옆에 오지 않게 문장을 다시 써라 — 갈래를 바꾸는 것은 처방이 아니다."
    )
}

/// 스캔에 더 이상 안 걸리는 제외 항목에 대한 판정문.
///
/// 파일시스템을 안 읽는다 — 호출자가 판정 결과를 넘긴다.
///
/// 왜 이것이 위반인가: 제외는 **면제를 발행하는 자리**다. 그 자리가 더 이상 스캔에 안
/// 걸리면 면제는 아무것도 안 막고 있고, 그 파일이 나중에 **진짜 낡은 사본을 얻어도
/// 조용히 통과한다.** 즉 쓰이지 않는 면제는 중립이 아니라 **앞으로 열릴 구멍**이다.
/// 사유 칸은 그때 도움이 안 된다 — 사유는 왜 뺐는지를 말할 뿐 지금도 빼야 하는지를
/// 말하지 않는다.
fn dead_exclusion_note(file: &str, kind: Kind, why: &str) -> String {
    format!(
        "`{file}` 는 제외 목록에 있지만 두 임계 어느 쪽도 어휘창 안에서 안 든다 \
         (갈래 {kind:?} · 사유 \"{why}\").\n  \
         [처방] 이 항목을 지워라. 남겨 두면 그 파일이 나중에 진짜 낡은 사본을 얻어도 \
         면제가 먼저 걸려 조용히 통과한다 — 제외는 중립이 아니라 발행된 면제다.\n  \
         [지우면 안 되는 경우] 어휘가 그 자리에서 잠깐 빠졌을 뿐이라면 그 문장을 다시 \
         쓰는 것이 먼저다. 판정 기준은 사유가 아니라 **지금 스캔에 걸리는가**이고, \
         안 걸리는 자리는 제외가 아니라 그냥 무관한 파일이다."
    )
}

/// 명부 한 자리에 대한 축 ① 판정문. 문제가 없으면 `None`.
///
/// 파일시스템을 안 읽는다 — `text` 는 호출자가 읽어 넘긴다. 세 갈래가 여기서
/// 갈리고, 셋의 처방이 서로 다르다:
/// - 어휘와 떨어졌다 → 문장을 다시 쓰거나 명부에서 뺀다. **값은 건드리지 않는다**
/// - 값이 아예 없다 → 낡은 값을 들고 있는 것이니 그 자리를 고친다
/// - 사본 줄 수가 다르다 → [`stale_copy_note`]
fn claim_note(
    rel: &str,
    label: &str,
    value: &str,
    source_file: &str,
    text: &str,
    vocab: &[&str],
    copies: usize,
) -> Option<String> {
    if claims_value(text, value, vocab) {
        // 파일 단위로는 한 줄만 맞아도 통과하고, 그러면 같은 파일의 낡은 사본이
        // 조용히 남는다(변이 E 로 실측).
        let seen = text.lines().filter(|l| has_bare_token(l, value)).count();
        return stale_copy_note(rel, label, value, seen, copies);
    }
    // 값 토큰 자체가 파일에 있나 — 이 한 물음이 두 상태를 가른다.
    if text.lines().any(|l| has_bare_token(l, value)) {
        Some(format!(
            "  {rel}: {label} 임계값 `{value}` 은 있는데 게이트 어휘와 **떨어져** 있다 \
             — 이 자리가 그 게이트를 말하기를 그만뒀거나, 문구가 바뀌어 어휘창 밖으로 \
             나갔다. 여전히 주장하는 자리면 그 문장이 무엇의 임계인지 드러나게 쓰고, \
             더는 주장하지 않으면 `*_CLAIMS` 에서 빼라. **값을 다시 적지 마라 — \
             값은 이미 맞다**"
        ))
    } else {
        Some(format!(
            "  {rel}: {label} 임계를 주장하는 자리인데 지금 값 `{value}` 이 아예 없다 \
             — 정본은 `{source_file}` 이다. 낡은 값을 들고 있으면 그 자리를 고쳐라"
        ))
    }
}

/// 축 ② — 스캔에 걸렸는데 명부에도 제외에도 없는 자리.
fn unclassified_note(rel: &str, label: &str, value: &str) -> String {
    format!(
        "  {rel}: {label} 임계값 `{value}` 을 게이트 어휘와 함께 드는데 분류가 없다. \
         현재 상태를 주장하면 명부(`*_CLAIMS`)에, 시점 측정·인용·다른 뜻이면 \
         사유와 함께 `EXCLUDED` 에 넣어라"
    )
}

/// 스캔 모수의 바닥. 넘으면 `None`.
///
/// 이 하한은 **미분류 0 이 초록인 이유가 둘**이라 필요하다 — 정말 없거나, 안 봤거나.
fn scan_floor_note(seen: usize) -> Option<String> {
    (seen <= 500).then(|| {
        format!("추적 텍스트 파일을 {seen} 개만 읽었다 — 모수가 무너지면 미분류 0 도 초록이 된다")
    })
}

/// 사본 줄 수가 명부와 다를 때의 판정문. 같으면 `None`.
///
/// `check` 에서 갈라 둔 이유는 이 문구가 **파일시스템 없이 재질 수 있어야** 하기
/// 때문이다 — 문구를 읽으려고 레포의 문서를 고쳐 볼 수는 없다.
fn stale_copy_note(
    rel: &str,
    label: &str,
    value: &str,
    seen: usize,
    expected: usize,
) -> Option<String> {
    if seen == expected {
        return None;
    }
    Some(format!(
        "  {rel}: {label} 임계값 `{value}` 을 든 줄이 {seen} 개다(명부 {expected}).\n  \
         줄었으면 **그중 하나가 낡은 값으로 남아 있을 수 있다** — 이 파일에서 `{value}` 을 \
         든 줄과 다른 수를 든 줄을 함께 훑어라. 늘었으면 새 사본이 생긴 것이니 그 줄이 \
         현재 상태를 주장하는지 확인해라.\n  \
         [시점] 짚힌 줄이 '그때 그 값이어서 이렇게 했다' 는 서술이면 **값을 고치지 마라** \
         — 지금 값으로 고치면 없던 거짓이 새로 생긴다. 그때는 그 줄을 그대로 두고 이 수를 \
         맞춰라. 라벨의 뜻은 `ci_channel_claims_match_workflows.rs` 의 `TIME_NOTE` 가 \
         정한다(같은 개념에 이름을 새로 만들지 않는다)."
    ))
}

fn check(label: &str, source: (&str, &str), claims: &[Claim], vocab: &[&str]) -> Vec<String> {
    let value = source_value(source);
    let mut wrong = Vec::new();

    // ① 명부의 각 자리가 지금 값을 드는가.
    //
    // 안 들 때 **상태가 둘**이라 갈라 보고한다. 한 칸에 섞으면 실패문이 한쪽에게 틀린
    // 처방을 준다 — 실측: 어휘 다중도 1 인 자리에서 그 낱말 하나만 다른 말로 바꿨더니
    // "지금 값을 안 든다, 그 자리를 고쳐라" 가 나왔는데 **그 파일은 그 값을 들고
    // 있었다.** 따르면 아무것도 안 고쳐지거나 중복을 쓴다.
    for (rel, copies) in claims {
        if let Some(note) = claim_note(rel, label, &value, source.0, &read(rel), vocab, *copies) {
            wrong.push(note);
        }
    }

    // ② 미분류 신규 자리 — 스캔 생존자가 전부 명부/제외 안인가.
    let known: BTreeSet<&str> = claims
        .iter()
        .map(|(f, _)| *f)
        .chain(EXCLUDED.iter().map(|(f, _, _)| *f))
        .collect();
    for rel in mentioning_files(&value, vocab) {
        if !known.contains(rel.as_str()) {
            wrong.push(unclassified_note(&rel, label, &value));
        }
    }
    wrong
}

#[test]
fn the_gate_thresholds_have_no_stale_copies() {
    let mut wrong = Vec::new();
    for g in GATES {
        wrong.extend(check(g.label, g.source, g.claims, g.vocab));
    }
    let sources: Vec<String> = GATES
        .iter()
        .map(|g| {
            format!(
                "`{}` 의 `{}` = {}",
                g.source.0,
                g.source.1.trim_end_matches(" = ").trim_end_matches('='),
                source_value(g.source)
            )
        })
        .collect();
    assert!(
        wrong.is_empty(),
        "게이트 임계값의 사본이 정본과 갈렸거나, 분류되지 않은 주장 자리가 생겼다.\n\
         정본: {}\n{}",
        sources.join(" · "),
        wrong.join("\n")
    );
}

/// 판정기가 실제로 무는지 확인하는 변이 — 파일은 안 고친다.
/// "다른 뜻" 이라는 갈래 주장을 검사한다.
///
/// `Kind::OtherMeaning` 은 **검사 가능한 주장**이다 — 다른 뜻이라면 이 게이트의 좁은
/// 주제어가 값 옆에 있을 리 없다. 이 판정이 없는 동안 그 주장을 보는 것은 아무것도
/// 없었고, 미분류 자리의 처방이 통 둘을 함께 내밀고 있었다([`unclassified_note`]).
///
/// ★ **빈 좌변의 초록을 막는다.** 검사한 (파일, 값) 짝이 0 이면 그 자체로 실패다 —
/// 위반 0 과 볼 것이 0 은 화면에서 같고, 갈래 이름이 바뀌거나 목록이 비면 이 판정은
/// 조용히 아무것도 안 보게 된다.
#[test]
fn a_different_meaning_exclusion_does_not_carry_the_gate_subject() {
    let values: Vec<String> = GATES.iter().map(|g| source_value(g.source)).collect();
    let mut checked = 0usize;
    let mut wrong = Vec::new();
    for (file, kind, why) in EXCLUDED {
        if *kind != Kind::OtherMeaning {
            continue;
        }
        let text = read(file);
        for (g, value) in GATES.iter().zip(&values) {
            if !claims_value(&text, value, g.vocab) {
                continue;
            }
            checked += 1;
            if let Some((line, hit)) = subject_near_value(&text, value, g.subject) {
                wrong.push(mislabeled_meaning_note(file, g.label, &hit, line, why));
            }
        }
    }
    assert!(
        checked > 0,
        "`Kind::OtherMeaning` 으로 검사한 (파일, 값) 짝이 0 이다 — 이 판정은 빈 좌변 \
         위에서 초록이 된다. 갈래 이름이 바뀌었거나 어휘 판정이 죽었다."
    );
    assert!(
        wrong.is_empty(),
        "\"다른 뜻\" 이라는 제외 사유가 {} 짝 중 {} 곳에서 틀렸다.\n{}",
        checked,
        wrong.len(),
        wrong.join("\n")
    );
}

/// 제외가 **아직 무언가를 막고 있는가**. 안 막고 있으면 지워야 한다.
///
/// **오늘 걸리는 자리는 0 이다**(제외 29 · 2026-09-08 · `b134d28e3`+). 그래서 이 판정은
/// 실제 사건을 되짚어 넣은 것이 아니라 **면제 목록이 자란다는 이 파일 자신의 서술**
/// ("`EXCLUDED` 는 자란다 — 결함이 아니다")에 붙는 반대쪽 래칫이다: 자라는 목록에
/// 물러나는 길이 없으면 면제만 쌓인다. 사유 칸은 그 길이 못 된다 — 사유는 왜 뺐는지를
/// 말하지 **지금도 빼야 하는지**를 말하지 않는다.
///
/// ★ 이 자리에 한때 "제외 29 중 둘이 죽어 있었다" 고 적으려 했다. 그 둘
/// (시간을 뜻한 "20분", `src/app/event_handler.rs`의 "20여개")은 **죽지 않았다** —
/// 그렇게 읽은 계측기가 python 이었고 python 의 `str.isalnum()` 은 유니코드라 `20여개` 의
/// `여` 를 낱말 안으로 셌다. [`has_bare_token`] 은 `is_ascii_alphanumeric` 이라 거기서
/// 낱말이 끊긴다. 판정기가 옳았고 사본이 틀렸다.
#[test]
fn no_exclusion_is_dead_weight() {
    let values: Vec<String> = GATES.iter().map(|g| source_value(g.source)).collect();
    let mut dead = Vec::new();
    for (file, kind, why) in EXCLUDED {
        let text = read(file);
        if !GATES
            .iter()
            .zip(&values)
            .any(|(g, value)| claims_value(&text, value, g.vocab))
        {
            dead.push(dead_exclusion_note(file, *kind, why));
        }
    }
    assert!(
        dead.is_empty(),
        "제외 목록에 죽은 면제가 {} 개 있다 — 제외는 발행된 면제라 안 쓰이면 지워야 한다.\n{}",
        dead.len(),
        dead.join("\n")
    );
}

mod threshold_mutations {
    use super::*;

    /// 두 상태가 실제로 갈리는가 — 값이 없는 것과, 값은 있는데 어휘가 떨어진 것.
    #[test]
    fn the_two_failure_states_are_told_apart() {
        // 값은 있는데 어휘가 창 밖 — 어휘가 있어야 주장으로 센다.
        let drifted = "함수 크기 상한(20) 을 넘기지 않도록.\n";
        assert!(!claims_value(drifted, "20", COGNITIVE_VOCAB));
        assert!(drifted.lines().any(|l| has_bare_token(l, "20")));
        // 값 자체가 없다.
        let gone = "인지 복잡도 상한(25) 을 넘기지 않도록.\n";
        assert!(!claims_value(gone, "20", COGNITIVE_VOCAB));
        assert!(!gone.lines().any(|l| has_bare_token(l, "20")));
    }

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
        for (file, _, why) in EXCLUDED {
            assert!(!why.trim().is_empty(), "{file} 의 제외 사유가 비었다");
        }
    }

    #[test]
    fn no_file_is_both_claimed_and_excluded() {
        let excluded: BTreeSet<&str> = EXCLUDED.iter().map(|(f, _, _)| *f).collect();
        for (rel, _) in GATES.iter().flat_map(|g| g.claims.iter()) {
            assert!(
                !excluded.contains(rel),
                "{rel} 가 명부와 제외목록에 둘 다 있다"
            );
        }
    }
}

/// 사본 줄 수 판정의 **문구** 양성 대조. 파일시스템을 안 읽는다.
///
/// 명부의 실제 수(`COGNITIVE_CLAIMS` 등)를 여기 안 쓴다 — 자기가 재려는 값으로 자기를
/// 지으면 그 값에 대해 항진명제가 된다. 여기 값은 메커니즘을 발화시키기 위한
/// 리터럴이고, 실측 수가 맞는지는 본 시험이 잰다.
///
/// ## 이 픽스처들이 실제로 무는가 (실측 2026-09-08, 내 트리 · 매 변이 패키지 전체 569)
///
/// | 변이 | rc | 죽은 시험 |
/// |---|---|---|
/// | `stale_copy_note` 에서 같을 때의 조용한 갈래 삭제 | 101 | `a_matching_count_says_nothing` + `judgment_wording::a_healthy_claim_says_nothing` + 본 판정 |
/// | 〃 의 줄어든 쪽 처방 문구 교체 | 101 | `a_shrunk_count_warns_that_one_copy_may_be_stale` — 이것만 |
/// | 〃 의 시점 라벨 정의 자리 인용 제거 | 101 | `the_note_forbids_editing_a_dated_line_and_points_at_the_existing_label` — 이것만 |
///
/// 첫 변이가 셋을 함께 죽이는 것이 옳다 — 조용한 갈래를 지우면 건강한 주장까지 위반이
/// 되므로 세 자리가 같은 회귀를 각자의 문장으로 말한다.
#[cfg(test)]
mod copy_count_wording {
    use super::stale_copy_note;

    #[test]
    fn a_matching_count_says_nothing() {
        assert!(stale_copy_note("a.md", "파일 SLOC", "1000", 3, 3).is_none());
    }

    #[test]
    fn a_shrunk_count_warns_that_one_copy_may_be_stale() {
        let m = stale_copy_note("a.md", "파일 SLOC", "1000", 2, 3).expect("발화해야 한다");
        assert!(m.contains("든 줄이 2 개다(명부 3)"), "{m}");
        assert!(m.contains("낡은 값으로 남아 있을 수 있다"), "{m}");
        assert!(m.contains("a.md"), "좌표가 없다: {m}");
    }

    /// 늘어난 쪽의 처방은 다르다 — 새 사본이 현재 상태를 주장하는지 확인하는 것이다.
    #[test]
    fn a_grown_count_asks_whether_the_new_line_claims_the_present() {
        let m = stale_copy_note("a.md", "cognitive", "20", 4, 3).expect("발화해야 한다");
        assert!(m.contains("든 줄이 4 개다(명부 3)"), "{m}");
        assert!(m.contains("새 사본이 생긴 것"), "{m}");
    }

    /// ③ 시점 서술을 지금 값으로 고치면 없던 거짓이 생긴다 — 그 경고가 문구에 있어야
    /// 하고, 라벨의 정의는 **이미 있는 것을 가리킨다**(새로 만들지 않는다).
    #[test]
    fn the_note_forbids_editing_a_dated_line_and_points_at_the_existing_label() {
        let m = stale_copy_note("a.md", "파일 SLOC", "1000", 2, 3).expect("발화해야 한다");
        assert!(m.contains("[시점]"), "{m}");
        assert!(m.contains("값을 고치지 마라"), "{m}");
        assert!(m.contains("TIME_NOTE"), "라벨 정의를 안 가리킨다: {m}");
    }
}

/// 나머지 판정 다섯의 **문구** 양성 대조. 파일시스템을 안 읽는다.
///
/// 이 모듈이 쓰는 값은 전부 자기 리터럴이다 — 가드의 명부·하한 상수에서 뽑아 쓰면
/// 그 상수에 대해 항진명제가 된다.
///
/// ## 이 픽스처들이 실제로 무는가 (실측 2026-09-08, 내 트리 · 매 변이 패키지 전체 569)
///
/// | 변이 | rc | 죽은 시험 |
/// |---|---|---|
/// | `parse_source_value` 의 "정확히 1 번" 무력화 | 101 | `a_source_prefix_that_appears_twice_refuses_to_guess` — 이것만 |
/// | 〃 의 "숫자가 없다" 단정 삭제 | 101 | `a_source_prefix_without_digits_says_the_reader_is_stale` — 이것만 |
/// | 〃 의 값 추출을 한 글자 밀기 | 101 | `a_well_formed_source_line_yields_the_value` + 본 판정 + `no_exclusion_is_dead_weight` |
/// | `scan_floor_note` 의 하한 500 → 2 | 101 | `a_collapsed_scan_says_zero_would_be_green` — 이것만 |
/// | `unclassified_note` 에서 제외 명부 이름 제거 | 101 | `an_unclassified_site_is_offered_both_bins` — 이것만 |
/// | `claim_note` 의 사본 계수를 하나 부풀림 | 101 | `a_healthy_claim_says_nothing` + 본 판정 |
///
/// 다섯 중 넷이 배타적이고, 나머지는 본 판정과 함께 죽는다 — 그 둘은 판정 능력이 겹치는
/// 것이 아니라 **처방이 갈리는** 자리다(본 판정은 "어느 파일이 낡았다", 이쪽은 "판독기가
/// 깨졌다").
///
/// ★ 세 번째 줄의 `no_exclusion_is_dead_weight` 는 **병합된 트리에서만** 함께 죽는다.
/// 그 시험은 `source_value` → [`parse_source_value`] 를 소비하므로, 값 추출이 밀리면
/// 임계값 대조가 전부 거짓이 되어 `EXCLUDED` 전 항목이 죽은 면제로 몰린다. 값 추출을
/// **느슨하게** 하는 위 두 변이는 실물 정본이 그 갈래를 안 밟아 그 시험을 안 죽인다 —
/// 셋 다 병합 트리에서 실측했다(위 표의 나머지 여섯은 두 트리에서 같은 답이 나왔다).
#[cfg(test)]
mod judgment_wording {
    use super::{
        claim_note, mislabeled_meaning_note, parse_source_value, scan_floor_note,
        subject_near_value, unclassified_note,
    };

    const VOCAB: &[&str] = &["임계", "복잡도"];

    /// 주제어가 창 안에 있으면 그 줄을 집는다 — 값과 같은 줄이 아니어도 된다.
    ///
    /// 리터럴로 짓는다: 가드의 `*_SUBJECT` 를 갖다 쓰면 그 상수에 대해 항진명제가
    /// 된다. 여기서 재는 것은 **창과 낱말 경계의 메커니즘**이다.
    #[test]
    fn a_subject_two_lines_above_the_value_is_found() {
        let text = "check-file-size 를 설명한다\n\n상한은 1000 줄이다\n";
        let hit = subject_near_value(text, "1000", &["check-file-size"]);
        assert_eq!(hit, Some((3, "check-file-size".to_string())));
    }

    /// 창은 ±2 줄이다 — 세 줄 떨어지면 안 집는다. 창을 넓히는 변경이 이 시험을 깨운다.
    #[test]
    fn a_subject_three_lines_away_is_out_of_the_window() {
        let text = "check-file-size 를 설명한다\n\n\n\n상한은 1000 줄이다\n";
        assert_eq!(subject_near_value(text, "1000", &["check-file-size"]), None);
    }

    /// 값이 낱말의 일부면 자리로 안 센다 — `1000ms` 옆의 주제어는 위반이 아니다.
    #[test]
    fn a_glued_value_is_not_a_site_even_next_to_the_subject() {
        let text = "check-file-size\n타임아웃 1000ms\n";
        assert_eq!(subject_near_value(text, "1000", &["check-file-size"]), None);
    }

    /// 판정문은 **두 처방을 갈라** 싣는다 — 갈래를 바꾸는 것은 처방이 아니라고 못 박는다.
    #[test]
    fn the_note_offers_the_roster_or_a_rewrite_but_not_a_relabel() {
        let m = mislabeled_meaning_note("a/b.md", "파일 SLOC", "check-file-size", 7, "사유");
        assert!(m.contains("a/b.md:7"), "{m}");
        assert!(m.contains("CLAIMS"), "명부로 옮기라는 갈래가 없다: {m}");
        assert!(
            m.contains("문장을 다시 써라"),
            "다시 쓰라는 갈래가 없다: {m}"
        );
        assert!(
            m.contains("갈래를 바꾸는 것은 처방이 아니다"),
            "가장 싼 수선을 금지하는 문장이 없다: {m}"
        );
    }

    /// 값이 있고 어휘도 붙어 있고 줄 수도 맞으면 아무 말도 안 한다.
    #[test]
    fn a_healthy_claim_says_nothing() {
        let text = "이 게이트의 임계는 20 이다.\n";
        assert!(claim_note("a.md", "cognitive", "20", "clippy.toml", text, VOCAB, 1).is_none());
    }

    /// 값은 있는데 어휘가 멀면 처방이 "값을 고쳐라" 가 **아니다** — 그 자리를 고치거나
    /// 명부에서 빼는 것이다. 이 갈래를 안 나눠 거짓 처방이 나간 적이 있다.
    #[test]
    fn a_value_far_from_the_vocabulary_is_told_not_to_rewrite_the_value() {
        let text = "복잡도를 말한다.\n한 줄.\n두 줄.\n세 줄.\n무관한 문장 20 개.\n";
        let m = claim_note("a.md", "cognitive", "20", "clippy.toml", text, VOCAB, 1)
            .expect("발화해야 한다");
        assert!(m.contains("떨어져"), "{m}");
        assert!(m.contains("값을 다시 적지 마라"), "{m}");
    }

    /// 값이 아예 없으면 그때는 "그 자리를 고쳐라" 가 맞고, 정본이 어디인지 함께 준다.
    #[test]
    fn a_missing_value_names_the_source_of_truth() {
        let text = "이 게이트의 임계는 15 이다.\n";
        let m = claim_note("a.md", "cognitive", "20", "clippy.toml", text, VOCAB, 1)
            .expect("발화해야 한다");
        assert!(m.contains("아예 없다"), "{m}");
        assert!(m.contains("clippy.toml"), "정본을 안 가리킨다: {m}");
    }

    #[test]
    fn an_unclassified_site_is_offered_both_bins() {
        let m = unclassified_note("a.md", "파일 SLOC", "1000");
        assert!(m.contains("분류가 없다"), "{m}");
        assert!(m.contains("_CLAIMS"), "{m}");
        assert!(m.contains("EXCLUDED"), "{m}");
    }

    #[test]
    fn a_collapsed_scan_says_zero_would_be_green() {
        assert!(scan_floor_note(9000).is_none());
        let m = scan_floor_note(3).expect("발화해야 한다");
        assert!(m.contains("3 개만 읽었다"), "{m}");
        assert!(m.contains("미분류 0 도 초록"), "{m}");
    }

    #[test]
    #[should_panic(expected = "정확히 1 번이어야")]
    fn a_source_prefix_that_appears_twice_refuses_to_guess() {
        parse_source_value("clippy.toml", "T = ", "T = 20\nT = 30\n");
    }

    #[test]
    #[should_panic(expected = "숫자가 없다")]
    fn a_source_prefix_without_digits_says_the_reader_is_stale() {
        parse_source_value("clippy.toml", "T = ", "T = abc\n");
    }

    #[test]
    fn a_well_formed_source_line_yields_the_value() {
        assert_eq!(parse_source_value("clippy.toml", "T = ", "T = 20\n"), "20");
    }
}
