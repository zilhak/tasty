//! 문서가 "CI 가 돌린다" 고 말하는 명령이 **실제로 자동으로 도는가** 를 대조한다.
//!
//! 배경: `cargo test --workspace` 전체 스위트는 `.github/workflows/test.yml` 의
//! `test-linux-x64` 잡에 있지만 그 잡은 `if: github.event_name == 'workflow_dispatch'`
//! 라 **수동 전용**이다. 그런데 레포 곳곳이 "`tests/X.rs` 가 `cargo test --workspace`(CI)
//! 로 강제한다" 라고 적어 왔다 — 자동으로 돌지 않는 채널을 강제 장치로 부른 것이다.
//! 그 서술을 읽은 사람은 자기가 아무것도 돌리지 않아도 어딘가가 잡아 준다고 믿는다.
//!
//! 이 가드가 없으면 같은 문장이 계속 새로 쓰인다: 컴파일도 통과하고, 그 주장이 틀렸다는
//! 사실은 워크플로 파일을 직접 열어야만 보인다. 채널 매트릭스 자체는
//! `docs/dev-guide/ci-gates.md`.
//!
//! **판정은 워크플로에서 읽는다**(문서를 문서로 검사하지 않는다):
//! - 자동 트리거(`push` / `pull_request` / `schedule`)를 가진 워크플로의 잡 중,
//!   `workflow_dispatch` 로 좁혀지지 않은 잡을 모은다.
//! - 그 잡들이 **전체 스위트**(`cargo test --workspace` 를 `--lib`/`--bins`/`--test` 로
//!   좁히지 않은 형태)를 돌리면 → 주장은 참이므로 그 판정은 조용히 통과한다.
//! - 돌리지 않으면 → "전체 스위트를 CI 가 돌린다" 는 서술은 전부 위반이다.
//!
//! 즉 전체 스위트가 자동화되는 날 이 가드는 스스로 잠잠해진다. 문서를 손으로 다시
//! 훑을 필요가 없다.
//!
//! **두 번째 축 — 명령을 적지 않는 형태.** 위 검사는 명령 리터럴 주변만 본다. 그런데
//! 같은 거짓말이 명령 없이, 테스트 파일 이름과 집행 주장만으로도 쓰인다. 이 축이 보는
//! 구분은 "CI 가 도는가" 가 아니라 **그 테스트가 자동 잡의 사정거리 안에 있는가** 다.
//! 자동 잡이 `--lib`/`--bins`/`--test <이름>` 으로 좁혀져 있으면 그 사정거리는 좁고, 그
//! 열거에 없는 `tests/*.rs` 통합 테스트를 자동 집행 장치로 부르는 서술은 거짓이다.
//! 좁히지 않은 자동 잡이 하나라도 있으면 이 축은 스스로 잠잠해진다.
//!
//! 열거는 이 파일이 복사해 갖고 있지 않고 워크플로에서 **런타임에** 읽는다. 복사본을
//! 들면 워크플로가 바뀐 날 가드가 조용히 낡는다.
//!
//! **세 번째 축 — 반대 방향.** 자동 잡이 lib 유닛 테스트를 돌리는 동안에는, `src/` 안의
//! 유닛 테스트를 두고 부재를 적는 것도 거짓이다(사실보다 약하다). 강한 부정은 강한
//! 긍정만큼 검증이 필요하다 — 한 방향만 잡는 가드는 틀린 방향 하나를 굳힌다. 이 축도
//! **"lib 은 자동으로 돈다" 를 상수로 들지 않고** 워크플로에서 읽는다. 상수로 들면
//! Windows 잡의 `--lib --bins` 가 사라지는 날 없는 채널을 근거로 고발한다.
//!
//! # 이 가드가 **모수에 넣지도 못하는** 자리 (줄바꿈)
//!
//! 표지를 리터럴 부분문자열로 찾는다. 그래서 표지가 **줄바꿈에 쪼개지면** 그 자리는
//! 위반이 아니라 **모수 밖**이 된다 — "0 건 발견" 과 "안 봤다" 가 화면에서 같아진다
//! (R8). 실측(2026-09-05, 스캔 1708 파일): 리터럴로 잡힌 표지 **222**, 줄바꿈에 쪼개져
//! 놓친 표지 **7**(약 3%). 그 7 중 둘이 실제로 낡은 문장이었고, 하나는 이 가드로
//! "고쳤다" 고 보고된 뒤에도 같은 파일에 남아 있던 자리다 —
//! **고침의 완료 판정을 이 가드로만 하면 안 된다.**
//!
//! 고치려면 줄바꿈+들여쓰기+주석 접두를 공백 하나로 접은 뒤 찾아야 하는데, 접는 순간
//! 오프셋이 원문과 어긋나 `claim_scope`·`line_of` 가 전부 딸려 온다. 한 줄 고침이
//! 아니라서 지금은 **한계를 적어 두는 쪽**을 골랐다.
//!
//! **네 번째 축 — 조합.** "통합 테스트에는 자동 실행 채널이 없다" 는 한동안 구성상
//! 참이어서 그 방향은 판정할 것이 없었다. 헤드리스 잡이 `--lib --bins` 에서 전체
//! 스위트로 넓어지면서 전제가 깨졌다 — 같은 문장이 이제 **조합마다** 갈린다(기본
//! 조합에는 여전히 없고, 헤드리스에는 있다). 단일 참·거짓으로 판정하면 어느 쪽으로
//! 고쳐도 반쪽이 거짓이 되므로, 판정 단위를 **그 테스트가 자동으로 도는 조합의 수**로
//! 둔다: 0 이면 부재 서술이 참, 1 이면 어느 조합인지 함께 적어야 참, 2 면 어떻게 적어도
//! 거짓이다.
//!
//! **가드가 막지 못하는 것** — 조용히 통과하는 형태를 적어 둔다. 여기 적힌 것은 사각인
//! 줄 알고 남긴 것이고, 적히지 않은 형태가 새 사각이다.
//!
//! - **대상을 특정하지 않은 집행 서술** — 테스트 이름도 명령도 없이 "CI 가 잡아 준다"
//!   라고만 쓴 문장. 무엇을 가리키는지 텍스트만으로 결정할 수 없어 판정할 대상이 없다.
//!   이름을 요구하지 않고 표지만으로 짚으면 정확히 쓴 문장까지 함께 걸리고, 그 오탐을
//!   피하려 표지를 좁히면 결국 아무것도 안 잡는다. 그래서 **판정하지 않고 통과시킨다** —
//!   다만 그런 문장은 리뷰에서 "무엇이 그걸 돌리나" 를 되물어야 한다.
//! - **한 문장이 두 축을 묶은 형태** — "cognitive 복잡도와 파일 SLOC 의 신규분을 자동
//!   차단한다" 처럼 참인 축과 거짓인 축이 한 문장에 섞인 경우. 참/거짓이 문장 단위로
//!   갈리지 않아 짚을 좌표가 없고, 문장을 통째로 위반으로 부르면 참인 절반까지 지우게
//!   된다. **판정하지 않는다** — 대신 채널 정본이 축별 표를 갖고, 파생 문서는 채널을 다시
//!   서술하지 말고 정본을 링크한다(`docs/dev-guide/ci-gates.md`). 이 한계는 규칙으로
//!   메울 수 없어서 문서 관행으로 메운다.
//! - **강제 수단이 워크플로 밖에 있는 것** — clippy `deny`·`#[deny]`·pre-commit·타입
//!   시스템은 워크플로를 읽어서는 보이지 않는다. 그래서 이 가드의 "자동으로 돌지
//!   않는다" 는 **워크플로 채널에 한한 말**이고, "아무도 안 막는다" 는 뜻이 아니다.
//! - **문서 밖의 주장** — 커밋 메시지·PR 본문·티켓은 스캔 대상이 아니다.
//! - **주어가 테스트 타깃이 아닌 부재 주장** — 이 가드의 주어 부류는 **이름이 지목된
//!   테스트 타깃**(`tests/X.rs` · `--test X`)이다. 그래서 같은 표지를 **워크플로 파일**이나
//!   **패키지 단위 호출**에 붙이면 조용히 통과한다. 셋을 변이로 갈랐다(2026-09-06):
//!
//!   ```text
//!   주어 = 워크플로 파일   "(`plugin-version-check.yml` 은 <부재 표지>)"   -> 52 초록 (안 잡힘)
//!   주어 = 패키지 호출     "| 문서 가드 (<부재 표지>) | cargo test -p … |"  -> 52 초록 (안 잡힘)
//!   주어 = 통합 테스트 이름 (대조군 — 실재하는 타깃 하나에 같은 표지)       -> 빨강 ✔
//!   ```
//!
//!   (위에서 `<부재 표지>` 라고 쓴 자리에는 [`ABSENCE_MARKERS`] 의 문구가 그대로 들어간다.
//!   셋째 줄만 이 판정기가 잡았고, 그것이 **대조군이 반응한다는 증거**다 — 앞의 둘이
//!   안 잡힌 것은 대조군 고장이 아니라 주어 부류 밖이라는 뜻이다. 이 문단이 표지를
//!   실물 이름 옆에 그대로 적으면 그 자체가 거짓 주장이 되어 이 판정기가 문단을 잡는다.
//!   실제로 처음 적을 때 그렇게 잡혔고, 처방 (나)로 고쳤다.)
//!
//!   앞의 둘이 사각이다. 그러니 **"이 가드가 채널 주장을 지킨다" 는 문장은 범위를 넘는다**
//!   — 지키는 것은 *테스트 타깃에 대한* 채널 주장이다. 워크플로·스크립트 게이트를 두고
//!   "자동 채널 없음" 이라 적으면 아무도 안 본다(R477: 가드가 있다 ≠ 가드가 덮는다).
//!   메우려면 주어 부류를 워크플로 이름까지 넓혀야 하는데, 그러면 워크플로를 *언급만* 하는
//!   문장이 전부 후보가 되어 오탐이 지배한다 — 그 판별식을 먼저 짓기 전에는 넓히지 마라.
//!
//! **실행으로 판정할 수 없는 전제 — 자동 채널 없음**(R16). 아래 셋은 이 축의 채널
//! 모델이 딛고 선 사실인데, 이 레포에서 실행으로 확인할 방법이 없다. (부재의 주어는 아래
//! 세 전제이지 이 타깃의 실행 채널이 아니다.) 변이를 지어내지도
//! 침묵하지도 않고 부재를 명시한다. 좌표는 base `db6571d7` — base 가 옮겨가 확인 수단이
//! 생기면 이 선언은 만료되고, 그때는 선언을 지우고 검사를 넣는다(R16-b).
//!
//! - **libtest 의 `--skip` 은 부분일치다.** 그래서 한 타깃의 모든 테스트 이름이 skip
//!   문자열을 포함할 때만 그 타깃이 통째로 빠진다고 본다. libtest 의 매칭 규칙 자체를
//!   이 레포의 테스트로 확인할 수는 없다.
//! - **`--no-default-features` 는 워크스페이스 전 멤버의 default feature 를 끈다.** 그래서
//!   `required-features` 가 걸린 타깃은 헤드리스 조합에서 아예 만들어지지 않는다고 본다.
//!   cargo 의 feature 해석을 여기서 재현해 확인하지 않는다.
//!
//!   ★ **이 전제만은 대역 밖 프로브로 확인했다**(2026-09-05). 레포에 실물이 하나 있다 —
//!   `Cargo.toml` 의 `[[test]] name = "gui_tests"` 가 `required-features = ["gui"]` 다.
//!   양방향으로 cargo 가 그 자리에서 직접 답한다:
//!
//!   ```text
//!   cargo test --no-default-features --test gui_tests --no-run
//!     -> rc 101, error: target `gui_tests` in package `tasty` requires the features: `gui`
//!   cargo test --test gui_tests --no-run
//!     -> rc 0, 빌드된다
//!   ```
//!
//!   **그래도 검사로 넣지 않는다**(R16-b 의 문자 그대로라면 넣어야 한다). 이 판정은
//!   테스트 안에서 cargo 를 다시 부르는 형태여야 하는데, 재귀 cargo 호출은 상위 빌드의
//!   락과 프로필을 물고 분 단위로 늘어난다 — 이 가드의 나머지 42 항목이 초 단위인 것과
//!   맞지 않는다. 값이 확인된 전제를 **비싼 검사로 바꾸는 것**은 R303 이 말리는 쪽이다.
//!   대신 위 명령과 결과를 여기 남긴다: 다음 사람은 재현에 두 줄이면 된다.
//! - **조합 한정 표지 목록의 완전성.** "어느 조합인지 함께 적었다" 를 판정하는 문자열
//!   목록이 정확히 쓴 문장을 빠짐없이 덮는지는, 아직 쓰이지 않은 문장을 대상으로 하므로
//!   실행으로 판정할 수 없다. 목록에 없는 정확한 표현은 **거짓 위반**으로 나타나며, 그때
//!   목록을 넓히는 것이 처방이다 — 이 방향의 오류는 조용하지 않다.

// 테스트 본문은 `let _ =` 사유 주석 정책의 범위 밖이다 — 전수 가드
// (`tests/let_underscore_documented.rs`)가 테스트 본문을 제외하므로, 여기서 나는
// `let_underscore_must_use` 경고는 정책상 조치 대상이 될 수 없다. 끄지 않으면
// 프로덕션의 진짜 신호가 그 안에 묻힌다 — `docs/dev-guide/error-handling.md`.
#![allow(clippy::let_underscore_must_use)]

//! ## 이 파일의 테스트 42 개가 각각 무엇을 재는가
//!
//! **5 개만 레포를 본다** — `no_file_claims_ci_runs_the_full_suite_while_it_does_not` ·
//! `no_file_claims_ci_enforces_an_integration_test_it_does_not_run` ·
//! `no_file_denies_the_automatic_channel_a_lib_test_actually_has` ·
//! `no_file_denies_a_channel_an_integration_test_actually_has` ·
//! `the_theme_table_keeps_the_two_channels_apart`. 이 다섯이 이 가드의 **판정**이다.
//! 나머지 37 은 그 판정이 쓰는 **헬퍼의 자기검사**로, 임시 디렉토리에 픽스처를 지어
//! 판독기가 그 형태를 어떻게 읽는지 고정한다. 둘은 다른 것을 잰다 — 37 이 초록이라는
//! 것은 판독기가 픽스처대로 읽는다는 뜻이지, 레포에 대한 판정이 옳다는 뜻이 아니다.
//!
//! ★ **그 37 이 잡 분할 규칙을 시험하지 않았다.** 실측(2026-09-05): 라이브러리의 잡 헤더
//! 규칙(2 칸 들여쓰기)을 3 칸으로 바꾸는 변이가 이 파일의 42 개를 **하나도 못 죽였다.**
//! 이유는 픽스처가 전부 **잡 하나짜리**여서다 — 잡이 하나면 헤더를 못 찾아 파일 전체가
//! 한 덩어리가 돼도 개수가 1 로 같다. 그 변이를 죽이는 단정은
//! `tasty_doc_guards::workflow_triggers` 의 단위 테스트에 있다(잡 둘 · 레포 부등식).
//! 판정이 사는 곳에 그 판정의 시험을 두는 것이고, 여기서 한 번 더 세지 않는다.
//!
//! ★ 그리고 위 다섯 중 둘은 **오늘 조기 반환한다**: `check-headless` 가 좁혀지지 않은
//! `cargo test --workspace` 를 돌기 때문에 "통합 테스트가 전부 자동으로 돈다" 가 참이 되고,
//! 그 축은 스스로 잠잠해진다. 설계된 동작이지만, 그 둘의 초록을 커버리지 근거로 읽으면
//! 안 된다 — 초록이 "덮였고 위반이 없다" 와 "볼 것이 없어 안 봤다" 둘 다와 양립한다.
//!

use std::path::{Path, PathBuf};

use tasty_doc_guards::workflow_triggers::automatic_job_bodies;

/// 레포 루트 — 이 크레이트가 `crates/` 아래 살아서 `CARGO_MANIFEST_DIR` 이 레포 루트가
/// 아니다. 해석과 검증을 [`tasty_doc_guards::repo_root`] 한 곳에 모은다(ADR-0138).
fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// 스캔에서 제외할 디렉토리 — 빌드 산출물과 커밋되지 않는 로컬 폴더.
const SKIP_DIRS: &[&str] = &["target", ".git", "_site", "node_modules"];

/// 텍스트로 읽을 확장자.
const TEXT_EXTS: &[&str] = &["rs", "md", "toml", "yml", "yaml", "sh"];

/// 그 자리가 **부재를 함께 말하고 있는가** — 이것이 주장을 정당하게 만드는 유일한
/// 조건이다.
///
/// 경로 allowlist 를 쓰지 않는다. 정당한 서술의 조건은 "어느 파일이냐" 가 아니라
/// "자동으로 돌지 않는다는 사실을 같이 적었느냐" 이고, 그것은 파일과 무관하게 판정할 수
/// 있다. 규칙으로 두면 앞으로 정확히 쓴 문장은 등록 없이 통과하고, 등록만 해 두고 문장은
/// 틀린 채로 두는 형태도 생기지 않는다.
const ABSENCE_MARKERS: &[&str] = &[
    "수동 전용",
    "수동 실행",
    "수동 트리거",
    "자동 채널 없음",
    "자동 채널이 없다",
    "자동 채널은 아니다",
    "자동으로 돌지 않는다",
    "자동으로 도는 채널은 없다",
    "그 채널도 수동",
    "workflow_dispatch",
    // 실행 축을 명시한 정밀한 형태 — 컴파일은 자동이라는 사실을 함께 남기려면 이렇게
    // 써야 하므로, 부재 표지도 이 형태를 알아야 한다.
    "실행 채널이 없",
    "실행 채널 없음",
    // **배타 주장도 부재 주장이다.** "X 잡에서만 일어난다" 는 X 밖의 채널을 부정한다 —
    // 조합이 하나뿐인 타깃에서는 [`COMBO_QUALIFIED_MARKERS`] 가 정당한 한정으로 통과시키고,
    // 둘인 타깃에서는 어떻게 적어도 거짓이므로 걸린다. 이 표지가 없던 동안 조합이 둘로
    // 늘어난 타깃 둘이 한 잡만 적은 채 살아 있었다.
    "에서만 일어난다",
    "실행은 수동",
];

/// `cargo test --workspace` 인용 지점 주변에서 "CI 가 돌린다" 는 표지를 찾는다.
///
/// **줄 단위로 보지 않는다** — 주석과 마크다운은 문장을 예사로 줄바꿈하고, 실제로
/// `tests/macos_bundle_codesign.rs` 는 명령과 "채널" 을 다른 줄에 뒀다. 줄로 끊어 보면
/// 그런 주장이 그대로 빠져나간다. 그래서 인용 지점 앞뒤 창을 한 덩어리로 읽는다.
/// CI 가 자동으로 돌린다는 뜻으로 읽히는 표지.
const CI_MARKERS: &[&str] = &[
    "(CI)",
    "CI 강제",
    "CI 채널",
    "CI channel",
    "CI 의",
    "CI 에서",
    "Linux CI",
    "test.yml",
    ".github/workflows",
];

/// 명령 문자열 없이 "CI 가 이 장치를 돌린다" 는 뜻으로 읽히는 표지.
///
/// [`CI_MARKERS`] 와 목록이 다르다. 저기엔 `test.yml` 같은 **참조**가 들어 있는데,
/// 워크플로를 가리키는 것 자체는 주장이 아니다 — 명령이 함께 있을 때만 주장이 된다.
/// 이 축은 명령이 없으므로 "강제한다/잡는다" 는 **집행 주장**만 표지로 삼는다.
const ENFORCE_MARKERS: &[&str] = &[
    "CI 강제",
    "CI 에서 강제",
    "CI 가 강제",
    "CI 로 강제",
    "CI 가 잡",
    "CI 에서 잡",
    "CI 가 막",
    "CI 에서 막",
    "CI 가 차단",
    "CI 에서 차단",
    "CI fail",
    "CI 가 fail",
    "CI 에서 fail",
    "(CI)",
];

/// 그 문장이 주장하는 것이 **실행이 아니라 컴파일/검사**인가.
///
/// 통합 테스트에 대해 자동 잡이 하는 일은 둘로 갈린다: **실행은 안 하지만 컴파일은
/// 한다**(Windows·headless 의 `clippy --all-targets` 가 `tests/*.rs` 를 타깃으로 잡는다).
/// 그래서 "이 테스트를 돌린다" 는 거짓이지만 "컴파일은 본다" 는 참이다. 이 구분을 빼면
/// 가드가 **참인 문장을 지우게 만든다** — 이 파일이 막으려는 실패의 거울상이다.
const COMPILE_CLAIM_MARKERS: &[&str] = &["컴파일", "clippy", "빌드", "compile"];

/// 그 자리가 **자동 채널을 이미 긍정하고 있는가** — 역방향 검사의 면제 조건.
///
/// 한 문장이 두 가드의 채널을 대비해 설명하면(이쪽은 자동으로 돈다, 저쪽은 아니다)
/// 창 안에 부재 표지와 lib 테스트 이름이 함께 놓인다. 그건 약하게 쓴 것이 아니라
/// **정확하게** 쓴 것이다.
const AUTOMATIC_CHANNEL_MARKERS: &[&str] = &[
    "--lib --bins",
    "자동으로 돈다",
    "자동으로 돌린다",
    "crossplatform-check",
];

/// 그 서술이 **자동 실행을 적극적으로 주장**하는가 — 부재 표지의 면제를 무효로 만드는 축.
///
/// [`AUTOMATIC_CHANNEL_MARKERS`] 와 목록이 겹치지만 같지 않다. 저기엔 `--lib --bins` ·
/// `crossplatform-check` 같은 **참조**가 들어 있는데, 잡이나 조합을 가리키는 것은 주장이
/// 아니다 — "자동 채널 없음. 그건 `crossplatform-check` 의 잡이 배선했다" 는 정확한 서술이고
/// 참조 때문에 모순이 되지 않는다. 이 축은 **서술어**만 표지로 삼는다.
const AFFIRMATIVE_RUN_MARKERS: &[&str] = &["자동으로 돈다", "자동으로 돌린다", "✅ 자동"];

/// 같은 서술의 부재 표지가 **면제로 성립하는가.**
///
/// 부재 표지가 서술 안에 있기만 하면 면제하던 형태에는 구멍이 있었다. 한 서술이 자동 실행과
/// 부재를 **동시에** 말해도 면제가 성립했다. 표 행은 한 서술이므로 범위 판정은 정확했고,
/// 틀린 것은 **범위 안의 모순을 준수로 읽은** 쪽이다 — 그래서 범위를 더 좁히는 것(글자 창 →
/// 서술)으로는 안 닫힌다. 가드가 초록인데 문장이 거짓인 형태다.
/// 재현형은 [`a_scope_that_asserts_both_automatic_and_absent_is_not_exempt`] 에 있다.
///
/// 면제와 검출의 부담이 다르다는 것이 이 판정의 근거다. **면제는 모호하면 안 된다** —
/// 어느 쪽이 그 주장을 한정하는지 사람이 못 가르는 서술은 면제받을 자격이 없고, 갈라 쓰면
/// 된다. 반대로 [`weak_absence_offsets`] 의 **출발점** 자격은 그대로 둔다: 긍정 표지가 함께
/// 있어도 그 자리가 부재를 말한 것은 여전히 참이라, 출발점을 좁히면 역방향 검사에 거짓
/// 음성만 는다.
fn absence_exempts(scope: &str) -> bool {
    ABSENCE_MARKERS.iter().any(|m| scope.contains(m))
        && !AFFIRMATIVE_RUN_MARKERS.iter().any(|m| scope.contains(m))
}

/// lib 유닛 테스트 이름 추출의 하한 — 추출이 깨지면 역방향 검사가 통째로 잠잠해진다.
const MIN_LIB_TESTS: usize = 100;

/// 스캔 하한 — 수집이나 인용 추출이 조용히 줄어드는 것을 잡는다.
///
/// 가드가 "위반 0" 을 보고하는 이유는 두 가지다: 정말 없거나, **아무것도 안 봤거나.**
/// 둘을 구분하지 않으면 스캔이 깨진 날 초록이 뜬다.
const MIN_SCANNED_FILES: usize = 400;
/// 같은 이유의 하한 — 통합 테스트 파일 인용 지점 수.
const MIN_TEST_CITATIONS: usize = 40;

/// 인용된 명령이 **좁혀진 조합**인가 — `--lib`/`--bins`/`--test` 로 좁힌 형태는 실제로
/// 자동으로 돈다(`crossplatform-check.yml` 의 Windows·headless 잡, semver 가드).
fn is_narrowed(tail: &str) -> bool {
    logical_command(tail)
        .split_whitespace()
        .any(|w| w.starts_with("--lib") || w.starts_with("--bins") || w.starts_with("--test"))
}

/// 인용 지점부터 **그 명령이 끝나는 곳까지** — YAML 의 백슬래시 연속행을 이어 붙인다.
///
/// 판정 범위를 상수로 잡지 않는다. 이 함수의 앞선 두 형태가 각각 상수 하나씩을 들고
/// 있다가 밀렸다: 앞 N 단어만 보던 형태는 `--no-fail-fast` 가 추가된 날 좁힘을 놓쳤고,
/// 그것을 고치며 들어온 "물리적 한 줄" 형태는 연속행에 놓인 플래그를 못 본다. 두 번째
/// 형태의 주석은 "명령 끝까지 본다" 고 적고 있었는데 — **고정 개수를 지웠을 뿐 고정
/// 범위를 안 지웠다.** 끝나는 자리를 셸과 같은 규칙(줄 끝 `\` 가 없으면 거기서 끝)으로
/// 구조에서 끌어내면 남는 상수가 없다.
///
/// 다음 YAML 스텝까지 끌어오지는 않는다 — 연속 표시가 없는 줄에서 멈추므로, 아래에
/// 놓인 다른 `run:` 의 플래그가 이 명령의 판정에 섞이지 않는다.
fn logical_command(tail: &str) -> String {
    let mut out = String::new();
    for line in tail.lines() {
        let trimmed = line.trim_end();
        let continues = trimmed.ends_with('\\');
        out.push_str(trimmed.trim_end_matches('\\'));
        out.push(' ');
        if !continues {
            break;
        }
    }
    out
}

/// 텍스트에서 "전체 스위트를 CI 가 돌린다" 는 주장의 바이트 오프셋들.
fn claim_offsets(text: &str, path: &str) -> Vec<usize> {
    const NEEDLE: &str = "cargo test --workspace";
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(NEEDLE) {
        let at = from + rel;
        from = at + NEEDLE.len();
        if is_narrowed(&text[from..]) {
            continue;
        }
        // 주장은 산문이다. Rust 소스의 문자열 리터럴·식별자는 검사 로직 자신일 뿐이라
        // 여기서 걸러진다 — 그래서 이 가드는 자기 자신을 위한 경로 면제가 필요 없다.
        if !is_prose_line(text, at, path) {
            continue;
        }
        // 표지도 **같은 서술 안**에 있어야 한다. 글자 수 창(`CLAIM_WINDOW`)으로 보던
        // 앞 형태는 문단을 넘어가 **다른 아이템의 코드 줄**에서 표지를 주웠다: 파서가
        // 명령 문자열을 어떻게 잘못 읽는지 설명하는 doc 주석이, 그 위 상수 리터럴 안의
        // `"test.yml"` 때문에 "CI 가 전체 스위트를 돌린다는 주장" 으로 고발됐다.
        // 주장은 한 서술 안에서 성립한다 — 근처에 있다고 성립하지 않는다. 부재 표지가
        // 이미 같은 규칙을 쓰고 있었고, 한 판정을 두 자리에서 다른 규칙으로 하면 그중
        // 하나가 조용히 낡는다.
        let scope = claim_scope(text, at);
        if !CI_MARKERS.iter().any(|m| scope.contains(m)) {
            continue;
        }
        // 같은 서술이 부재를 함께 말하면 정당하다 — 다만 그 서술이 자동 실행도 함께
        // 주장하면 모순이라 면제되지 않는다.
        if absence_exempts(scope) {
            continue;
        }
        found.push(at);
    }
    found
}

/// 바이트 오프셋 → 1-기준 줄 번호.
fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].lines().count().max(1)
}

/// 자동 트리거를 가진 워크플로의, `workflow_dispatch` 로 좁혀지지 않은 잡 본문.
///
/// yml 을 파싱하지 않고 들여쓰기로 잡 경계를 잡는다 — 이 레포의 워크플로는 전부
/// `jobs:` 아래 2 칸 들여쓰기의 평평한 잡 목록이고, 파싱기를 들이는 것보다 이 구조를
/// 깨뜨렸을 때 눈에 띄는 편이 낫다.
/// 그 경로가 워크플로 파일인가 — GitHub Actions 가 인정하는 두 확장자를 모두 본다.
fn is_workflow_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("yml") | Some("yaml")
    )
}

fn automatic_job_bodies_of_dir(workflows: &Path) -> Vec<String> {
    let mut bodies = Vec::new();
    let Ok(entries) = std::fs::read_dir(workflows) else {
        panic!("워크플로 디렉토리를 읽지 못했다: {}", workflows.display());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // GitHub Actions 는 `.yml` 과 `.yaml` 을 **둘 다** 워크플로로 읽는다. 여기서 한쪽만
        // 보면 그 잡은 자동 채널 계산에서 통째로 빠지고, 그러면 이 가드의 세 축이 모두
        // 그 잡을 못 본 채 판정한다 — 실제보다 **약한** 채널을 가정하게 되므로 거짓
        // 위반(참인 서술을 짚음)이 난다.
        //
        // 이 파일 자신이 그 비대칭을 갖고 있었다: 스캔 대상 확장자([`TEXT_EXTS`])에는
        // `yaml` 이 있어서, `.yaml` 워크플로는 **문서로는 읽히고 워크플로로는 안 읽히는**
        // 상태였다. 한 파일 안에서 두 모수가 어긋나 있으면 어느 쪽이 옳은지 읽는 사람이
        // 판단할 수 없다.
        if !is_workflow_file(&path) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        // 트리거 판정: `on:` 블록에 push/pull_request/schedule 중 하나라도 있으면 자동.
        let head: String = text
            .lines()
            .take_while(|l| !l.starts_with("jobs:"))
            .collect();
        if !(head.contains("push:") || head.contains("pull_request:") || head.contains("schedule:"))
        {
            continue;
        }
        // 잡 분할과 수동 전용 제외는 **라이브러리 한 벌**이 한다. 한때 여기 사본이
        // 있었고 lib 판과 문자 그대로 같았다 — 같은 물음에 답이 둘이면 갈릴 때까지만
        // 같다. 파일 단위 트리거 판정(바로 위)은 이 가드의 물음("자동 채널이 있는가")에
        // 속하므로 여기 남는다. lib 쪽은 "매 push 도는가" 라 판정이 더 좁다.
        bodies.extend(automatic_job_bodies(&text));
    }
    bodies
}

/// 한 잡 본문이 **좁혀지지 않은** `cargo test --workspace` 를 돌리는가.
///
/// 판정을 [`is_narrowed`] 에 넘긴다 — 이 함수가 들고 있던 `take(4)` 는 [`logical_command`]
/// 가 지운 것과 **같은 종류의 고정 범위**였다. 좁힘 플래그가 다섯 번째 이후에 오거나
/// 백슬래시 연속행에 놓이면 못 봤고, 그러면 좁혀진 스텝을 "전체 스위트" 로 읽어 이
/// 가드의 첫 번째 축이 통째로 잠잠해진다. 판정 하나를 두 자리에서 서로 다른 규칙으로
/// 하고 있었던 것이 결함이다.
fn a_job_body_runs_the_full_suite(body: &str) -> bool {
    cargo_test_tails(body)
        .iter()
        .any(|tail| a_full_suite_invocation(tail))
}

/// 한 `cargo test` 호출이 전체 스위트인가 — 인자 꼬리만 보는 형태.
///
/// `cargo clippy --workspace` 처럼 `cargo test` 가 아닌 명령은 [`cargo_test_tails`] 에서
/// 이미 걸러진다. 여기까지 온 것은 전부 테스트 호출이다.
fn a_full_suite_invocation(tail: &str) -> bool {
    tail.contains("--workspace") && !is_narrowed(tail)
}

/// **기본 조합**의 자동 잡이 전체 스위트를 돌리는가.
///
/// 조합을 구별해야 한다. 문서가 인용하는 `cargo test --workspace` 는 기본 조합의
/// 명령이고, 헤드리스 잡이 도는 것은 `--no-default-features` 조합이다. 둘을 하나로 보면
/// **헤드리스가 전체 스위트를 돌리기 시작한 날 이 축이 통째로 침묵한다** — 실제로 그랬다.
/// 조합을 안 보는 판정은 "주장이 참이 됐다" 와 "다른 조합에서만 참이다" 를 못 가른다.
fn ci_actually_runs_the_full_suite(root: &Path) -> bool {
    automatic_test_invocations(root)
        .iter()
        .any(|(combo, tail)| *combo == Combo::Default && a_full_suite_invocation(tail))
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        panic!("디렉토리를 읽지 못했다: {}", dir.display());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) || name.starts_with('.') && name != ".github" {
                continue;
            }
            collect_files(&path, out);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| TEXT_EXTS.contains(&e))
        {
            out.push(path);
        }
    }
}

/// 잡 본문에서 `cargo test` 호출 하나하나의 **인자 꼬리**를 뽑는다.
///
/// 스텝 경계(`- name:`)까지를 한 호출로 본다 — 워크플로가 `run: >` 접힘 문법으로 한
/// 명령을 여러 줄에 걸쳐 쓰기 때문에 줄 단위로 끊으면 `--test` 열거가 잘려 나간다.
/// 한 잡 본문의 `run:` 블록을 **논리적 명령 목록**으로 편다.
///
/// 앞 형태는 본문에서 `"cargo test"` 를 문자열로 찾았는데, 그 문자열은 명령에만 있는
/// 것이 아니다 — **스텝 이름**(`- name: cargo test (unit)`)에도 있다. 사람이 읽으라고
/// 붙인 라벨이 명령으로 파싱됐고, 라벨에는 좁힘 플래그가 없으니 "안 좁혀진 호출" 로
/// 읽혔다. 그 하나 때문에 [`integration_tests_run_automatically`] 가 즉시 `None` 을 냈고
/// **두 번째 축이 통째로 침묵**했다 — 침묵의 이유가 사실이 아니라 파싱이었다.
///
/// 그리고 YAML 스칼라 방식을 봐야 한다. `run: >`(folded)는 줄바꿈이 공백이 되어 여러
/// 줄이 **한 명령**이고, `run: |`(literal)은 줄이 그대로 남아 셸의 `\` 규칙이 적용된다.
/// 접힘을 모르면 `test.yml` 의 semver 가드처럼 `--test` 셋이 다음 줄에 놓인 명령을
/// "안 좁혀진 전체 스위트" 로 읽는다. 스칼라 방식은 구조에서 읽으므로 상수가 없다.
fn run_commands(body: &str) -> Vec<String> {
    let indent_of = |l: &str| l.len() - l.trim_start().len();
    let lines: Vec<&str> = body.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let bare = line.trim_start();
        let bare = bare.strip_prefix("- ").unwrap_or(bare);
        let Some(rest) = bare.strip_prefix("run:") else {
            i += 1;
            continue;
        };
        let base = indent_of(line);
        let head = rest.trim();
        // 블록 지시자(`|`/`>`, chomping 접미사 포함)인가, 아니면 명령이 그 자리에서
        // 시작하는가. 지시자가 없는 평문 스칼라도 이어지는 줄은 접힌다.
        let literal = head.starts_with('|');
        let block = literal || head.starts_with('>');
        let mut collected: Vec<String> = Vec::new();
        if !block && !head.is_empty() {
            collected.push(head.to_string());
        }
        i += 1;
        while i < lines.len() {
            let l = lines[i];
            if l.trim().is_empty() {
                collected.push(String::new());
                i += 1;
                continue;
            }
            if indent_of(l) <= base {
                break;
            }
            collected.push(l.trim().to_string());
            i += 1;
        }
        let mut cur = String::new();
        for l in &collected {
            if l.is_empty() {
                if !cur.trim().is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                cur.clear();
                continue;
            }
            let continues = !literal || l.ends_with('\\');
            cur.push_str(l.trim_end_matches('\\'));
            cur.push(' ');
            if !continues {
                out.push(std::mem::take(&mut cur));
            }
        }
        if !cur.trim().is_empty() {
            out.push(cur);
        }
    }
    out
}

/// `run:` 안의 `cargo test` 호출들 — 명령 이름 뒤의 인자 꼬리로 돌려준다.
fn cargo_test_tails(body: &str) -> Vec<String> {
    run_commands(body)
        .iter()
        .filter_map(|cmd| cmd.split_once("cargo test").map(|(_, t)| t.to_string()))
        .collect()
}

/// 자동 잡이 테스트를 돌릴 때 쓰는 **feature 조합**.
///
/// "자동 실행 채널이 있는가" 는 더 이상 단일 참·거짓이 아니다. 기본 조합의 자동 잡은
/// `--lib --bins` 라 통합 테스트를 하나도 안 돌리고, 헤드리스 조합의 자동 잡은 전체
/// 스위트를 돌린다. 하나로 뭉개면 어느 쪽으로 적어도 반쪽이 거짓이 된다.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Combo {
    /// 기본 feature 조합 — 문서가 인용하는 `cargo test --workspace` 가 이것이다.
    Default,
    /// `--no-default-features`.
    Headless,
}

impl Combo {
    fn label(self) -> &'static str {
        match self {
            Combo::Default => "기본 조합",
            Combo::Headless => "헤드리스 조합",
        }
    }
}

/// 자동 잡의 `cargo test` 호출들 — (조합, 인자 꼬리).
fn automatic_test_invocations(root: &Path) -> Vec<(Combo, String)> {
    let mut out = Vec::new();
    for body in automatic_job_bodies_of_dir(&root.join(".github/workflows")) {
        for tail in cargo_test_tails(&body) {
            let combo = if tail.contains("--no-default-features") {
                Combo::Headless
            } else {
                Combo::Default
            };
            out.push((combo, tail));
        }
    }
    out
}

/// 자동 잡이 **이름을 지목해** 돌리는 통합 테스트 이름들.
///
/// `None` 은 "이 축이 성립하지 않는다" 는 뜻이다 — 자동 잡 중 하나가 좁혀지지 않은
/// `cargo test` 를 돌리면 통합 테스트가 전부 자동으로 도는 것이므로 어떤 인용도 거짓이
/// 아니다. 그때 이 가드는 첫 번째 축과 같은 방식으로 스스로 잠잠해진다.
/// 이 `cargo test` 호출이 **일부로 좁혀졌는가** — 타깃으로든 패키지로든.
///
/// ★ `-p` / `--package` 를 세지 않던 때가 있었고, 그래서 `cargo test -p tasty-doc-guards`
/// 하나가 [`integration_tests_run_automatically`] 를 `None` 으로 만들었다. 그 `None` 의
/// 뜻은 "통합 테스트가 **전부** 자동으로 돈다" 라서, 한 패키지만 도는 호출이 그 결론을
/// 낸 것은 틀린다 — 그리고 그 틀림의 결과는 판정 둘이 **조용히 조기 반환**하는 것이다.
/// 실측으로 잡았다(2026-09-05): 잠잠하게 만든 호출에 `--workspace` 가 있는지 **다른
/// 성질로** 물었더니 없었다.
///
/// 함수로 뺀 이유는 [`the_self_silencing_axis_names_what_silenced_it`] 이 같은 판정을
/// 물어야 하기 때문이다. 거기서 다시 쓰면 사본이 되고, 사본은 원본보다 단순해서 **덜
/// 잡는 쪽으로** 갈린다 — 그러면 "축이 왜 잠잠한가" 를 묻는 검사가 틀린 답으로 안심시킨다.
fn tail_is_narrowed(tail: &str) -> bool {
    tail.split_whitespace().any(|w| {
        w.starts_with("--lib")
            || w.starts_with("--bins")
            || w == "--test"
            || w == "-p"
            || w == "--package"
    })
}

fn integration_tests_run_automatically(root: &Path) -> Option<std::collections::BTreeSet<String>> {
    let mut named = std::collections::BTreeSet::new();
    {
        for (_combo, tail) in automatic_test_invocations(root) {
            let words: Vec<&str> = tail.split_whitespace().collect();
            if !tail_is_narrowed(&tail) {
                return None;
            }
            for pair in words.windows(2) {
                if pair[0] == "--test" {
                    named.insert(pair[1].to_string());
                }
            }
        }
    }
    Some(named)
}

/// 자동 잡이 쓰는 플래그 중 **이 모델이 다루지 못하는** 것.
///
/// feature 를 명시적으로 켜는 형태가 들어오면 "조합은 둘" 이라는 전제가 깨진다. 그때
/// 조용히 기본값으로 읽으면 채널 수가 틀리고, 채널 수가 틀린 가드는 **틀린 근거로 남의
/// 서술을 고발한다.** 그래서 모델을 못 세우면 초록이 아니라 빨강이다 — 계측기의 고장이
/// "이상 없음" 으로 읽히면 그건 계측기가 아니다.
const UNMODELLED_TEST_FLAGS: &[&str] = &["--features", "--all-features"];

/// 그 자리가 **조합을 한정해서** 채널을 말하고 있는가.
///
/// 채널이 한 조합에만 있을 때 "자동 채널이 없다" 는 반쪽만 참이고 "자동으로 돈다" 도
/// 반쪽만 참이다. 정확히 쓰려면 어느 조합인지를 함께 적어야 하고, 그 형태를 적은 서술은
/// 이 축에서 정당하다.
const COMBO_QUALIFIED_MARKERS: &[&str] = &[
    "check-headless",
    "헤드리스 조합에서만",
    "헤드리스에서만",
    "기본 조합에는",
    "기본 조합 전용",
    "조합에서만",
    // 채널이 하나라고 **세어서** 적은 형태. "…에서만" 과 뜻이 같은데 문자열이 다르다.
    // 성질로 가르는 쪽(구체적 조합 선택자를 부르는가)은 실측에서 기각됐다 — 낡은 서술도
    // 같은 서술 안에 `--lib --bins` 를 담고 있어서, 성질로 가르면 **낡은 것을 면제한다.**
    // 한정자가 어느 주장에 붙는지를 못 가르는 문제라 [ADR-0144] 와 같은 벽이다. 목록이
    // 자라는 대가는 그 ADR 의 재검토 조건이 받는다.
    "조합 하나",
];

/// `key = "value"` 한 줄.
fn toml_string(block: &str, key: &str) -> Option<String> {
    for line in block.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        return Some(rest.trim().trim_matches('"').to_string());
    }
    None
}

/// `key = ["a", "b"]` 한 줄.
fn toml_array(block: &str, key: &str) -> Vec<String> {
    for line in block.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(key) else {
            continue;
        };
        let Some(rest) = rest.trim_start().strip_prefix('=') else {
            continue;
        };
        let rest = rest.trim().trim_start_matches('[').trim_end_matches(']');
        return rest
            .split(',')
            .map(|w| w.trim().trim_matches('"').to_string())
            .filter(|w| !w.is_empty())
            .collect();
    }
    Vec::new()
}

/// 워크스페이스의 매니페스트 경로들 — 루트 + `crates/*`.
fn manifests(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.join("Cargo.toml")];
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        let mut found: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path().join("Cargo.toml"))
            .filter(|m| m.is_file())
            .collect();
        found.sort();
        out.extend(found);
    }
    out
}

/// `[[test]]` 타깃 이름 -> (요구 feature, 그 패키지의 default feature).
///
/// 조합별로 **빌드되는지**가 여기서 갈린다. `required-features` 가 걸린 타깃은 헤드리스
/// 조합(`--no-default-features`)에서 아예 만들어지지 않으므로 실행 채널도 없다.
fn test_target_features(
    root: &Path,
) -> std::collections::BTreeMap<String, (Vec<String>, Vec<String>)> {
    let mut out = std::collections::BTreeMap::new();
    for manifest in manifests(root) {
        let Ok(text) = std::fs::read_to_string(&manifest) else {
            continue;
        };
        let defaults = text
            .split_once("\n[features]")
            .map(|(_, rest)| toml_array(rest.split("\n[").next().unwrap_or(rest), "default"))
            .unwrap_or_default();
        for block in text.split("[[test]]").skip(1) {
            let block = block.split("\n[").next().unwrap_or(block);
            let required = toml_array(block, "required-features");
            if required.is_empty() {
                continue;
            }
            if let Some(name) = toml_string(block, "name") {
                out.insert(name, (required, defaults.clone()));
            }
        }
    }
    out
}

/// `--skip` 인자로 지목된 이름들.
fn skip_names(tail: &str) -> Vec<String> {
    let words: Vec<&str> = tail.split_whitespace().collect();
    words
        .windows(2)
        .filter(|p| p[0] == "--skip")
        .map(|p| p[1].to_string())
        .collect()
}

/// `--` 뒤에 놓인 **양성 필터**(이름 인자)들과 `--exact` 여부.
///
/// `--skip` 의 대칭이다. `--skip X` 가 "X 를 빼고 나머지" 라면 이쪽은 "이것만" 이고,
/// 그래서 채널 판정에 주는 효과도 대칭이어야 한다 — skip 이 **전부**를 덮으면 채널이
/// 아니듯, 양성 필터가 **일부만** 고르면 그 호출은 타깃 전체의 채널이 아니다.
///
/// 이 구분이 없으면 한 테스트만 지목한 스텝이 그 타깃 전체의 실행 채널로 세어지고,
/// 그 타깃에 새 테스트가 생겨도 자동으로 돈다고 잘못 말하게 된다.
///
/// ## 층을 가른다 — **저장은 이름으로, 판정은 성질로**
///
/// 이름이 여기 나오는 것 자체는 정당하다. cargo 의 인터페이스가 이름이고
/// (`--test <타깃>`, `-- <필터>`), 어느 타깃·어느 테스트를 가리키는지를 **적어 두는**
/// 층에서는 이름 말고 쓸 것이 없다. 이 함수가 하는 일이 그 층이다 — 이름을 **꺼낸다.**
///
/// 틀리는 것은 그 다음 층이다. **"채널이 있는가" 를 이름의 등장으로 판정하면 샌다** —
/// 명령에 `--test e2e_tests` 라는 **글자**가 있다는 것과 그 호출이 그 타깃을 **다 돌린다**는
/// 것은 다른 명제다. 그래서 호출부는 이름이 아니라 성질로 묻는다: *이 호출이 그 타깃의
/// 모든 `#[test]` 를 실제로 덮는가.* 덮으면 채널이고, 좁히면 아니다.
///
/// 같은 층 혼동이 이 저장소에서 반복해서 났다(이름 모양으로 면제·분류를 판정한 자리들).
/// 규칙은 하나다 — **이름은 무엇을 가리키는지 말할 뿐, 그것이 무엇인지는 말하지 않는다.**
fn positive_filters(tail: &str) -> (Vec<String>, bool) {
    let words: Vec<&str> = tail.split_whitespace().collect();
    let Some(sep) = words.iter().position(|w| *w == "--") else {
        return (Vec::new(), false);
    };
    let after = &words[sep + 1..];
    let exact = after.iter().any(|w| *w == "--exact");
    let mut filters = Vec::new();
    let mut i = 0;
    while i < after.len() {
        let w = after[i];
        // 값을 하나 먹는 harness 플래그는 그 값까지 건너뛴다.
        if w == "--skip" || w == "--test-threads" || w == "--logfile" || w == "--format" {
            i += 2;
            continue;
        }
        if w.starts_with('-') {
            i += 1;
            continue;
        }
        filters.push(w.to_string());
        i += 1;
    }
    (filters, exact)
}

/// 한 파일에서 `#[test]` 가 붙은 함수 이름들.
fn test_fn_names(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    let mut names = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() != "#[test]" {
            continue;
        }
        for next in lines.iter().skip(i + 1).take(4) {
            let t = next.trim_start();
            let t = t.strip_prefix("async ").unwrap_or(t);
            if let Some(rest) = t.strip_prefix("fn ")
                && let Some(name) = rest.split(['(', '<']).next()
                && !name.is_empty()
            {
                names.push(name.to_string());
                break;
            }
        }
    }
    names
}

/// `#[test]` 이름과 그 자리에 `#[ignore]` 가 붙었는지.
///
/// [`test_fn_names`] 와 갈라 두는 이유: 저쪽은 "이 타깃에 어떤 테스트가 있나" 를 묻고
/// 이쪽은 "그 테스트가 **평범한 `cargo test` 로 도는가**" 를 묻는다. 두 물음의 답이
/// 다르고, 뒤쪽을 앞쪽으로 대신하면 `#[ignore]` 33 건이 "돈다" 로 세어진다 —
/// **모수를 줄이는 방향의 어긋남은 언제나 초록으로 나오므로** 아무도 안 본다.
fn test_fns_with_ignore(text: &str) -> Vec<(String, bool)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.trim() != "#[test]" {
            continue;
        }
        let mut ignored = false;
        for next in lines.iter().skip(i + 1).take(4) {
            let t = next.trim_start();
            if t.starts_with("#[ignore") {
                ignored = true;
                continue;
            }
            let t = t.strip_prefix("async ").unwrap_or(t);
            if let Some(rest) = t.strip_prefix("fn ")
                && let Some(name) = rest.split(['(', '<']).next()
                && !name.is_empty()
            {
                out.push((name.to_string(), ignored));
                break;
            }
        }
    }
    out
}

/// 줄바꿈·연속 공백을 한 칸으로 접은 사본. 마크다운 본문은 문장이 여러 줄에 걸쳐
/// 접히므로, 문구를 원문에서 그대로 찾으면 **있는 것을 없다고** 판정한다(실측으로 밟았다).
fn unwrapped(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "N 통과" 꼴 — 숫자와 통과/passed 가 붙어 있는 자리.
fn states_a_pass_count(flat: &str) -> bool {
    let bytes: Vec<char> = flat.chars().collect();
    for marker in ["통과", "passed"] {
        let mut from = 0;
        while let Some(pos) = flat[from..].find(marker) {
            let at = from + pos;
            from = at + marker.len();
            let head = flat[..at].chars().count();
            let lo = head.saturating_sub(6);
            if bytes[lo..head].iter().any(|c| c.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

/// 마크다운 **절** 단위로 "gui 스위트의 통과 수를 적었다" 를 찾는다. 반환은 1-기반 줄 번호.
///
/// ## 왜 파일 단위가 아니라 절 단위인가
///
/// 파일 단위로 물으면 한 문서 안의 **무관한 두 문장**이 서로를 위반으로 만든다. 실측으로
/// 밟았다 — `docs/dev-guide/e2e-tests.md` 는 스위트 단위 통과 수를 앞 절에 적고
/// `gui_tests` 는 3.5k 자 뒤의 다른 절에서 언급하는데, 둘은 같은 것을 말하지 않는다.
/// 반대로 "N 자 이내" 같은 문자 창을 쓰면 그 N 이 곧 마법의 수가 된다. 문서의 heading
/// 구조가 이미 범위를 주고 있으므로 그것을 쓴다.
///
/// ## 두 범위를 다르게 잡는다 — 위반은 좁게, 면제는 넓게
///
/// 위반은 **고유 범위**(다음 heading 직전까지, 하위 절 제외)에서 찾고, 표지는
/// **포함 범위**(같거나 얕은 다음 heading 직전까지, 하위 절 포함)에서 찾는다. 수를 적은
/// 절 바로 아래에 "그 수가 왜 단일 값이 아닌가" 를 푸는 하위 절을 두는 것은 정상적인
/// 문서 구조다 — 그것을 위반으로 세면 규칙이 잘 쓴 글을 벌한다. 반대로 **옆 절**의 표지는
/// 끌어오지 않는다(그 함정은 이 파일의 다른 가드에서 실측으로 두 번 샜다).
fn gui_pass_counts_missing_marker(text: &str, marker: &str) -> Vec<usize> {
    let lines: Vec<&str> = text.lines().collect();
    // (시작 줄, heading 레벨). 첫 heading 앞의 머리말은 자식을 가질 수 없으므로 가장 깊은
    // 레벨로 두어 고유 범위와 포함 범위가 같아지게 한다.
    let mut heads: Vec<(usize, usize)> = vec![(0, usize::MAX)];
    let mut fence = false;
    for (i, ln) in lines.iter().enumerate() {
        if ln.trim_start().starts_with("```") {
            fence = !fence;
            continue;
        }
        if fence {
            continue;
        }
        // 마크다운은 들여쓰기 3칸까지를 heading 으로 보고 4칸부터는 코드 블록으로 본다.
        let indent = ln.len() - ln.trim_start_matches(' ').len();
        let body = &ln[indent..];
        let hashes = body.chars().take_while(|c| *c == '#').count();
        if indent <= 3 && (1..=6).contains(&hashes) && body.chars().nth(hashes) == Some(' ') {
            heads.push((i, hashes));
        }
    }

    let mut out = Vec::new();
    for (k, &(start, level)) in heads.iter().enumerate() {
        let own_end = heads.get(k + 1).map_or(lines.len(), |&(i, _)| i);
        let own = unwrapped(&lines[start..own_end].join("\n"));
        if !own.contains("gui_tests") || !states_a_pass_count(&own) {
            continue;
        }
        let scope_end = heads[k + 1..]
            .iter()
            .find(|&&(_, l)| l <= level)
            .map_or(lines.len(), |&(i, _)| i);
        let scope = unwrapped(&lines[start..scope_end].join("\n"));
        if !scope.contains(marker) {
            out.push(start + 1);
        }
    }
    out
}

/// 통합 테스트 타깃 이름 -> 그 소스 경로.
fn integration_target_path(root: &Path, name: &str) -> Option<PathBuf> {
    let direct = root.join("tests").join(format!("{name}.rs"));
    if direct.is_file() {
        return Some(direct);
    }
    for entry in std::fs::read_dir(root.join("crates")).ok()?.flatten() {
        let path = entry.path().join("tests").join(format!("{name}.rs"));
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

/// 호출이 `-p` / `--package` 로 좁혀졌다면 그 패키지 이름들.
///
/// 이 축이 없던 동안 `cargo test -p <어떤것> --locked` 은 **좁혀지지 않은 호출**로 읽혔다.
/// `--lib`/`--bins`/`--test` 중 아무것도 없기 때문이다. 그래서 한 패키지만 도는 잡 하나가
/// 레포의 **모든** 통합 타깃에 채널을 주는 것으로 계산됐고, 그 결과 참인 "자동 채널 없음"
/// 서술들이 무더기로 거짓으로 고발됐다. 실측으로 잡았다 — `doc-guards.yml` 을 넣은 회차에
/// 무관한 타깃 여덟 자리가 한꺼번에 걸렸다.
fn packages_named(tail: &str) -> Vec<String> {
    let logical = logical_command(tail);
    let words: Vec<&str> = logical.split_whitespace().collect();
    let mut out: Vec<String> = words
        .windows(2)
        .filter(|w| w[0] == "-p" || w[0] == "--package")
        .map(|w| w[1].to_string())
        .collect();
    out.extend(
        words
            .iter()
            .filter_map(|w| {
                w.strip_prefix("--package=")
                    .or_else(|| w.strip_prefix("-p="))
            })
            .map(str::to_string),
    );
    out
}

/// 매니페스트의 `[package] name`.
fn package_name(manifest: &Path) -> Option<String> {
    let text = std::fs::read_to_string(manifest).ok()?;
    let block = text.split("[package]").nth(1)?;
    let block = block.split("\n[").next()?;
    toml_string(block, "name")
}

/// 그 통합 타깃 소스가 **어느 패키지 소속인가.**
///
/// `<root>/tests/x.rs` 면 루트 패키지, `<root>/crates/<디렉토리>/tests/x.rs` 면 그 크레이트다.
/// 디렉토리 이름이 곧 패키지 이름이라고 가정하지 않는다 — 매니페스트에서 읽는다.
fn owning_package(root: &Path, source: &Path) -> Option<String> {
    let rel = source.strip_prefix(root).ok()?;
    let mut parts = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string());
    match parts.next()?.as_str() {
        "tests" => package_name(&root.join("Cargo.toml")),
        "crates" => {
            let dir = parts.next()?;
            package_name(&root.join("crates").join(dir).join("Cargo.toml"))
        }
        _ => None,
    }
}

/// 그 통합 테스트가 **자동으로 도는 조합들.**
///
/// 판정의 단위가 조합인 이유는 [`Combo`] 에 적었다. 여기서 세 가지를 함께 본다:
/// 조합에서 그 타깃이 **빌드되는가**(`required-features`), 그 호출이 통합 테스트를
/// **돌리는가**(`--lib --bins` 로 좁혀졌으면 안 돌린다), `--skip` 이 그 타깃을 **통째로
/// 걷어내는가**(그 타깃의 모든 테스트 이름이 skip 에 걸리면 실행 채널이 아니다).
fn integration_target_channels(
    root: &Path,
    target: &str,
    invocations: &[(Combo, String)],
    features: &std::collections::BTreeMap<String, (Vec<String>, Vec<String>)>,
) -> std::collections::BTreeSet<Combo> {
    let mut out = std::collections::BTreeSet::new();
    // 그런 타깃이 없으면 채널도 없다. 자리표시자(`tests/X.rs`)를 실재하는 타깃으로 읽으면
    // 가드가 **존재하지 않는 테스트의 채널**을 근거로 고발한다 — 실측으로 이 파일 자신의
    // 모듈 doc 이 그렇게 잡혔다.
    let Some(source) = integration_target_path(root, target) else {
        return out;
    };
    let owner = owning_package(root, &source);
    for (combo, tail) in invocations {
        // `-p` 로 좁힌 호출은 그 패키지의 타깃만 돌린다.
        let named_packages = packages_named(tail);
        if !named_packages.is_empty() && !owner.as_ref().is_some_and(|o| named_packages.contains(o))
        {
            continue;
        }
        if let Some((required, defaults)) = features.get(target) {
            let enabled: &[String] = match combo {
                Combo::Default => defaults,
                Combo::Headless => &[],
            };
            if !required.iter().all(|r| enabled.contains(r)) {
                continue;
            }
        }
        let words: Vec<&str> = tail.split_whitespace().collect();
        let named: Vec<&str> = words
            .windows(2)
            .filter(|p| p[0] == "--test")
            .map(|p| p[1])
            .collect();
        if named.is_empty() {
            // `--lib`/`--bins` 로 좁힌 호출은 통합 타깃을 하나도 만들지 않는다.
            if words.iter().any(|w| *w == "--lib" || *w == "--bins") {
                continue;
            }
        } else if !named.contains(&target) {
            continue;
        }
        let skips = skip_names(tail);
        if !skips.is_empty()
            && let Ok(text) = std::fs::read_to_string(&source)
        {
            let names = test_fn_names(&text);
            if !names.is_empty() && names.iter().all(|n| skips.iter().any(|s| n.contains(s))) {
                continue;
            }
        }
        // 양성 필터가 타깃을 **좁히면** 그 호출은 타깃 전체의 채널이 아니다(위 doc).
        let (filters, exact) = positive_filters(tail);
        if !filters.is_empty()
            && let Ok(text) = std::fs::read_to_string(&source)
        {
            let names = test_fn_names(&text);
            let covers_all = !names.is_empty()
                && names.iter().all(|n| {
                    filters
                        .iter()
                        .any(|f| if exact { n == f } else { n.contains(f) })
                });
            if !covers_all {
                continue;
            }
        }
        out.insert(*combo);
    }
    out
}

/// 자동 잡 중 **lib 유닛 테스트를 실제로 돌리는** 것이 있는가.
///
/// 역방향 축은 "lib 테스트는 자동으로 돈다" 를 상수처럼 들고 있었다 — 그 사실은 소스가
/// 아니라 워크플로에 산다. 다른 두 축은 워크플로를 읽는데 이 축만 안 읽으면, Windows 잡의
/// `--lib --bins` 가 사라지는 날 이 축은 **없는 채널을 근거로 고발한다.**
fn lib_tests_run_automatically(root: &Path) -> bool {
    let main = package_name(&root.join("Cargo.toml"));
    automatic_test_invocations(root).iter().any(|(_, tail)| {
        // `-p <다른 패키지>` 로 좁힌 잡은 본체의 lib 유닛을 안 돌린다.
        let named_packages = packages_named(tail);
        if !named_packages.is_empty() && !main.as_ref().is_some_and(|m| named_packages.contains(m))
        {
            return false;
        }
        let words: Vec<&str> = tail.split_whitespace().collect();
        if words.iter().any(|w| *w == "--lib") {
            return true;
        }
        // `--test`/`--bins` 로만 좁힌 호출은 lib 유닛 테스트를 돌리지 않는다.
        !words.iter().any(|w| *w == "--test" || *w == "--bins")
    })
}

/// 통합 테스트를 지목하면서 **그 테스트가 실제로 가진 채널보다 약하게** 적은 자리.
///
/// 판정은 **부재를 주장한 자리에서만** 출발한다. 채널을 언급하지 않는 서술이나 "여기서는
/// 판단하지 않는다" 고 적은 서술은 애초에 이 축에 들어오지 않는다 — 정직하게 유보한
/// 문장에 벌을 주면 가드가 사람을 침묵시키는 쪽으로 작동한다.
fn overstated_absence(
    text: &str,
    path: &str,
    channels_of: &dyn Fn(&str) -> std::collections::BTreeSet<Combo>,
    class_channels: &std::collections::BTreeSet<Combo>,
) -> Vec<(usize, String)> {
    // 경로로 면제하지 않는다. 이 축의 대상은 **통합 타깃**이고, cargo 는 통합 타깃을
    // `tests/X.rs` 라는 위치로 정의한다 — 그러므로 `src/` 파일은 이 축의 대상을 정의할
    // 수 없고, "정의 파일 자신" 이라는 면제 사유가 여기서는 성립하지 않는다.
    //
    // 앞 형태는 `src/` 를 통째로 건너뛰었다. 실측하니 그 면제를 걷어도 걸리는 자리가
    // **0** 이었다 — 방패가 둘 겹쳐 있었고 위쪽(클래스 지목)이 먼저 막고 있었기 때문이다.
    // 그래서 이 면제는 크기를 줄인 것이 아니라 **효과가 없었다.** 아래 클래스 지목과
    // 함께 봐야 이 축이 값을 한다.
    //
    // 여기서 경로를 읽는 것은 이름 기반 휴리스틱이 아니다. `tests/X.rs` → 타깃 X 는
    // **cargo 의 규격**이지 이 가드의 짐작이 아니다. 한 겹 아래가 또 짐작인 판정(예:
    // 무시 목록 파일로 "생성물" 을 가르는 것)과는 갈린다 — 그쪽은 규격이 아니라 관례라
    // 기각됐다. 이 주석이 그 구분을 소유한다.
    let mut found = Vec::new();
    for at in absence_offsets(text) {
        if !is_prose_line(text, at, path) {
            continue;
        }
        let scope = claim_scope(text, at);
        // 한 서술이 여러 테스트를 지목하면 위반은 **하나**다. 지목마다 한 줄을 내면
        // 같은 문장이 세 번 고발되고, 읽는 사람은 고칠 자리가 셋이라고 오해한다.
        //
        // 어느 지목이 이 문장의 **주어**인지는 이 판정기가 알 수 없다 — 뒤에 붙은
        // "선례: tests/X.rs" 도 같은 범위에 들어온다. 그래서 주어를 단정하지 않고, 이
        // 범위 안에 자동으로 도는 통합 테스트가 있다는 사실만 말한다.
        let mut running: Vec<(String, Vec<&str>)> = Vec::new();
        let mut cited = cited_tests(scope);
        // **모듈 doc 은 자기 자신에 대한 서술이다 — 그런데 자기 이름을 안 부른다.**
        // 통합 타깃의 `//!` 는 "이 테스트는 …" 이라고 쓰지 "`tests/X.rs` 는 …" 이라고
        // 쓰지 않는다. 지목이 없다는 이유로 건너뛰면 이 축은 **가드 자신의 채널 서술을
        // 통째로 못 본다.** 실측으로 셋이 그렇게 살아남았다(옮긴 일곱 중 셋이 자기
        // 모듈 doc 에서 "자동 채널이 없다" 를 유지하고 있었는데, 그 셋은 새 잡에서
        // 실제로 돈다). 지목이 하나도 없을 때만 파일 자신을 주어로 세운다 — 남을
        // 지목한 문장의 주어를 빼앗지 않기 위해서다.
        let in_module_doc = text[text[..at].rfind('\n').map_or(0, |i| i + 1)..]
            .trim_start()
            .starts_with("//!");
        if cited.is_empty()
            && in_module_doc
            && speaks_of_itself(scope)
            && !is_quoted(text, scope, at)
            && let Some(own) = integration_test_name(path)
        {
            cited.push((at, own.to_string()));
        }
        for (_, target) in cited {
            let channels = channels_of(&target);
            if channels.is_empty() {
                continue;
            }
            let labels: Vec<&str> = channels.iter().map(|c| c.label()).collect();
            if running.iter().all(|(name, _)| *name != target) {
                running.push((target, labels));
            }
        }

        // **클래스 지목도 지목이다.** `tests/*.rs` 는 이름이 아니라 부류를 부르는데,
        // 이름만 세던 추출기에서는 지목 0 건이 되어 그 서술을 **아무 축도 판정하지
        // 않았다.** 실측으로 다섯 자리가 그 사각에서 낡은 채로 살아 있었고, 그중 셋은
        // `src/` 밖이었다 — 경로 면제를 아무리 좁혀도 안 잡히는 자리다.
        //
        // 부류의 채널은 통합 타깃 전체의 합집합이다. "통합 테스트는 자동으로 안 돈다"
        // 는 그 합집합이 비어 있을 때만 참이다.
        if running.is_empty() && !class_channels.is_empty() && cites_the_test_class(scope) {
            let labels: Vec<&str> = class_channels.iter().map(|c| c.label()).collect();
            running.push(("*".to_string(), labels));
        }
        if running.is_empty() {
            continue;
        }
        let both = running.iter().any(|(_, labels)| labels.len() >= 2);
        if !both && COMBO_QUALIFIED_MARKERS.iter().any(|m| scope.contains(m)) {
            continue;
        }
        let listed: Vec<String> = running
            .iter()
            .map(|(name, labels)| format!("tests/{name}.rs({})", labels.join(" · ")))
            .collect();
        found.push((
            at,
            if both {
                format!("{} — 두 조합 모두에서 돈다", listed.join(", "))
            } else {
                format!("{} — 조합을 한정해서 적어라", listed.join(", "))
            },
        ));
    }
    found.sort();
    found.dedup();
    found
}

/// 그 서술이 **자기 자신**을 가리키는가 — 모듈 doc 의 암묵 주어를 세울 조건.
///
/// 모듈 doc 이 채널을 말할 때 주어가 늘 이 타깃인 것은 아니다. 같은 doc 이 **다른 것**의
/// 채널(축 하나 · 전제 하나 · 남의 잡)을 말하기도 한다. 자기지칭이 있는 서술만 자기
/// 것으로 읽는다 — 없는 것까지 자기 것으로 읽으면 "이 축에는 자동 채널이 없다" 같은
/// 참인 문장을 이 타깃의 부재 주장으로 오해한다(실측으로 넷이 그렇게 걸렸다).
fn speaks_of_itself(scope: &str) -> bool {
    ["이 테스트", "이 가드", "이 파일", "this test", "This test"]
        .iter()
        .any(|m| scope.contains(m))
}

/// 그 자리가 **인용 안**인가 — `at` 은 `text` 기준 오프셋이고 `scope` 는 그 부분 슬라이스다.
///
/// 인용된 문장은 주장이 아니라 예시다 — 이 가드 자신의 doc 이 판정 대상 문장을 그대로
/// 옮겨 적는다. 같은 형태를 CI 표지 축에서도 고쳤다(설명문 안의 명령 문자열).
fn is_quoted(text: &str, scope: &str, at: usize) -> bool {
    let base = scope.as_ptr() as usize - text.as_ptr() as usize;
    if at < base {
        return false;
    }
    let rel = (at - base).min(scope.len());
    scope
        .char_indices()
        .take_while(|(i, _)| *i < rel)
        .filter(|(_, c)| *c == '"' || *c == '\u{201c}' || *c == '\u{201d}')
        .count()
        % 2
        == 1
}

/// 경로가 통합 테스트면 그 테스트 이름 — `tests/X.rs` 와 `crates/<c>/tests/X.rs` 둘 다.
fn integration_test_name(rel: &str) -> Option<&str> {
    let after = rel.rsplit_once("tests/")?.1;
    if after.contains('/') {
        return None;
    }
    after.strip_suffix(".rs")
}

/// 그 서술이 통합 테스트를 **부류로** 지목하는가 — `tests/*.rs` · `tests/*_guard.rs` 형태.
///
/// 이름 지목([`cited_tests`])과 같은 자격이다. 다른 것은 주어가 하나가 아니라 전부라는
/// 점뿐이고, 그래서 채널도 통합 타깃 전체의 합집합으로 잰다.
fn cites_the_test_class(scope: &str) -> bool {
    let mut from = 0;
    while let Some(rel) = scope[from..].find("tests/") {
        let at = from + rel;
        from = at + "tests/".len();
        let rest = &scope[from..];
        let end = rest
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '*'))
            .map_or(rest.len(), |(i, _)| i);
        if rest[..end].contains('*') && rest[end..].starts_with(".rs") {
            return true;
        }
    }
    false
}

/// 텍스트가 지목하는 통합 테스트 인용 지점 — (오프셋, 이름).
fn cited_tests(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find("tests/") {
        let at = from + rel;
        from = at + "tests/".len();
        let rest = &text[from..];
        let end = rest
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_'))
            .map_or(rest.len(), |(i, _)| i);
        if rest[end..].starts_with(".rs") {
            found.push((at, rest[..end].to_string()));
        }
    }
    found
}

/// `src/` 안의 lib 유닛 테스트 이름(`#[test]` 가 붙은 함수).
///
/// 이 목록이 역방향 판정의 축이다 — **문자열이 아니라 대상의 위치**로 가른다. 똑같은
/// 집행 서술이라도 그 테스트가 여기 있으면 참이고 `tests/` 아래 있으면 거짓이다.
fn lib_test_names(root: &Path) -> std::collections::BTreeSet<String> {
    let mut files = Vec::new();
    collect_files(root, &mut files);
    let mut names = std::collections::BTreeSet::new();
    for file in files {
        let rel = file.strip_prefix(root).unwrap_or(&file);
        let rel = rel.to_string_lossy().replace('\\', "/");
        if !rel.ends_with(".rs") || !(rel.starts_with("src/") || rel.contains("/src/")) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&file) else {
            continue;
        };
        names.extend(test_fn_names(&text));
    }
    names
}

/// 부재 표지가 놓인 오프셋들.
fn absence_offsets(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for marker in ABSENCE_MARKERS {
        let mut from = 0;
        while let Some(rel) = text[from..].find(marker) {
            let at = from + rel;
            from = at + marker.len();
            found.push(at);
        }
    }
    found.sort_unstable();
    found
}

/// `text` 안에서 `name` 이 **낱말로** 등장하는 오프셋들.
fn word_offsets(text: &str, name: &str) -> Vec<usize> {
    let boundary = |c: Option<char>| c.is_none_or(|c| !(c.is_alphanumeric() || c == '_'));
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(name) {
        let at = from + rel;
        from = at + name.len();
        let before = text[..at].chars().next_back();
        let after = text[at + name.len()..].chars().next();
        if boundary(before) && boundary(after) {
            found.push(at);
        }
    }
    found
}

/// 실패 메시지에 공통으로 붙는 **판정 범위** 한 줄.
///
/// [`claim_scope`] 의 doc 이 이미 정확히 적고 있다. 문제는 그것을 읽는 사람이 이 파일을
/// 여는 사람뿐이라는 것이다 — 빨간 것을 보는 저자는 자기 문서를 연다. 범위가 줄 단위라고
/// 짐작하면 "이 줄에는 그런 말 없는데" 에서 멈춘다.
const SCOPE_NOTE: &str = "\n\n  [범위] 이 판정기가 보는 것은 줄이 아니라 **마크다운 항목 \
    하나**다 — 표는 그 행, 목록은 그 항목(들여쓴 이어짐 포함), 산문은 빈 줄 사이 한 문단, \
    Rust 는 이어진 주석 블록. 그래서 같은 항목 안에 있으면 다른 문장에 적힌 표지도 함께 \
    읽히고, 항목을 가르면 갈라진다.";

/// 그 옆의 두 번째 줄 — **주어를 단정하지 않는다**는 자백.
///
/// 이 사실도 소스에는 있었다(`absence_offsets` 의 주석: "어느 지목이 이 문장의 주어인지는
/// 이 판정기가 알 수 없다"). 정직하지만 저자에게 닿지 않는 자리에 있었다. 그래서 저자는
/// 빨간 것을 보고도 **문장을 고칠지 인용을 옮길지 정하지 못한다** — 판정기가 둘 중 어느
/// 쪽을 지목한 것인지 말해주지 않으니, 고른 쪽이 맞는지도 알 수 없다. 실측으로 그 형태가
/// 났다(2026-09-06, 다른 레인의 저자). 아는 자리와 필요한 자리가 다르면 아는 것만으로는
/// 아무 일도 안 일어난다.
///
/// 처방을 **둘 다** 적는 이유가 여기 있다. 하나만 적으면 판정기가 모르는 것을 아는 척하게
/// 되고, 그 척은 틀린 쪽을 고치게 만든다.
const SUBJECT_NOTE: &str = "\n  [주어] 이 판정기는 그 범위 안의 **어느 지목이 문제 문장의 \
    주어인지 단정하지 않는다** — 뒤에 덧붙인 선례·참조 지목도 같은 범위로 들어온다. 그러니 \
    처방이 둘이고, 어느 쪽인지는 저자만 안다: (가) 항목을 갈라 채널 주장과 다른 지목을 서로 \
    다른 항목에 두거나, (나) 채널 주장 자체를 사실에 맞게 고쳐라.\n  \
    ★ (가)는 **그 지목이 이 문장의 주어가 아닐 때만** 옳다. 주어인데 항목만 가르면 이 \
    판정기는 조용해지고 **거짓 문장은 그대로 남는다** — 그건 고친 것이 아니라 가드를 끈 \
    것이다. 그러니 가르기 전에 물어라: 이 문장이 채널의 부재를 말하는 대상이 정말 그 \
    지목이 아닌가. 맞다면 (나)뿐이다.";

/// 주장이 놓인 **한 서술의 범위** — 표는 그 행, 산문은 그 문단, Rust 는 이어진 주석
/// 블록. 면제도 표지 탐색도 이 범위에서 한다.
///
/// **면제는 좁게, 검출은 넓게** 가 원칙이지만, 둘의 단위가 어긋나면 반대로 샌다.
/// 실측한 두 형태가 그것이다.
/// - 면제만 넓으면: 문서 끝에 붙인 실행 주장이, 앞 절에 있던 "컴파일" 한 단어로
///   면제됐다.
/// - 검출만 넓으면: 표의 **옆 행**에 있는 표지를 끌어와 정확히 쓴 행을 위반으로 짚었다.
///
/// 그래서 단위를 하나로 맞춘다. 셋으로 갈리는 이유:
/// - **표 행**(`|` 로 시작)과 **목록 항목**(`-`/`*`/`1.` 로 시작) — 한 행·한 항목이 곧
///   한 서술인데 사이에 빈 줄이 없어, 문단으로 묶으면 표나 목록 전체가 한 덩어리가
///   된다. 실측으로 옆 항목의 표지가 끌려와 오탐이 났다. 항목의 접힌 줄(들여쓴 이어짐)
///   은 그 항목에 붙인다.
/// - **문단**(빈 줄 사이) — 마크다운 산문은 한 문장을 예사로 줄바꿈한다. 줄로 끊으면
///   "그 잡은 수동 / 전용이라" 처럼 정확히 쓴 서술이 반토막 난다.
/// - **주석 블록** — Rust doc 도 같은 이유. 빈 주석 줄(`//!` 뿐)이 문단 경계다.
fn claim_scope(text: &str, at: usize) -> &str {
    let line_start = |i: usize| text[..i].rfind('\n').map_or(0, |j| j + 1);
    let line_end = |i: usize| text[i..].find('\n').map_or(text.len(), |j| i + j);
    let kind = |start: usize| -> u8 {
        let line = text[start..line_end(start)].trim_start();
        if line.starts_with('|') {
            0 // 표 행 — 혼자 선다
        } else if line.starts_with("//!") || line.starts_with("///") {
            if line.trim_end().len() <= 3 { 2 } else { 1 } // 주석 / 빈 주석(경계)
        } else if line.trim().is_empty() {
            2 // 빈 줄 — 경계
        } else {
            3 // 산문
        }
    };

    let starts_item = |start: usize| {
        let line = text[start..line_end(start)].trim_start();
        line.starts_with("- ")
            || line.starts_with("* ")
            || line.split_once(". ").is_some_and(|(head, _)| {
                !head.is_empty() && head.bytes().all(|b| b.is_ascii_digit())
            })
    };

    let mut lo = line_start(at);
    let mut hi = line_end(at);
    let here = kind(lo);
    if here == 0 || here == 2 {
        return &text[lo..hi];
    }
    while lo > 0 && !starts_item(lo) {
        let prev = line_start(lo - 1);
        if kind(prev) != here {
            break;
        }
        lo = prev;
    }
    while hi < text.len() {
        let next = hi + 1;
        if next >= text.len() || kind(next) != here || starts_item(next) {
            break;
        }
        hi = line_end(next);
    }
    &text[lo..hi]
}

/// 그 오프셋이 **산문**에 있는가 — Rust 소스에서는 주석 줄만.
///
/// 주장은 사람이 읽는 문장이지 코드가 아니다. 이 구분이 없으면 표지 목록을 문자열
/// 리터럴로 들고 있는 파일(이 가드 자신이 그렇다)이 자기 목록에 걸린다.
fn is_prose_line(text: &str, at: usize, rel: &str) -> bool {
    if !rel.ends_with(".rs") {
        return true;
    }
    let start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let end = text[at..].find('\n').map_or(text.len(), |i| at + i);
    text[start..end].trim_start().starts_with("//")
}

/// 한 파일의 **집행 주장 위반**과 인용 수 — 파일 순회·경로 처리와 분리된 판정기.
///
/// 순수 함수인 이유: 면제를 겨냥한 변이를 **합성 문자열**로 찌를 수 있어야 하기
/// 때문이다. 판정이 순회 루프 안에 있으면 변이가 "레포에 진짜 위반을 심는" 방식으로만
/// 가능해지고, 그건 느리고 트리를 더럽히며 되돌리다 사고가 난다.
fn enforcement_violations(
    text: &str,
    path: &str,
    automatic: &std::collections::BTreeSet<String>,
) -> (Vec<usize>, usize) {
    let mut candidates = cited_tests(text);
    let cited = candidates.len();
    // 자기 자신을 "이 테스트가 …" 로만 부르는 형태 — 파일 안에 자기 경로가 없어서 위
    // 인용 추출로는 잡히지 않는다. 통합 테스트 파일이면 집행 표지가 놓인 자리마다 자기
    // 이름을 인용한 것으로 본다.
    if let Some(own) = integration_test_name(path) {
        for marker in ENFORCE_MARKERS {
            let mut from = 0;
            while let Some(off) = text[from..].find(marker) {
                let at = from + off;
                from = at + marker.len();
                candidates.push((at, own.to_string()));
            }
        }
    }

    let mut found = Vec::new();
    for (at, name) in candidates {
        if automatic.contains(&name) {
            continue;
        }
        if !is_prose_line(text, at, path) {
            continue;
        }
        // 표지도 같은 범위에서 찾는다 — 옆 행·옆 항목의 표지를 끌어오면 정확히 쓴
        // 서술이 위반으로 걸린다.
        let scope = claim_scope(text, at);
        if !ENFORCE_MARKERS.iter().any(|m| scope.contains(m)) {
            continue;
        }
        if absence_exempts(scope) {
            continue;
        }
        // 실행이 아니라 컴파일을 주장하는 문장은 참이다 — 자동 잡의 `--all-targets`
        // clippy 가 통합 테스트 타깃을 컴파일한다.
        if COMPILE_CLAIM_MARKERS.iter().any(|m| scope.contains(m)) {
            continue;
        }
        found.push(at);
    }
    found.sort_unstable();
    found.dedup();
    (found, cited)
}

/// 한 파일에서 **lib 테스트를 두고 부재를 적은 자리** — 역방향 판정기.
fn weak_absence_offsets(
    text: &str,
    path: &str,
    lib_tests: &std::collections::BTreeSet<String>,
) -> Vec<usize> {
    // 면제의 사유는 **이 파일이 그 이름을 정의한다**는 성질이지 경로 이름이 아니다.
    // 앞 형태는 `src/` 를 통째로 건너뛰었는데, 그러면 남의 테스트 이름을 부르는 `src/`
    // 모듈 doc 까지 함께 빠진다 — 거기에도 채널 주장이 산다. 이 파일이 정의하는 이름만
    // 빼고 나머지는 판정한다.
    let defined_here: std::collections::BTreeSet<String> =
        test_fn_names(text).into_iter().collect();
    let mut found = Vec::new();
    // 부재를 적은 자리에서 출발한다 — lib 테스트 이름 전체를 파일마다 훑으면 이름 수 x
    // 파일 수가 되어 스캔이 느려지고, 얻는 것은 같다.
    for at in absence_offsets(text) {
        if !is_prose_line(text, at, path) {
            continue;
        }
        let scope = claim_scope(text, at);
        if AUTOMATIC_CHANNEL_MARKERS.iter().any(|m| scope.contains(m)) {
            continue;
        }
        if lib_tests
            .iter()
            .any(|n| !defined_here.contains(n) && !word_offsets(scope, n).is_empty())
        {
            found.push(at);
        }
    }
    found
}

/// 문서가 "CI 가 전체 스위트를 돌린다" 고 말하면, 실제로 그런지 워크플로와 대조한다.
#[test]
fn no_file_claims_ci_runs_the_full_suite_while_it_does_not() {
    let root = repo_root();
    if ci_actually_runs_the_full_suite(&root) {
        // 전체 스위트가 자동 채널에 올라갔다 — 주장이 참이 됐으므로 검사할 것이 없다.
        return;
    }

    let mut files = Vec::new();
    collect_files(&root, &mut files);
    assert!(
        !files.is_empty(),
        "스캔 대상 파일이 하나도 없다 — 수집이 깨졌다"
    );

    let mut violations = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for at in claim_offsets(&text, &rel_str) {
            violations.push(format!("{rel_str}:{}", line_of(&text, at)));
        }
    }

    assert!(
        violations.is_empty(),
        "전체 스위트(`cargo test --workspace`)는 자동으로 돌지 않는다 — `test.yml` 의 \
         `test-linux-x64` 는 `workflow_dispatch` 전용이다. 아래는 그것을 CI 강제 장치로 \
         서술한 자리다. 실제 채널은 `docs/dev-guide/ci-gates.md` 를 보고, 서술을 \
         '자동 채널 없음' 으로 고쳐라:\n  {}{SCOPE_NOTE}{SUBJECT_NOTE}",
        violations.join("\n  ")
    );
}

/// 문서가 어떤 통합 테스트를 자동 집행 장치로 부르면, 자동 잡이 실제로 그 이름을
/// 돌리는지 워크플로에서 읽어 대조한다.
#[test]
fn no_file_claims_ci_enforces_an_integration_test_it_does_not_run() {
    let root = repo_root();
    let Some(automatic) = integration_tests_run_automatically(&root) else {
        // 자동 잡이 좁혀지지 않은 `cargo test` 를 돌린다 — 통합 테스트가 전부 자동으로
        // 도는 것이므로 이 축은 성립하지 않는다.
        return;
    };

    let mut files = Vec::new();
    collect_files(&root, &mut files);
    assert!(
        files.len() >= MIN_SCANNED_FILES,
        "스캔한 파일이 {}개뿐이다(하한 {MIN_SCANNED_FILES}) — 수집이 줄었다",
        files.len()
    );

    let mut citations = 0usize;
    let mut violations = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };

        let (found, cited) = enforcement_violations(&text, &rel_str, &automatic);
        citations += cited;
        for at in found {
            violations.push(format!("{rel_str}:{}", line_of(&text, at)));
        }
    }

    assert!(
        citations >= MIN_TEST_CITATIONS,
        "통합 테스트 인용을 {citations}개밖에 못 찾았다(하한 {MIN_TEST_CITATIONS}) — 추출이 깨졌다"
    );

    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "자동 잡이 이름을 지목해 돌리는 통합 테스트는 {automatic:?} 뿐이다(나머지 자동 \
         테스트는 `--lib --bins` = 유닛 뿐). 아래는 그 밖의 통합 테스트를 CI 강제 장치로 \
         서술한 자리다. 문장을 지우지 말고, 그 문장이 전하려던 사실은 남긴 채 채널 주장만 \
         `docs/dev-guide/ci-gates.md` 에 맞춰라:\n  {}{SCOPE_NOTE}{SUBJECT_NOTE}",
        violations.join("\n  ")
    );
}

/// 반대 방향 — **lib 유닛 테스트**를 두고 "자동 채널이 없다" 고 적은 자리.
///
/// `--lib --bins` 잡은 main push·PR 에서 자동으로 돈다. 그러므로 그 안에 있는 테스트를
/// 두고 부재를 적으면 사실보다 **약하다**. 강한 부정("없다")은 강한 긍정만큼 틀릴 수
/// 있고, 틀린 방향만 다를 뿐 다음 사람의 판단을 망치는 것은 같다.
#[test]
fn no_file_denies_the_automatic_channel_a_lib_test_actually_has() {
    let root = repo_root();
    if !lib_tests_run_automatically(&root) {
        // 자동 잡이 lib 유닛 테스트를 하나도 안 돌린다 — 그 채널이 없으므로 "없다" 고
        // 적은 서술이 참이 됐고, 이 축은 다른 두 축과 같은 방식으로 스스로 잠잠해진다.
        return;
    }
    let lib_tests = lib_test_names(&root);
    assert!(
        lib_tests.len() >= MIN_LIB_TESTS,
        "lib 테스트 이름을 {}개밖에 못 찾았다(하한 {MIN_LIB_TESTS}) — 추출이 깨졌다",
        lib_tests.len()
    );

    let mut files = Vec::new();
    collect_files(&root, &mut files);
    let mut violations = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        // 부재를 적은 자리에서 출발한다 — lib 테스트 이름 전체를 파일마다 훑으면
        // 이름 수 x 파일 수가 되어 스캔이 느려지고, 얻는 것은 같다.
        for at in weak_absence_offsets(&text, &rel_str, &lib_tests) {
            violations.push(format!("{rel_str}:{}", line_of(&text, at)));
        }
    }

    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "아래는 **lib 유닛 테스트**를 두고 자동 채널의 부재를 적은 자리다. 그 테스트는 \
         `crossplatform-check.yml` 의 `cargo test --workspace --lib --bins`(main push · PR)로 \
         자동으로 돈다 — 서술이 사실보다 약하다. 채널 정본은 `docs/dev-guide/ci-gates.md`:\n  {}{SCOPE_NOTE}{SUBJECT_NOTE}",
        violations.join("\n  ")
    );
}

/// 통합 테스트를 지목하면서 그 테스트가 **실제로 가진 채널을 부정**하면 잡는다.
///
/// 한동안 "통합 테스트에는 자동 실행 채널이 없다" 가 구성상 참이었고, 그래서 이 방향은
/// 판정할 것이 없었다. 헤드리스 잡이 `--lib --bins` 에서 전체 스위트로 넓어지면서 그
/// 전제가 깨졌다 — 이제 같은 문장이 조합마다 갈린다. 조합 수로 셋으로 나눈다:
/// 0 이면 부재 서술이 참이고, 1 이면 **어느 조합인지 함께 적어야** 참이며, 2 면 어떻게
/// 적어도 부재 서술은 거짓이다.
#[test]
fn no_file_denies_a_channel_an_integration_test_actually_has() {
    let root = repo_root();
    let invocations = automatic_test_invocations(&root);
    let unmodelled: Vec<String> = invocations
        .iter()
        .flat_map(|(_, tail)| {
            UNMODELLED_TEST_FLAGS
                .iter()
                .filter(|f| tail.split_whitespace().any(|w| w == **f))
                .map(|f| format!("{f} in `cargo test{tail}`"))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        unmodelled.is_empty(),
        "자동 잡이 feature 를 명시적으로 켠다 — 조합이 둘이라는 이 가드의 모델이 더 이상 \
         워크플로를 설명하지 못한다. 모델을 넓히기 전에는 채널 수가 틀리고, 틀린 채널 \
         수로 남의 서술을 고발하게 된다:\n  {}",
        unmodelled.join("\n  ")
    );

    let features = test_target_features(&root);
    let channels_of =
        |target: &str| integration_target_channels(&root, target, &invocations, &features);

    let mut files = Vec::new();
    collect_files(&root, &mut files);
    assert!(
        files.len() >= MIN_SCANNED_FILES,
        "스캔한 파일이 {}개다(하한 {MIN_SCANNED_FILES}) — 수집이 조용히 줄었다",
        files.len()
    );

    // 부류 지목(`tests/*.rs`)의 채널 = **루트 패키지** 통합 타깃의 합집합.
    //
    // 모수를 워크스페이스 전체로 잡으면 안 된다. 크레이트 자기 `tests/` 를 자기 잡에서
    // 도는 패키지가 있어서 합집합이 두 조합으로 부풀고, 그러면 루트 관례를 말하는
    // 정확한 서술까지 "조합을 한정해서 적어라" 로 고발한다 — 실측으로 그 형태가 났다.
    // `tests/X.rs`(루트) 와 `crates/<c>/tests/X.rs` 는 cargo 가 다른 패키지의 타깃으로
    // 가르므로, 이 구분은 이름이 아니라 **소유 패키지**라는 성질이다.
    let mut class_channels = std::collections::BTreeSet::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if !rel_str.ends_with(".rs") || !rel_str.starts_with("tests/") {
            continue;
        }
        if let Some(name) = integration_test_name(&rel_str) {
            class_channels.extend(channels_of(name));
        }
    }

    let mut violations = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        for (at, why) in overstated_absence(&text, &rel_str, &channels_of, &class_channels) {
            violations.push(format!("{rel_str}:{} — {why}", line_of(&text, at)));
        }
    }
    violations.sort();
    violations.dedup();
    assert!(
        violations.is_empty(),
        "아래는 통합 테스트를 두고 자동 채널의 부재를 적었지만, 그 테스트가 실제로는 \
         자동으로 도는 자리다. 조합 정본은 `docs/dev-guide/ci-gates.md`:\n  {}{SCOPE_NOTE}{SUBJECT_NOTE}",
        violations.join("\n  ")
    );
}

// ─── gui 칸의 채널 주장 — 세 층, 세 테스트 ─────────────────────────────────────
//
// 셋을 **한 테스트 안의 세 단정**으로 두지 않는다. 그러면 앞 단정이 죽는 순간 뒤 단정은
// 아예 안 돌아서, 한 번에 하나씩만 판정된다 — 뭉친 주장의 다른 얼굴이다. 함수를 갈라야
// 세 층이 서로를 가리지 않는다.
//
// 뭉치면 왜 나쁜가: 한 주장으로 두면 셋 중 하나만 참이어도 통과하고, 그 통과가 칸의
// 크기를 부풀린다. 실측으로 그 형태가 났다 — 셋을 뭉쳐 세면 "디스플레이가 사는 것" 이
// 1 이 아니라 11 로 보인다. 그리고 **모수를 줄이는 방향의 어긋남은 언제나 초록**이라
// (`#[ignore]` 33 건을 "돈다" 로 세는 쪽), 뭉친 주장은 틀린 채로 조용히 산다.
//
// 세 층은 답의 **종류**가 서로 다르다 — 그래서 셋을 같은 단위로 셀 수 없다:
//   층 1 은 이름 하나(값이 1), 층 2 는 전수 성질(값이 0), 층 3 은 **값이 없다**.

/// 층 1 — 디스플레이가 살리는 것은 `#[ignore]` 가 **아닌** 그 하나다.
///
/// `multi_window_owner_routing` 은 무시 표시가 없는데도 창이 없어 못 돌던 테스트다.
/// 지금 그것을 살리는 자동 채널은 이름을 지목한 스텝 하나뿐이라, 그 스텝이 사라지면
/// 이 층의 값은 1 이 아니라 0 이 된다.
///
/// ## 이 층만 **워크플로 파서에 기댄다** — 그 파서의 잡 분할은 이제 고정돼 있다
///
/// 뒤의 두 층은 소스와 문서만 읽는데, 이 층은 `automatic_test_invocations` 를 거쳐
/// `automatic_job_bodies` 의 **2 칸 들여쓰기 = 잡 헤더** 규칙에 기댄다. 그 규칙을
/// 3 칸으로 깨뜨리는 변이를 실제로 쏴 봤다:
///
/// ```text
/// 원본(2칸)   bodies 16   invocations 6
/// 변이(3칸)   bodies  8   invocations 5   ← 잡 절반과 호출 하나가 사라진다
/// ```
///
/// 그런데도 이 파일의 테스트는 **하나도 안 죽었다.** 이 층이 초록이었던 것은 사라진
/// 호출이 마침 이 층이 지목하는 것이 아니었기 때문이지, 파서가 맞아서가 아니다.
/// 그러니 지금 이 층이 말할 수 있는 것은 "가드가 본다" 가 아니라 **"가드가 본다고
/// 되어 있다"** 다 — 초록은 "덮였다" 와 "안 덮여서 볼 수 없다" 둘 다와 양립한다.
///
/// **그 단정이 지금 섰다 — 다만 이 파일 밖이다.** 같은 변이를 패키지 전체에 다시 쏘면
/// 넷이 죽는다(`workflow_triggers` 의 잡 분할·접힌 스칼라·잡 수 하한 셋과
/// `no_filtered_scan_guard_reads_only_ignored_paths`). 그러니 이 층의 문장은 다시
/// "가드가 본다" 로 쓸 수 있다.
///
/// ★ **다만 모수를 옮겨 적지 마라.** 이 파일만 돌리면 그 변이에서 여전히 **전부 초록**이다
/// (실측 2026-09-06: 이 파일 52 초록 / 패키지 4 실패). 즉 파서가 고정된 것은 **패키지
/// 모수에서**이고, 이 파일 하나를 근거로는 여전히 아무것도 말할 수 없다. 위의 두 표는
/// 그래서 지운 게 아니라 남겨 둔다 — 무엇이 왜 초록인지가 층 1 의 실제 성질이다.
#[test]
fn the_gui_layer_a_display_revives_is_exactly_the_one_named_test() {
    const THE_ONE: &str = "multi_window_owner_routing";
    let root = repo_root();

    let e2e = integration_target_path(&root, "e2e_tests").expect("tests/e2e_tests.rs 가 없다");
    let e2e_text = std::fs::read_to_string(&e2e).expect("e2e_tests.rs 를 읽지 못했다");
    let e2e_fns = test_fns_with_ignore(&e2e_text);
    assert!(
        e2e_fns.len() > 10,
        "e2e_tests 에서 테스트를 {}건밖에 못 뽑았다 — 추출이 죽으면 아래 판정이 \
         언제나 참이 된다(R435)",
        e2e_fns.len()
    );
    let one = e2e_fns
        .iter()
        .find(|(n, _)| n == THE_ONE)
        .expect("층 1 의 그 하나가 사라졌다 — 이름이 바뀌었으면 이 층의 값을 다시 세라");
    assert!(
        !one.1,
        "`{THE_ONE}` 에 `#[ignore]` 가 붙었다. 그러면 이 층이 사는 것은 1 이 아니라 0 이고, \
         그 잡의 스텝은 아무것도 안 돌린다"
    );

    let invocations = automatic_test_invocations(&root);
    assert!(
        !invocations.is_empty(),
        "자동 잡의 `cargo test` 호출을 하나도 못 뽑았다 — 추출이 죽었다(R435)"
    );
    let selected = invocations.iter().any(|(_, tail)| {
        let (filters, exact) = positive_filters(tail);
        filters.iter().any(|f| {
            if exact {
                f == THE_ONE
            } else {
                THE_ONE.contains(f.as_str())
            }
        })
    });
    assert!(
        selected,
        "층 1 의 그 하나를 이름으로 지목해 돌리는 자동 스텝이 없다. 그것이 이 층의 \
         **유일한** 채널이라(다른 잡은 `--skip` 하거나 창이 없다) 스텝이 사라지면 값은 0 이다"
    );
}

/// 층 2 — gui 스위트가 요구하는 것은 디스플레이가 아니라 **플래그**다.
///
/// `gui_tests` 는 전수 `#[ignore]` 라, 창이 있어도 평범한 `cargo test` 는 한 건도 안
/// 돌린다(R417: `#[ignore]` 는 실행만 막고 컴파일은 막지 않는다). 여기서 무시 표시가
/// 하나라도 빠지면 그 테스트는 **어느 자동 잡도 안 보는데** 아무도 그 사실을 모른다.
#[test]
fn the_gui_suite_needs_a_flag_not_a_display() {
    let root = repo_root();
    let gui = integration_target_path(&root, "gui_tests").expect("tests/gui_tests.rs 가 없다");
    let gui_text = std::fs::read_to_string(&gui).expect("gui_tests.rs 를 읽지 못했다");
    let gui_fns = test_fns_with_ignore(&gui_text);
    assert!(
        gui_fns.len() > 10,
        "gui_tests 에서 테스트를 {}건밖에 못 뽑았다 — 추출이 죽었다(R435)",
        gui_fns.len()
    );
    let running: Vec<&String> = gui_fns
        .iter()
        .filter(|(_, ig)| !ig)
        .map(|(n, _)| n)
        .collect();
    assert!(
        running.is_empty(),
        "`gui_tests` 에 `#[ignore]` 없는 테스트가 생겼다: {running:?}\n\
         그러면 이 층의 서술('디스플레이가 있어도 한 건도 안 돈다')이 거짓이 되고, \
         그 테스트는 **어느 자동 잡도 안 보는데** 아무도 그 사실을 모른다"
    );
}

/// 층 3 — `--ignored` 를 줘도 나오는 수에는 **단일 값이 없다.** 값 대신 그 단정을 지킨다.
///
/// 이 칸에는 수를 박지 않는다 — 박으면 그 수가 곧 낡고, 낡은 수는 없는 수보다
/// 나쁘다(ADR-0139). 실제로 계기마다 답이 다르고 **서로 반대 방향으로** 흔들린다:
/// 한 프로세스로 돌리면 한 panic 이 공유 인스턴스를 오염시켜 뒤를 다 죽이고, 프로세스를
/// 가르면 그 오염은 사라지지만 순서·상태에 기대던 것들이 대신 죽는다.
///
/// 그래서 지키는 것은 수가 아니라 **"단일 값이 없다" 는 단정 자체**다. 통과 수를 적은
/// 절은 그 절이나 그 하위 절에 단정을 함께 담아야 한다 — 누가 수만 채워 넣으면 빨개진다.
///
/// **은퇴 조건**을 함께 박는다. 수가 흔들리는 원인(lock 뒤의 단일 공유 인스턴스)이
/// 사라지면 수가 안정될 수 있고, 그때까지 이 규칙이 남으면 없는 병을 지키게 된다.
#[test]
fn the_gui_ignored_layer_has_no_single_value() {
    const MARKER: &str = "단일 값이 없다";
    const CLAIM_DOC: &str = "docs/dev-guide/ci-gates.md";
    let root = repo_root();

    let common = root.join("tests/gui_common/mod.rs");
    let common_text = std::fs::read_to_string(&common).expect("tests/gui_common/mod.rs 가 없다");
    assert!(
        common_text.contains("Mutex") && common_text.contains(".lock()"),
        "gui 하네스의 공유 인스턴스(lock 뒤의 단일 인스턴스)가 사라졌다. 그것이 수를 \
         흔들던 원인이므로, 이 층의 '{MARKER}' 가 아직 참인지 다시 재라 — 참이 아니게 \
         됐으면 이 층 규칙과 문서의 표기를 함께 걷어라"
    );

    let mut files = Vec::new();
    collect_files(&root, &mut files);
    let mut scanned = 0usize;
    let mut carries_marker = false;
    let mut violations = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&root).unwrap_or(file);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        if !rel_str.ends_with(".md") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(file) else {
            continue;
        };
        if !unwrapped(&text).contains("gui_tests") {
            continue;
        }
        scanned += 1;
        if rel_str == CLAIM_DOC {
            carries_marker = unwrapped(&text).contains(MARKER);
        }
        for line in gui_pass_counts_missing_marker(&text, MARKER) {
            violations.push(format!("{rel_str}:{line}"));
        }
    }
    assert!(
        scanned > 0,
        "`gui_tests` 를 언급하는 문서를 하나도 못 찾았다 — 수집이 죽었다(R435)"
    );
    assert!(
        carries_marker,
        "{CLAIM_DOC} 에서 '{MARKER}' 가 사라졌다. 그 단정이 이 층의 **값 자리**라, \
         없어지면 이 판정은 지킬 것이 없는 채로 언제나 초록이 된다(R435)"
    );
    assert!(
        violations.is_empty(),
        "아래 절이 gui 스위트의 통과 수를 적으면서 '{MARKER}' 는 안 적었다. 그 수는 \
         계기(한 프로세스인가 갈랐는가)마다 다르고 서로 반대 방향으로 흔들리므로, \
         수만 남으면 읽는 쪽이 그것을 커버리지로 읽는다. 그 절이나 그 하위 절에 \
         단정을 함께 담아라:\n  {}",
        violations.join("\n  ")
    );
}

/// 회귀 케이스 — **한 표 안에서 채널이 갈리는 행들.**
///
/// `docs/design/systems/theme.md` 의 토큰 규칙 표는 네 자리에서 가드를 인용하는데, 셋은
/// 통합 테스트(`tests/design_token_adherence.rs`)이고 하나는 lib 유닛 테스트다. 문자열만
/// 보고 일괄 처리하면 넷이 같아 보여서 **맞게 적힌 행까지 함께 지워진다.** 이 테스트는
/// 그 표가 대상별로 갈린 상태를 유지하는지 고정한다.
///
/// 갈리는 **지점**은 바뀌었다. 통합 테스트 행이 요구하던 것은 한때 부재 표지였는데,
/// 헤드리스 잡이 전체 스위트로 넓어지면서 그 서술이 거짓이 됐다 — 즉 이 테스트가 거짓인
/// 문장을 요구하고 있었다. 이제 요구하는 것은 **조합 한정 표지**다. 두 행의 차이는
/// 여전히 남는다: lib 유닛 테스트는 두 조합 모두에서 돌고, 통합 테스트는 헤드리스
/// 조합에서만 돈다.
/// ★ **축이 스스로 잠잠해졌다면, 그 근거가 실재하는지 묻는다** — 초록의 이유를 묻는 것이다.
///
/// 이 파일의 판정 다섯 중 둘은 [`integration_tests_run_automatically`] 가 `None` 이면
/// 조기 반환한다. `None` 의 뜻은 "좁혀지지 않은 자동 호출이 있다 = 통합 테스트가 전부
/// 자동으로 돈다" 이고, 그때는 검사할 것이 없는 게 맞다.
///
/// **그런데 같은 `None` 이 판독이 깨져도 난다.** 좁힘은 `--test <이름>` 같은 플래그로
/// 판정하는데, 이 레포의 `test.yml` 은 그것을 **접힌 스칼라(`>`)로 여러 줄에 걸쳐** 쓴다.
/// 평탄화가 깨지면 그 플래그들이 사라져 좁혀지지 않은 호출로 보이고, 그러면 판정 둘이
/// **틀린 이유로** 잠잠해진다 — 그리고 그 둘은 조용히 초록이다. 초록은 "덮였고 위반이
/// 없다" 와 "볼 것이 없어 안 봤다" 둘 다와 양립하므로, 잠잠해진 이유를 여기서 확인한다.
#[test]
fn the_self_silencing_axis_names_what_silenced_it() {
    let root = repo_root();
    if integration_tests_run_automatically(&root).is_some() {
        // 축이 살아 있다 — 두 판정이 실제로 돌므로 여기서 볼 것이 없다.
        return;
    }

    let invocations = automatic_test_invocations(&root);
    assert!(
        !invocations.is_empty(),
        "자동 잡의 `cargo test` 호출을 하나도 못 읽었다 — 판독이 깨졌다. 이 상태에서 나온 \
         판정은 무엇도 안 본 것이다"
    );
    let unnarrowed: Vec<&String> = invocations
        .iter()
        .filter(|(_, tail)| !tail_is_narrowed(tail))
        .map(|(_, tail)| tail)
        .collect();
    assert!(
        !unnarrowed.is_empty(),
        "판정 둘이 스스로 잠잠해졌는데(`integration_tests_run_automatically` 가 `None`) \
         그 근거가 되는 좁혀지지 않은 자동 호출이 하나도 없다 — 판독이 깨졌다"
    );

    // ★ **근거를 좁힘 판정으로 다시 묻지 않는다.** 그러면 `None` 을 만든 그 술어로 그
    // `None` 을 정당화하는 동어반복이 된다 — 술어가 깨지면 둘이 함께 틀리고 초록이다.
    // 실측으로 그 형태를 한 번 썼다가 변이가 안 죽어서 잡았다(2026-09-05). 그래서
    // **다른 성질**로 묻는다: 잠잠하게 만든 그 호출이 정말 전체 스위트를 돌리는가.
    //
    // 확인만이 아니라 교정이기도 하다 — `-p` 도 `--workspace` 도 없는 `cargo test` 는
    // 루트 패키지만 돌린다. 그것을 "통합 테스트가 전부 자동으로 돈다" 의 근거로 쓰면
    // 애초에 틀린다.
    let not_whole: Vec<String> = unnarrowed
        .iter()
        .filter(|tail| !tail.split_whitespace().any(|w| w == "--workspace"))
        .map(|tail| format!("cargo test{tail}"))
        .collect();
    assert!(
        not_whole.is_empty(),
        "판정 둘을 잠잠하게 만든 호출이 **전체 스위트가 아니다**(`--workspace` 가 없다). \
         좁힘 판독이 깨져 좁혀진 호출을 좁혀지지 않은 것으로 읽었거나, 루트 패키지만 도는 \
         호출을 전체 스위트로 센 것이다. 어느 쪽이든 그 둘의 초록은 '위반이 없다' 가 \
         아니라 '안 봤다' 다:\n  {}",
        not_whole.join("\n  ")
    );
}

#[test]
fn the_theme_table_keeps_the_two_channels_apart() {
    let path = repo_root().join("docs/design/systems/theme.md");
    let text = std::fs::read_to_string(&path).expect("theme.md 를 읽지 못했다");

    let integration = word_offsets(&text, "design_token_adherence");
    assert!(
        !integration.is_empty(),
        "표가 통합 테스트 가드를 더 이상 인용하지 않는다 — 회귀 케이스가 사라졌다"
    );
    for at in integration {
        let window = claim_scope(&text, at);
        assert!(
            COMBO_QUALIFIED_MARKERS.iter().any(|m| window.contains(m)),
            "{}:{} — 통합 테스트인데 어느 조합에서 도는지가 함께 적혀 있지 않다.\n  \
             ★ 이 판정기가 보는 것은 **표지가 있는가**뿐이고 그 표지가 **맞는가**는 \
             안 본다. 그러니 아무 표지나 붙이면 빨강은 사라지지만 그 행은 이제 \
             **틀린 조합을 단언한다** — 없던 것보다 나쁘다. 그 타깃이 실제로 도는 \
             조합을 `docs/dev-guide/ci-gates.md` 에서 확인하고 그것을 적어라.{SCOPE_NOTE}",
            path.display(),
            line_of(&text, at)
        );
    }

    let lib = word_offsets(&text, "ui_font_size_tokens_are_integers_at_every_zoom");
    assert!(
        !lib.is_empty(),
        "표가 lib 유닛 테스트 행을 더 이상 인용하지 않는다 — 양방향성의 증거가 사라졌다"
    );
    for at in lib {
        let window = claim_scope(&text, at);
        assert!(
            AUTOMATIC_CHANNEL_MARKERS.iter().any(|m| window.contains(m)),
            "{}:{} — lib 유닛 테스트인데 자동 채널이 적혀 있지 않다(사실보다 약하다){SCOPE_NOTE}",
            path.display(),
            line_of(&text, at)
        );
    }
}

// ─── 면제를 겨냥한 변이 (합성 입력) ───────────────────────────────────────────
//
// 면제를 하나 추가할 때마다 **그 면제를 겨냥한 변이**를 같이 넣는다. "면제 범위 안쪽에
// 진짜 위반을 심었을 때 잡히는가" 를 묻는 것이고, 검증하지 않은 면제는 그 면제만큼
// 구멍이다. 실측으로 두 번 샜다 — 한 번은 앞 절의 컴파일 언급이 면제로 작동해 위반을
// 가렸고, 한 번은 표의 옆 행 표지를 끌어와 정확히 쓴 행을 위반으로 짚었다.
//
// 판정기가 순수 함수라 합성 문자열로 찌른다. 레포에 진짜 위반을 심는 방식은 느리고
// 트리를 더럽히며 되돌리다 사고가 난다.
//
// **픽스처의 표지는 조각으로 조립한다.** 통째로 적으면 이 파일 자신이 위반으로 잡힌다 —
// 그것을 경로 면제로 덮으면 면제가 하나 늘고, 그 파일이 나중에 들이는 진짜 위반까지
// 함께 새어 나간다. 조립하면 면제 없이 닫힌다.
fn enforce() -> String {
    format!("CI 가 {}", "강제한다")
}

/// 자동 잡이 이름으로 지목하는 통합 테스트가 하나도 없는 상태.
fn no_named_tests() -> std::collections::BTreeSet<String> {
    std::collections::BTreeSet::new()
}

fn named(names: &[&str]) -> std::collections::BTreeSet<String> {
    names.iter().map(|s| (*s).to_string()).collect()
}

/// 층 3 의 판정기를 겨냥한 변이 — 절 범위가 맞게 잘리는가.
///
/// 표지를 조각으로 조립하지 않고 그대로 적는다: 이 판정기는 `.md` 만 훑으므로 `.rs` 인
/// 이 파일 자신은 대상이 아니다.
const NO_SINGLE_VALUE: &str = "단일 값이 없다";

#[test]
fn a_pass_count_in_an_unrelated_section_is_not_a_gui_claim() {
    // 실측한 오탐의 축소판 — 통과 수와 `gui_tests` 가 서로 다른 절에 있다.
    let text = concat!(
        "## 어느 바이너리를 띄우는가\n\n스위트 단위 10 / 11 통과.\n\n",
        "## 시나리오 하나에 테스트 하나\n\n`gui_tests` 는 전수 무시다.\n"
    );
    assert!(
        gui_pass_counts_missing_marker(text, NO_SINGLE_VALUE).is_empty(),
        "무관한 두 절이 서로를 위반으로 만들었다"
    );
}

#[test]
fn a_pass_count_beside_gui_tests_without_the_marker_is_caught() {
    let text = "# 머리\n\n## 남은 칸\n\n`gui_tests` 는 11 통과다.\n";
    assert_eq!(
        gui_pass_counts_missing_marker(text, NO_SINGLE_VALUE),
        vec![3],
        "같은 절에서 수만 적은 자리를 못 잡았다"
    );
}

#[test]
fn a_marker_in_a_child_section_qualifies_the_parent() {
    // 수를 적은 절 **아래**에 그 수가 왜 단일 값이 아닌지를 푸는 하위 절을 두는 것은
    // 정상적인 문서 구조다. 이것을 위반으로 세면 규칙이 잘 쓴 글을 벌한다.
    let text = format!(
        "## 남은 칸\n\n`gui_tests` 는 11 통과다.\n\n### 왜 그 수가 흔들리나\n\n이 칸에는 {NO_SINGLE_VALUE}.\n"
    );
    assert!(
        gui_pass_counts_missing_marker(&text, NO_SINGLE_VALUE).is_empty(),
        "하위 절의 단정이 부모 절을 못 덮었다"
    );
}

#[test]
fn a_marker_in_a_sibling_section_does_not_exempt() {
    let text = format!(
        "## 남은 칸\n\n`gui_tests` 는 11 통과다.\n\n## 다른 칸\n\n이 칸에는 {NO_SINGLE_VALUE}.\n"
    );
    assert_eq!(
        gui_pass_counts_missing_marker(&text, NO_SINGLE_VALUE),
        vec![1],
        "옆 절의 단정이 면제로 작동했다"
    );
}

#[test]
fn a_heading_inside_a_fence_does_not_split_a_section() {
    let text = format!(
        "## 남은 칸\n\n```sh\n# gui_tests 를 이렇게 돈다\n```\n\n`gui_tests` 는 11 통과이고 이 칸에는 {NO_SINGLE_VALUE}.\n"
    );
    assert!(
        gui_pass_counts_missing_marker(&text, NO_SINGLE_VALUE).is_empty(),
        "코드 펜스 안의 `#` 주석을 heading 으로 읽어 절을 갈랐다"
    );
}

#[test]
fn an_absence_marker_in_a_neighbouring_table_row_does_not_exempt() {
    let text = format!(
        "| 포맷 | `tests/a_guard.rs` 가 강제한다 — 자동 채널이 없다 |\n| 린트 | `tests/b_guard.rs` 를 {} |\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert_eq!(found.len(), 1, "옆 행의 부재 표지가 면제로 작동했다");
    assert_eq!(line_of(&text, found[0]), 2);
}

#[test]
fn an_absence_marker_in_a_neighbouring_list_item_does_not_exempt() {
    let text = format!(
        "- `tests/a_guard.rs` 가 강제한다 — 자동 채널이 없다.\n- `tests/b_guard.rs` 를 {}.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert_eq!(found.len(), 1, "옆 항목의 부재 표지가 면제로 작동했다");
    assert_eq!(line_of(&text, found[0]), 2);
}

#[test]
fn a_compile_marker_in_another_paragraph_does_not_exempt() {
    // 실측한 누수 그대로: 앞 문단이 컴파일을 언급하고, 뒤 문단이 실행을 주장한다.
    let text = format!(
        "자동 잡의 clippy 는 통합 테스트를 컴파일한다.\n\n`tests/b_guard.rs` 를 {}.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert_eq!(found.len(), 1, "앞 문단의 컴파일 언급이 면제로 작동했다");
}

#[test]
fn a_compile_claim_in_the_same_sentence_is_true_and_exempt() {
    let text = format!(
        "`tests/b_guard.rs` 를 {} — 다만 그것은 컴파일 검사다.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert!(found.is_empty(), "참인 컴파일 주장을 위반으로 짚었다");
}

#[test]
fn a_wrapped_sentence_keeps_its_absence_marker_in_scope() {
    // 마크다운 산문은 한 문장을 예사로 줄바꿈한다 — 반토막 내면 정확히 쓴 서술이
    // 오탐이 된다.
    let text = format!(
        "`tests/b_guard.rs` 를 {} — 다만 그 잡은 수동\n전용이라 실행 채널이 없다.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert!(found.is_empty(), "접힌 문장이 반토막 나 오탐이 났다");
}

#[test]
fn a_doc_comment_block_is_one_scope_but_a_blank_comment_line_splits_it() {
    let joined = format!(
        "//! `tests/b_guard.rs` 를 {} — 그 잡은 수동\n//! 전용이라 실행 채널이 없다.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&joined, "src/some_module.rs", &no_named_tests());
    assert!(found.is_empty(), "이어진 주석 블록이 갈라졌다");

    let split = format!(
        "//! 그 잡은 수동 전용이라 실행 채널이 없다.\n//!\n//! `tests/b_guard.rs` 를 {}.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&split, "src/some_module.rs", &no_named_tests());
    assert_eq!(
        found.len(),
        1,
        "빈 주석 줄 너머의 부재 표지가 면제로 작동했다"
    );
}

#[test]
fn the_named_enumeration_exempts_only_the_names_the_workflow_lists() {
    let text = format!("`tests/api_baseline_0_7.rs` 를 {}.\n", enforce());
    let (found, _) = enforcement_violations(&text, "docs/x.md", &named(&["api_baseline_0_7"]));
    assert!(found.is_empty(), "열거된 이름을 위반으로 짚었다");

    // 워크플로에서 그 이름이 빠지면 같은 문장이 거짓이 된다 — 목록을 복사해 갖고 있지
    // 않다는 것이 이 대비로 드러난다.
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert_eq!(found.len(), 1, "열거가 사라졌는데 판정이 따라오지 않았다");
}

#[test]
fn a_code_literal_is_not_a_claim() {
    let text = format!(
        "    let needle = \"`tests/b_guard.rs` 를 {}\";\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&text, "src/some_module.rs", &no_named_tests());
    assert!(found.is_empty(), "코드 리터럴을 서술로 읽었다");
}

#[test]
fn a_scope_that_asserts_both_automatic_and_absent_is_not_exempt() {
    // 실측 뮤테이션의 형태다 — 한 표 행이 "자동으로 돌린다" 와 "workflow_dispatch 전용"
    // 을 동시에 말한다. 범위는 정확한데(표 행은 한 서술) 그 안이 모순이라, 범위를 좁히는
    // 것으로는 안 닫힌다.
    let contradiction = "| 테스트 | `cargo test --workspace --locked` | 이 조합은 CI 가 자동으로 돌린다 — `test.yml` 의 전체 스위트는 `workflow_dispatch` 전용이다 |\n";
    assert_eq!(
        claim_offsets(contradiction, "docs/x.md").len(),
        1,
        "한 서술 안의 모순을 준수로 읽었다"
    );

    // 대조군: 같은 행에서 긍정 서술어만 빼면 정확한 서술이 되고, 그것은 면제된다.
    let precise = "| 테스트 | `cargo test --workspace --locked` | 이 조합 그대로는 자동 채널 없음 — `test.yml` 의 전체 스위트는 `workflow_dispatch` 전용이다 |\n";
    assert!(
        claim_offsets(precise, "docs/x.md").is_empty(),
        "정확히 쓴 행을 위반으로 짚었다"
    );
}

#[test]
fn a_reference_to_an_automatic_job_is_not_an_assertion_that_it_runs() {
    // 면제를 무효로 만드는 것은 **서술어**지 참조가 아니다. 잡 이름이나 조합을 가리키는
    // 것만으로 모순이 되면, "자동 채널 없음. 그건 `crossplatform-check` 의 잡이 배선했다"
    // 처럼 정확히 쓴 서술이 통째로 위반이 된다.
    let referring = "| lint | `cargo test --workspace --locked` | 자동 채널 없음 — `crossplatform-check` 의 Windows 잡이 `--lib --bins` 를 배선했다. `test.yml` 참조 |\n";
    assert!(
        claim_offsets(referring, "docs/x.md").is_empty(),
        "참조를 자동 실행 주장으로 읽었다"
    );
}

#[test]
fn the_contradiction_rule_applies_to_the_enforcement_axis_too() {
    // 같은 헬퍼가 두 자리에서 면제를 준다. 한쪽만 고치면 절반이다.
    let contradiction = format!(
        "`tests/b_guard.rs` 를 {} — 자동으로 돌린다. 그래도 그 잡은 수동 전용이다.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&contradiction, "docs/x.md", &no_named_tests());
    assert_eq!(found.len(), 1, "집행 축에서 모순이 면제로 작동했다");

    let precise = format!(
        "`tests/b_guard.rs` 를 {} — 다만 그 잡은 수동 전용이다.\n",
        enforce()
    );
    let (found, _) = enforcement_violations(&precise, "docs/x.md", &no_named_tests());
    assert!(found.is_empty(), "정확히 쓴 서술을 위반으로 짚었다");
}

#[test]
fn the_automatic_channel_marker_exempts_only_inside_the_same_row() {
    let libs = named(&["some_lib_test"]);

    let same_row = "| lib | 있다 — `--lib --bins` 가 돈다 | `some_lib_test` 자동 채널이 없다 |\n";
    assert!(
        weak_absence_offsets(same_row, "docs/x.md", &libs).is_empty(),
        "자동 채널을 함께 적은 행을 약한 서술로 짚었다"
    );

    let other_row =
        "| lib | 있다 — `--lib --bins` 가 돈다 |\n| 통합 | `some_lib_test` 는 자동 채널이 없다 |\n";
    assert_eq!(
        weak_absence_offsets(other_row, "docs/x.md", &libs).len(),
        1,
        "옆 행의 자동 채널 표지가 면제로 작동했다"
    );
}

// ─── 의도된 false negative (한계를 붙박는다) ─────────────────────────────────
//
// 못 잡는 것이 **의도**인 입력도 고정한다. 나중에 판정기를 넓힐 때 그 결정이 테스트
// 실패로 드러나야 하고, 안 적어 두면 의도된 한계와 버그가 구분되지 않는다.

#[test]
fn narrowing_is_seen_however_many_flags_precede_it() {
    // 좁힘 판정이 앞 몇 단어만 본다면, 플래그가 늘어난 날 조용히 밀린다.
    assert!(is_narrowed(" --workspace --lib --bins --locked\n"));
    assert!(is_narrowed(
        " --locked --no-fail-fast --frozen --offline --lib --bins\n"
    ));
    assert!(is_narrowed(
        " --locked --no-default-features --test api_baseline_0_7\n"
    ));
    // 좁혀지지 않은 전체 스위트는 그대로 잡힌다.
    assert!(!is_narrowed(" --locked --no-fail-fast\n"));
    // 다음 줄의 좁힘을 끌어오지 않는다.
    assert!(!is_narrowed(
        " --locked\n      - name: other\n        run: cargo test --lib\n"
    ));
}

/// 좁힘이 **백슬래시 연속행**에 놓여도 본다. YAML 의 `run:` 은 긴 명령을 줄로 나누는 것이
/// 관례이고, 물리적 한 줄만 읽으면 그 형태에서 좁혀진 스텝을 "안 좁혀졌다" 로 판정한다 —
/// 그러면 그 위에 선 채널 주장 판정이 통째로 뒤집힌다.
///
/// 이 테스트가 판정 범위를 붙박는 변이다. 범위를 다시 물리적 한 줄로 되돌리면 첫 두
/// 단언이 죽고, 반대로 줄 수 제한 없이 통째로 읽게 넓히면 마지막 단언이 죽는다.
#[test]
fn narrowing_is_seen_on_a_continuation_line() {
    assert!(is_narrowed(
        " --workspace --locked \\\n      --lib --bins\n"
    ));
    // 연속이 여러 번 이어져도 끝까지 따라간다.
    assert!(is_narrowed(
        " --workspace \\\n      --locked \\\n      --no-fail-fast \\\n      --test api_baseline_0_7\n"
    ));
    // 연속 표시가 끝난 **다음** 줄의 플래그는 이 명령의 것이 아니다.
    assert!(!is_narrowed(
        " --workspace \\\n      --locked\n      --lib --bins\n"
    ));
}

/// 첫 번째 축의 판정도 **고정 범위를 갖지 않는다.**
///
/// 이 단언들이 [`a_job_body_runs_the_full_suite`] 의 범위를 붙박는 변이다. 그 판정을
/// 다시 `take(N)` 으로 되돌리면 앞 두 단언이 죽고, 연속행을 안 잇는 형태로 되돌리면
/// 세 번째가 죽는다. **`is_narrowed` 와 같은 규칙을 쓰는 것**이 이 함수의 계약이다 —
/// 한 판정을 두 자리에서 다른 규칙으로 하면 그중 하나가 조용히 낡는다.
#[test]
fn the_full_suite_judgement_uses_the_same_narrowing_rule() {
    // 좁힘이 다섯 번째 이후에 와도 좁혀진 것으로 본다(전체 스위트가 아니다).
    assert!(!a_job_body_runs_the_full_suite(
        "        run: cargo test --workspace --locked --no-fail-fast --frozen --offline --lib --bins\n"
    ));
    assert!(!a_job_body_runs_the_full_suite(
        "        run: cargo test --workspace --locked --no-fail-fast --frozen --test api_baseline_0_7\n"
    ));
    // 좁힘이 백슬래시 연속행에 놓여도 본다.
    assert!(!a_job_body_runs_the_full_suite(
        "        run: |\n          cargo test --workspace --locked \\\n            --lib --bins\n"
    ));
    // 좁혀지지 않은 전체 스위트는 그대로 잡힌다 — 이 방향이 죽으면 축이 통째로 잠잠해진다.
    assert!(a_job_body_runs_the_full_suite(
        "        run: cargo test --workspace --no-default-features --locked --no-fail-fast\n"
    ));
    // `--skip` 은 좁힘이 아니다. 연속행으로 이어져 있어도 전체 스위트 그대로다.
    assert!(a_job_body_runs_the_full_suite(
        "        run: |\n          cargo test --workspace --locked --no-fail-fast -- \\\n            --skip all_e2e_tests\n"
    ));
    // `cargo test --workspace` 가 없는 본문은 이 축의 대상이 아니다.
    assert!(!a_job_body_runs_the_full_suite(
        "        run: cargo clippy --workspace --all-targets\n"
    ));
}

/// 합성 워크플로 디렉토리 — 판정기가 경로를 주입받으므로 조합 수를 마음대로 만들 수 있다.
///
/// 레포 실물에 고정하면 **조합이 하나뿐인 조건을 만들 수 없어** 한쪽 방향만 재게 된다.
fn workflow_dir(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tasty-ci-guard-{}-{}-{:?}",
        name,
        std::process::id(),
        std::thread::current().id()
    ));
    // 이전 실행 잔여물 제거 — 없으면 NotFound 라 실패가 정상 경로다.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("합성 워크플로 디렉토리를 만들지 못했다");
    for (file, body) in files {
        std::fs::write(dir.join(file), body).expect("합성 워크플로를 쓰지 못했다");
    }
    dir
}

/// 합성 레포 — `tests/<이름>.rs` 와 `.github/workflows/` 를 갖춘 최소 트리.
///
/// 채널 계산은 워크플로(무엇을 돌리나)와 트리(그 타깃이 있나)를 함께 읽으므로, 둘 중
/// 하나만 합성하면 판정의 절반이 실물에 걸린 채로 남는다.
fn fake_repo(name: &str, targets: &[(&str, &str)], workflows: &[(&str, &str)]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "tasty-ci-repo-{}-{}-{:?}",
        name,
        std::process::id(),
        std::thread::current().id()
    ));
    // 이전 실행 잔여물 제거 — 없으면 NotFound 라 실패가 정상 경로다.
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("tests")).expect("합성 tests/ 를 만들지 못했다");
    std::fs::create_dir_all(dir.join(".github/workflows")).expect("합성 워크플로를 만들지 못했다");
    for (file, body) in targets {
        std::fs::write(dir.join("tests").join(file), body).expect("합성 타깃을 쓰지 못했다");
    }
    for (file, body) in workflows {
        std::fs::write(dir.join(".github/workflows").join(file), body)
            .expect("합성 워크플로를 쓰지 못했다");
    }
    dir
}

/// **모듈 doc 은 자기 이름을 안 부른다.**
///
/// 통합 타깃의 `//!` 는 "이 테스트는 …" 이라고 쓰지 "`tests/X.rs` 는 …" 이라고 쓰지
/// 않는다. 지목이 없다는 이유로 건너뛰면 이 축은 **가드 자신의 채널 서술을 통째로 못
/// 본다** — 실측으로 열셋이 그렇게 살아남았고, 전부 헤드리스 잡이 전체 스위트로 넓어지기
/// 전의 문장이었다.
///
/// 다만 같은 doc 이 **남의** 채널을 말하기도 한다(축 하나 · 전제 하나 · 다른 잡). 그래서
/// 자기지칭이 있고 인용 안이 아닌 서술에만 자기 타깃을 주어로 세운다. 네 방향을 함께
/// 본다 — 셋째·넷째가 없으면 이 축이 아무것도 못 잡도록 망가져도 첫째가 초록이다.
#[test]
fn a_module_doc_speaks_for_its_own_target() {
    let headless = |_: &str| combos(&[Combo::Headless]);
    let none = |_: &str| combos(&[]);

    // (가) 자기지칭 + 부재 주장 -> 자기 타깃의 채널로 판정한다.
    let denying = "//! 이 테스트가 그 집행 채널이다 — 그 잡은 수동 전용이다.\n";
    assert_eq!(
        overstated_absence(denying, "tests/alpha.rs", &headless, &combos(&[])).len(),
        1,
        "자기 채널을 부정하는 모듈 doc 을 못 잡았다"
    );

    // (나) 자기지칭이 없으면 남의 말일 수 있다 — 주어를 세우지 않는다.
    let other = "//! 그 빌드 잡은 수동 전용이라 이 실패를 자동으로 잡지 못한다.\n";
    assert!(
        overstated_absence(other, "tests/alpha.rs", &headless, &combos(&[])).is_empty(),
        "주어가 남인 서술을 이 타깃의 부재 주장으로 읽었다"
    );

    // (다) 인용된 문장은 주장이 아니다.
    let quoted = "//! 이 가드는 \"그 잡은 수동 전용이다\" 같은 서술을 잡는다.\n";
    assert!(
        overstated_absence(quoted, "tests/alpha.rs", &headless, &combos(&[])).is_empty(),
        "인용을 주장으로 읽었다"
    );

    // (라) 채널이 정말 0 이면 부재 주장은 참이다 — 자기지칭이 있어도 안 걸린다.
    assert!(
        overstated_absence(denying, "tests/alpha.rs", &none, &combos(&[])).is_empty(),
        "채널이 없는 타깃의 참인 부재 주장을 고발했다"
    );
}

/// **설명문 안의 명령 문자열은 주장이 아니다.**
///
/// 이 축은 "CI 가 전체 스위트를 돌린다" 고 **주장하는** 서술을 찾는다. 그런데 표지를
/// 글자 수 창으로 보던 형태는 문단을 넘어가 **다른 아이템의 코드 줄**에서 표지를 주웠고,
/// 그래서 파서가 명령 문자열을 어떻게 잘못 읽는지 **설명하는** doc 주석이 고발됐다
/// (`src/source_guards/mod.rs` 의 `flatten_workflow` doc — 그 위 상수 리터럴에 `"test.yml"`
/// 이 있었다). 주장하는 문장과 인용하는 문장을 가르는 것은 거리가 아니라 **서술의 경계**다.
///
/// 두 방향을 함께 본다. 뒤쪽이 없으면 표지 판정이 아무것도 못 찾도록 망가져도 앞쪽
/// 단언은 초록이다 — 이 축이 통째로 잠잠해진 것을 통과로 읽는다.
#[test]
fn an_example_command_inside_an_explanation_is_not_a_claim() {
    // (가) 설명문. 표지(`test.yml`)는 위쪽 **코드 줄**에 있고 서술 안에는 없다.
    let explaining = concat!(
        "const EXPECTED: &[(&str, usize)] = &[(\"crossplatform-check.yml\", 2), (\"test.yml\", 3)];\n",
        "\n",
        "/// 평탄화하는 이유: 줄 끝 `\\` 이음이 한 명령을 여러 줄에 나눈다.\n",
        "/// 줄 단위로 보면 `cargo test --workspace` 에서 끊겨 뒤 플래그를 놓친다.\n",
    );
    assert!(
        claim_offsets(explaining, "src/x.rs").is_empty(),
        "설명문 안의 인용을 주장으로 고발했다 — 표지가 그 서술 밖(위 상수)에 있는데도 걸렸다"
    );

    // (나) 진짜 주장. 표지가 **같은 서술 안**에 있다. 여전히 걸려야 한다.
    let claiming = "/// 이 가드는 `cargo test --workspace` 로 CI 에서 자동으로 강제된다.\n";
    assert_eq!(
        claim_offsets(claiming, "src/x.rs").len(),
        1,
        "표지가 같은 서술 안에 있는 진짜 주장을 놓쳤다 — 축이 잠잠해졌다"
    );

    // (다) 대조: (가)의 표지를 그 서술 안으로 옮기면 걸린다. 못 잡는 이유가 "설명문이라
    // 봐준다" 가 아니라 **표지가 그 서술에 없어서**라는 것.
    let claiming_in_scope =
        "/// `test.yml` 이 `cargo test --workspace` 로 이 가드를 자동 강제한다.\n";
    assert_eq!(claim_offsets(claiming_in_scope, "src/x.rs").len(), 1);
}

/// `-p` 좁힘을 판정하려면 **패키지가 둘 이상인 합성 레포**가 필요하다. [`fake_repo`] 는
/// 루트 `tests/` 만 만든다 — 그 형태로는 "다른 패키지의 타깃" 을 표현할 수 없다.
fn fake_two_package_repo(name: &str) -> PathBuf {
    let dir = fake_repo(name, &[("root_side.rs", &one_test("a"))], &[]);
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname = \"rootpkg\"\n")
        .expect("루트 매니페스트를 쓰지 못했다");
    let sub = dir.join("crates/subpkg");
    std::fs::create_dir_all(sub.join("tests")).expect("합성 크레이트를 만들지 못했다");
    std::fs::write(sub.join("Cargo.toml"), "[package]\nname = \"subpkg\"\n")
        .expect("크레이트 매니페스트를 쓰지 못했다");
    std::fs::write(sub.join("tests/sub_side.rs"), one_test("b"))
        .expect("크레이트 타깃을 쓰지 못했다");
    dir
}

/// `-p <패키지>` 로 좁힌 잡은 **그 패키지의 타깃만** 돌린다.
///
/// 이 축이 없던 동안 `-p` 호출은 `--lib`/`--bins`/`--test` 가 없다는 이유로 "안 좁혀진
/// 전체 호출" 로 읽혔고, 한 패키지짜리 잡 하나가 레포의 모든 통합 타깃에 채널을 줬다.
/// 그 결과는 침묵이 아니라 **거짓 고발**이었다 — 참인 "자동 채널 없음" 서술 여덟 자리가
/// 한꺼번에 걸렸다. 그래서 이 테스트는 두 방향을 함께 본다: 남의 것은 안 받고, **자기
/// 것은 받는다.** 뒤쪽이 없으면 `packages_named` 가 언제나 전부를 거르도록 망가져도
/// 앞쪽 단언은 초록이다.
#[test]
fn a_package_narrowed_job_does_not_reach_another_package() {
    let dir = fake_two_package_repo("pkgnarrow");
    let invocations = vec![(
        Combo::Default,
        " -p subpkg --locked --no-fail-fast\n".to_string(),
    )];
    let features = std::collections::BTreeMap::new();

    assert_eq!(
        integration_target_channels(&dir, "sub_side", &invocations, &features),
        combos(&[Combo::Default]),
        "지목된 패키지 자신의 타깃은 채널을 받아야 한다"
    );
    assert_eq!(
        integration_target_channels(&dir, "root_side", &invocations, &features),
        combos(&[]),
        "`-p subpkg` 가 루트 패키지의 타깃에까지 채널을 줬다"
    );

    // `-p` 가 없는 같은 호출은 둘 다 돌린다 — 좁힘을 만든 것이 `-p` 라는 대조.
    let wide = vec![(Combo::Default, " --workspace --locked\n".to_string())];
    assert_eq!(
        integration_target_channels(&dir, "root_side", &wide, &features),
        combos(&[Combo::Default])
    );
}

/// `-p <다른 패키지>` 잡은 **본체의 lib 유닛 채널이 아니다.**
///
/// 역방향 축은 "`--lib`/`--test`/`--bins` 가 없으면 전부 돈다" 로 읽는다. `-p` 를 안 보면
/// 문서 가드 잡 하나가 그 축에 "lib 유닛도 자동으로 돈다" 는 근거를 준다.
#[test]
fn a_package_narrowed_job_is_not_a_lib_channel() {
    let dir = fake_two_package_repo("pkglib");
    std::fs::write(
        dir.join(".github/workflows/only-sub.yml"),
        "on:\n  push:\n    branches: [main]\njobs:\n  j:\n    steps:\n      - run: cargo test -p subpkg --locked\n",
    )
    .expect("합성 워크플로를 쓰지 못했다");
    assert!(!lib_tests_run_automatically(&dir));

    std::fs::write(
        dir.join(".github/workflows/only-sub.yml"),
        "on:\n  push:\n    branches: [main]\njobs:\n  j:\n    steps:\n      - run: cargo test -p rootpkg --locked\n",
    )
    .expect("합성 워크플로를 쓰지 못했다");
    assert!(
        lib_tests_run_automatically(&dir),
        "본체 패키지를 지목한 잡은 lib 채널이 맞다 — 이 대조가 없으면 위 단언은 언제나 참이다"
    );
}

fn one_test(name: &str) -> String {
    format!("#[test]\nfn {name}() {{}}\n")
}

fn combos(list: &[Combo]) -> std::collections::BTreeSet<Combo> {
    list.iter().copied().collect()
}

/// 스텝 **이름**은 명령이 아니다.
///
/// 앞 형태는 잡 본문에서 `"cargo test"` 를 문자열로 찾았고, 그 문자열은 사람이 읽으라고
/// 붙인 `- name: cargo test (unit)` 에도 있다. 라벨에는 좁힘 플래그가 없으니 "안 좁혀진
/// 호출" 로 읽혔고, 그 하나 때문에 두 번째 축이 통째로 침묵했다.
#[test]
fn a_step_name_is_not_a_command() {
    let body = "  j:\n    steps:\n      - name: cargo test (unit)\n        shell: pwsh\n        run: cargo test --workspace --lib --bins --locked\n";
    let tails = cargo_test_tails(body);
    assert_eq!(tails.len(), 1, "스텝 이름을 명령으로 읽었다: {tails:?}");
    assert!(is_narrowed(&tails[0]), "좁힘을 못 봤다: {tails:?}");
}

/// YAML 접힘 스칼라(`run: >`)의 여러 줄은 **한 명령**이다.
#[test]
fn a_folded_scalar_is_one_command() {
    let body = "  j:\n    steps:\n      - name: t\n        run: >\n          cargo test --locked --no-default-features\n          --test alpha\n          --test beta\n";
    let tails = cargo_test_tails(body);
    assert_eq!(tails.len(), 1, "접힌 명령을 쪼갰다: {tails:?}");
    assert!(
        is_narrowed(&tails[0]),
        "접힌 줄에 놓인 --test 를 못 봤다: {tails:?}"
    );
    // 꼬리가 아니라 **명령 전체**를 본다. 지시자는 `cargo test` 앞에 붙으므로 꼬리만
    // 보는 단언은 그것을 못 본다 — 첫 형태가 그랬고 변이가 살아남았다.
    let commands = run_commands(body);
    assert_eq!(commands.len(), 1, "접힌 블록을 쪼갰다: {commands:?}");
    assert!(
        commands[0].trim_start().starts_with("cargo"),
        "블록 지시자를 명령 내용으로 읽었다: {commands:?}"
    );
}

/// 리터럴 스칼라(`run: |`)는 셸의 `\` 규칙을 그대로 따른다 — 그리고 줄이 여럿이면 명령도
/// 여럿이다.
#[test]
fn a_literal_scalar_keeps_the_shell_continuation_rule() {
    let body = "  j:\n    steps:\n      - name: t\n        run: |\n          cargo test --workspace --locked -- \\\n            --skip only_this\n          cargo test --lib\n";
    let tails = cargo_test_tails(body);
    // 두 줄은 두 명령이다. 접힘으로 읽으면 하나로 붙고, 그러면 둘째 줄의 `--lib` 가
    // 첫째 명령의 좁힘으로 읽혀 전체 스위트 판정이 뒤집힌다.
    assert_eq!(
        tails.len(),
        2,
        "리터럴 블록의 두 명령을 하나로 붙였다: {tails:?}"
    );
    assert!(
        !is_narrowed(&tails[0]),
        "다음 줄의 --lib 를 첫 명령의 좁힘으로 읽었다: {tails:?}"
    );
    assert_eq!(skip_names(&tails[0]), vec!["only_this".to_string()]);
}

/// `required-features` 가 걸린 타깃은 헤드리스 조합에서 만들어지지 않는다.
#[test]
fn a_required_feature_keeps_a_target_off_the_headless_channel() {
    let root = fake_repo(
        "reqfeat",
        &[("guarded.rs", &one_test("t"))],
        &[("w.yml", AUTOMATIC_FULL)],
    );
    let invocations = vec![(
        Combo::Headless,
        " --workspace --no-default-features".to_string(),
    )];
    let mut features = std::collections::BTreeMap::new();
    features.insert(
        "guarded".to_string(),
        (vec!["gui".to_string()], vec!["gui".to_string()]),
    );
    assert!(
        integration_target_channels(&root, "guarded", &invocations, &features).is_empty(),
        "gui 를 요구하는 타깃이 헤드리스 조합에서 돈다고 판정했다"
    );
    let default_only = vec![(Combo::Default, " --workspace".to_string())];
    assert_eq!(
        integration_target_channels(&root, "guarded", &default_only, &features),
        combos(&[Combo::Default]),
        "기본 조합에서는 그 타깃이 만들어지는데 채널이 없다고 판정했다"
    );
    // 정리 — 실패해도 임시 디렉토리가 남을 뿐이라 테스트 결과에 영향이 없다.
    let _ = std::fs::remove_dir_all(&root);
}

/// `--skip` 이 그 타깃의 **모든** 테스트를 덮으면 실행 채널이 아니다. 하나라도 남으면
/// 채널이다 — 부분 skip 을 전면 부재로 읽으면 참인 채널을 지운다.
#[test]
fn a_skip_that_covers_every_test_removes_the_targets_channel() {
    let root = fake_repo(
        "skips",
        &[
            ("whole.rs", &one_test("all_of_it")),
            (
                "partial.rs",
                &format!("{}{}", one_test("all_of_it"), one_test("survivor")),
            ),
        ],
        &[("w.yml", AUTOMATIC_FULL)],
    );
    let invocations = vec![(
        Combo::Headless,
        " --workspace --no-default-features -- --skip all_of_it".to_string(),
    )];
    let features = std::collections::BTreeMap::new();
    assert!(
        integration_target_channels(&root, "whole", &invocations, &features).is_empty(),
        "모든 테스트가 skip 된 타깃을 실행 채널로 셌다"
    );
    assert_eq!(
        integration_target_channels(&root, "partial", &invocations, &features),
        combos(&[Combo::Headless]),
        "일부만 skip 된 타깃의 채널을 지웠다"
    );
    // 정리 — 실패해도 임시 디렉토리가 남을 뿐이라 테스트 결과에 영향이 없다.
    let _ = std::fs::remove_dir_all(&root);
}

/// 양성 필터(`-- <이름>`)가 타깃을 좁히면 그 호출은 타깃 전체의 채널이 아니다.
/// 좁히지 않으면(모든 테스트가 필터에 걸리면) 채널이다 — `--skip` 규칙의 대칭이다.
///
/// 이 구분이 없으면 한 건만 지목한 관측용 스텝 하나가 그 타깃 전체의 실행 채널로
/// 세어지고, 그 타깃에 새 테스트가 생겨도 자동으로 돈다고 잘못 말하게 된다.
#[test]
fn a_positive_filter_that_narrows_the_target_is_not_its_channel() {
    let root = fake_repo(
        "filters",
        &[
            (
                "narrowed.rs",
                &format!("{}{}", one_test("chosen_one"), one_test("left_out")),
            ),
            ("whole.rs", &one_test("chosen_one")),
        ],
        &[("w.yml", AUTOMATIC_FULL)],
    );
    let invocations = vec![(
        Combo::Headless,
        " --workspace --no-default-features -- chosen_one --exact".to_string(),
    )];
    let features = std::collections::BTreeMap::new();
    assert!(
        integration_target_channels(&root, "narrowed", &invocations, &features).is_empty(),
        "한 건만 고른 호출을 타깃 전체의 실행 채널로 셌다"
    );
    assert_eq!(
        integration_target_channels(&root, "whole", &invocations, &features),
        combos(&[Combo::Headless]),
        "필터가 그 타깃의 모든 테스트를 덮는데 채널을 지웠다"
    );
    // `--exact` 가 없으면 부분일치라 판정이 달라진다 — 그 축도 함께 고정한다.
    let loose = vec![(
        Combo::Headless,
        " --workspace --no-default-features -- chosen".to_string(),
    )];
    assert!(
        integration_target_channels(&root, "narrowed", &loose, &features).is_empty(),
        "부분일치 필터도 좁히는 것은 마찬가지다"
    );
    // 정리 — 실패해도 임시 디렉토리가 남을 뿐이라 테스트 결과에 영향이 없다.
    let _ = std::fs::remove_dir_all(&root);
}

/// `--test <이름>` 으로 지목한 호출은 **그 이름에만** 채널을 준다. 그리고 존재하지 않는
/// 타깃에는 채널이 없다 — 자리표시자(`tests/X.rs`)를 실재하는 타깃으로 읽으면 없는
/// 테스트의 채널을 근거로 고발하게 된다.
#[test]
fn a_named_target_gets_the_channel_only_for_itself() {
    let root = fake_repo(
        "named",
        &[("alpha.rs", &one_test("a")), ("beta.rs", &one_test("b"))],
        &[("w.yml", AUTOMATIC_FULL)],
    );
    let invocations = vec![(
        Combo::Headless,
        " --locked --no-default-features --test alpha".to_string(),
    )];
    let features = std::collections::BTreeMap::new();
    assert_eq!(
        integration_target_channels(&root, "alpha", &invocations, &features),
        combos(&[Combo::Headless])
    );
    assert!(
        integration_target_channels(&root, "beta", &invocations, &features).is_empty(),
        "지목되지 않은 타깃에 채널을 줬다"
    );
    assert!(
        integration_target_channels(&root, "nonexistent", &invocations, &features).is_empty(),
        "존재하지 않는 타깃에 채널을 줬다"
    );
    // 정리 — 실패해도 임시 디렉토리가 남을 뿐이라 테스트 결과에 영향이 없다.
    let _ = std::fs::remove_dir_all(&root);
}

/// 역방향 축의 전제("lib 은 자동으로 돈다")도 **워크플로에서** 읽는다.
#[test]
fn the_lib_axis_reads_the_workflow_instead_of_assuming() {
    let with_lib = fake_repo(
        "libyes",
        &[],
        &[(
            "w.yml",
            "on:\n  push:\n    branches: [main]\njobs:\n  a:\n    steps:\n      - name: t\n        run: cargo test --workspace --lib --bins --locked\n",
        )],
    );
    assert!(lib_tests_run_automatically(&with_lib));
    let named_only = fake_repo(
        "libno",
        &[],
        &[(
            "w.yml",
            "on:\n  push:\n    branches: [main]\njobs:\n  a:\n    steps:\n      - name: t\n        run: cargo test --locked --test alpha\n",
        )],
    );
    assert!(
        !lib_tests_run_automatically(&named_only),
        "`--test` 로만 좁힌 자동 잡을 lib 채널로 읽었다"
    );
    for dir in [&with_lib, &named_only] {
        // 정리 — 실패해도 임시 디렉토리가 남을 뿐이라 테스트 결과에 영향이 없다.
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// 도는 통합 테스트를 두고 부재를 적으면 위반이고, **조합을 한정하면** 참이다.
#[test]
fn an_absence_claim_about_a_running_integration_test_needs_the_combination() {
    let bare = "`tests/alpha.rs` 가 강제한다 — 자동 채널이 없다.\n";
    let violations = overstated_absence(
        bare,
        "docs/x.md",
        &|_| combos(&[Combo::Headless]),
        &combos(&[]),
    );
    assert_eq!(violations.len(), 1, "도는 테스트의 부재 주장을 놓쳤다");

    let qualified = "`tests/alpha.rs` 가 강제한다 — 기본 조합에는 자동 채널이 없다(자동 실행은 헤드리스에서만).\n";
    assert!(
        overstated_absence(
            qualified,
            "docs/x.md",
            &|_| combos(&[Combo::Headless]),
            &combos(&[])
        )
        .is_empty(),
        "조합을 한정해 정확히 쓴 문장을 위반으로 짚었다"
    );
}

/// 채널이 없는 테스트의 부재 주장은 그대로 참이다 — 이 축이 **참인 문장을 지우지 않는다.**
#[test]
fn a_test_with_no_channel_keeps_its_absence_claim() {
    let text = "`tests/alpha.rs` 는 자동 채널이 없다.\n";
    assert!(
        overstated_absence(text, "docs/x.md", &|_| combos(&[]), &combos(&[])).is_empty(),
        "채널이 0 인데 위반으로 짚었다"
    );
}

/// 두 조합 모두에서 도는 테스트는 **어떻게 한정해도** 부재 서술이 거짓이다.
#[test]
fn two_channels_cannot_be_qualified_away() {
    let text = "`tests/alpha.rs` 는 기본 조합에는 자동 채널이 없다(헤드리스에서만 돈다).\n";
    let violations = overstated_absence(
        text,
        "docs/x.md",
        &|_| combos(&[Combo::Default, Combo::Headless]),
        &combos(&[]),
    );
    assert_eq!(
        violations.len(),
        1,
        "두 조합에서 도는데 한정 표지 하나로 면제됐다"
    );
}

/// **배타 주장도 부재 주장이다** — "X 잡에서만 돈다" 는 X 밖의 채널을 부정한다.
///
/// 이 형태가 표지 목록에 없던 동안, 조합이 하나에서 둘로 늘어난 타깃 둘이 **옛 잡 하나만
/// 적은 채** 살아 있었다. 부재 어휘("없다")를 하나도 안 쓰기 때문에 다른 표지에 안 걸린다.
/// 늘어난 쪽(고발돼야 한다)과 정확히 적은 쪽(고발되면 안 된다)을 같은 자리에서 못박는다.
#[test]
fn an_exclusive_job_claim_is_an_absence_claim() {
    // 표지는 조각으로 조립한다 — 통째로 적으면 이 파일 자신이 위반으로 잡힌다.
    let only = format!("{}일어난다", "에서만 ");

    // (+) 조합이 둘인데 잡 하나만 적었다. 부재 어휘가 없으므로, 걸린다면 이 표지 때문이다.
    let overstated = format!("`tests/alpha.rs` 가 강제한다 — 자동 실행은 그 잡{only}.\n");
    let violations = overstated_absence(
        &overstated,
        "docs/x.md",
        &|_| combos(&[Combo::Default, Combo::Headless]),
        &combos(&[]),
    );
    assert_eq!(
        violations.len(),
        1,
        "두 조합에서 도는데 배타 주장이 통과했다: {violations:?}"
    );

    // (−) 조합이 하나뿐이고 그 조합을 함께 적었다 — 참인 문장이므로 통과해야 한다.
    let exact = format!("`tests/alpha.rs` 가 강제한다 — 자동 실행은 `check-headless` 잡{only}.\n");
    assert!(
        overstated_absence(
            &exact,
            "docs/x.md",
            &|_| combos(&[Combo::Headless]),
            &combos(&[])
        )
        .is_empty(),
        "조합을 함께 적은 배타 주장을 위반으로 짚었다"
    );

    // (±) 같은 문장이라도 조합이 둘로 늘면 거짓이 된다 — 실제로 이렇게 낡았다.
    assert_eq!(
        overstated_absence(
            &exact,
            "docs/x.md",
            &|_| combos(&[Combo::Default, Combo::Headless]),
            &combos(&[])
        )
        .len(),
        1,
        "조합이 둘로 늘었는데 옛 한정 표지가 계속 면제했다"
    );
}

/// 부재를 **주장하지 않은** 서술은 이 축에 들어오지 않는다.
///
/// 판단을 유보하고 이유를 적은 문장에 벌을 주면 가드가 사람을 침묵시키는 쪽으로
/// 작동한다 — 정확히 쓰는 것보다 아무 말도 안 하는 것이 싸지면 안 된다.
#[test]
fn a_statement_that_asserts_no_absence_is_not_judged() {
    let text = "`tests/alpha.rs` 의 채널은 여기서 단정하지 않는다 — 정본은 ci-gates 다.\n";
    assert!(
        overstated_absence(
            text,
            "docs/x.md",
            &|_| combos(&[Combo::Headless]),
            &combos(&[])
        )
        .is_empty(),
        "부재를 주장하지 않은 문장을 짚었다"
    );
}

/// `src/` 파일도 이 축의 대상이다 — 통합 타깃은 거기서 정의될 수 없기 때문이다.
///
/// 앞 형태는 `src/` 를 통째로 면제했다. 그 면제가 겨냥한 것은 "정의 파일의 이름 등장은
/// 서술이 아니다" 인데, 이 축의 대상은 `tests/X.rs` 라는 **위치**로 정의되는 통합 타깃이라
/// `src/` 파일은 그것을 정의할 수 없다. 면제 사유가 성립하지 않는 자리였고, 그 사이 `src/`
/// 모듈 doc 의 채널 주장이 통째로 사각이었다.
#[test]
fn a_source_file_is_judged_because_it_cannot_define_an_integration_target() {
    let text = "// `tests/alpha.rs` 는 자동 채널이 없다.\n";
    assert_eq!(
        overstated_absence(
            text,
            "src/x.rs",
            &|_| combos(&[Combo::Headless]),
            &combos(&[])
        )
        .len(),
        1,
        "src/ 를 통째로 면제해 모듈 doc 의 채널 주장을 놓쳤다"
    );
}

/// 부류 지목(`tests/*.rs`)도 이름 지목과 같은 자격으로 판정된다.
#[test]
fn a_class_citation_is_judged_against_the_whole_class() {
    let text = "`tests/*.rs` 는 자동 채널이 없다.\n";
    assert_eq!(
        overstated_absence(
            text,
            "docs/x.md",
            &|_| combos(&[]),
            &combos(&[Combo::Headless])
        )
        .len(),
        1,
        "부류를 지목한 부재 주장이 판정에서 빠졌다"
    );

    // 대조군 — 부류의 채널이 실제로 비어 있으면 그 주장은 참이다.
    assert!(
        overstated_absence(text, "docs/x.md", &|_| combos(&[]), &combos(&[])).is_empty(),
        "채널이 없는데 부재 주장을 고발했다"
    );
}

const AUTOMATIC_FULL: &str = "on:\n  push:\n    branches: [main]\njobs:\n  a:\n    steps:\n      - name: t\n        run: cargo test --workspace --locked\n";

/// 워크플로 모수는 **두 확장자를 다 본다.**
///
/// 한쪽만 보면 그 잡이 자동 채널 계산에서 통째로 빠지고, 이 가드는 실제보다 약한 채널을
/// 가정한 채 **참인 서술을 위반으로 짚는다.** 이 파일은 스캔 확장자에는 `yaml` 을 넣고
/// 워크플로 판독에서는 뺀 상태였다 — 두 모수가 한 파일 안에서 어긋나 있었다.
#[test]
fn a_workflow_is_read_under_either_extension() {
    for (name, file) in [("yml", "ci.yml"), ("yaml", "ci.yaml")] {
        let dir = workflow_dir(name, &[(file, AUTOMATIC_FULL)]);
        let bodies = automatic_job_bodies_of_dir(&dir);
        assert_eq!(bodies.len(), 1, "{file} 을 워크플로로 읽지 않았다");
        assert!(bodies[0].contains("cargo test --workspace"));
        // 정리 — 실패해도 임시 디렉토리가 남을 뿐이라 테스트 결과에 영향이 없다.
        let _ = std::fs::remove_dir_all(&dir);
    }

    // 워크플로가 아닌 확장자는 들어오지 않는다 — 모수를 넓히는 것과 아무거나 읽는 것은 다르다.
    let dir = workflow_dir("other", &[("notes.md", AUTOMATIC_FULL)]);
    assert!(
        automatic_job_bodies_of_dir(&dir).is_empty(),
        "워크플로가 아닌 파일을 잡 본문으로 읽었다"
    );
    // 정리 — 실패해도 임시 디렉토리가 남을 뿐이라 테스트 결과에 영향이 없다.
    let _ = std::fs::remove_dir_all(&dir);
}

/// 자동 트리거가 없는 워크플로는 두 확장자 어느 쪽이든 제외된다 — 넓힌 것은 **확장자**이지
/// 트리거 판정이 아니다.
#[test]
fn widening_the_extension_does_not_widen_the_trigger_rule() {
    let manual = "on:\n  workflow_dispatch:\njobs:\n  a:\n    steps:\n      - name: t\n        run: cargo test --workspace --locked\n";
    for (name, file) in [("m-yml", "manual.yml"), ("m-yaml", "manual.yaml")] {
        let dir = workflow_dir(name, &[(file, manual)]);
        assert!(
            automatic_job_bodies_of_dir(&dir).is_empty(),
            "{file}: 수동 전용 워크플로가 자동 잡으로 들어왔다"
        );
        // 정리 — 실패해도 임시 디렉토리가 남을 뿐이라 테스트 결과에 영향이 없다.
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn a_claim_that_names_no_test_is_deliberately_not_judged() {
    let text = format!("이 규칙은 {}.\n", enforce());
    let (found, _) = enforcement_violations(&text, "docs/x.md", &no_named_tests());
    assert!(
        found.is_empty(),
        "대상이 특정되지 않은 서술은 판정하지 않는다 — 짚을 좌표가 없다"
    );
}

/// 면제의 사유는 **이 파일이 그 이름을 정의한다**는 성질이지 `src/` 라는 경로가 아니다.
#[test]
fn only_the_file_that_defines_the_name_is_exempt() {
    let libs = named(&["some_lib_test"]);

    // 그 이름을 여기서 정의한다 — 이름 등장은 서술이 아니라 정의다.
    let defines = "//! `some_lib_test` 는 자동 채널이 없다.\n#[test]\nfn some_lib_test() {}\n";
    assert!(
        weak_absence_offsets(defines, "crates/x/src/lib.rs", &libs).is_empty(),
        "정의 파일의 이름 등장을 서술로 읽었다"
    );

    // 남의 이름을 부르는 `src/` 모듈 doc 은 서술이다 — 앞 형태는 경로만 보고 이것까지
    // 면제했다.
    let refers = "//! `some_lib_test` 는 자동 채널이 없다.\n";
    assert_eq!(
        weak_absence_offsets(refers, "crates/x/src/lib.rs", &libs).len(),
        1,
        "남의 이름을 부른 모듈 doc 이 경로 면제로 빠졌다"
    );
}
