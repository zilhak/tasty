//! **출하되는 설치 파일이 늘거나 이름이 바뀌면 설치 가이드가 그것을 알아야 한다.**
//!
//! `CLAUDE.md` 의 "문서 갱신 (필수)" 는 사용자에게 보이는 동작(메뉴·단축키·설정 키·
//! CLI 명령·**설치 절차**)이 바뀌면 공개 사이트의 사용자 가이드도 같은 커밋에서 갱신하라고
//! 요구한다. CLI 축은 별도 가드가 보고, 이 파일은 그 목록의 **설치 절차** 쪽을 맡는다.
//!
//! # 왜 파일명 축만 보는가 — 두 쪽이 같은 어휘를 쓰는 자리가 여기뿐이다
//!
//! "설치 절차" 는 한 덩어리가 아니라 둘이다.
//!
//! - **산출물 파일명** — 릴리스가 올리는 이름과 가이드가 적는 이름이 **같은 문자열**이다.
//!   버전 자리만 다르고(`${VERSION}` ↔ `{ver}`) 나머지는 글자 그대로 같다. 기계가 대조할
//!   수 있는 어휘가 있고, 이 가드가 보는 것이 그것이다.
//! - **절차 본문** — 설치·제거 명령, 설치 위치, glibc 하한, `.msi` 가 `~/.tasty` 를 지운다는
//!   사실. 이쪽은 소스가 WiX 선언과 패키징 스크립트의 내부 변수이고 가이드는 산문이다.
//!   **공통 어휘가 없다.** 이 축을 재는 채널은 **없다** — 그렇게 적어 둔다. 채널이 없는데
//!   있는 것처럼 세지 않으려는 것이다.
//!
//! # 모수
//!
//! `.github/workflows/release.yml` 의 `gh release upload` 가 올리는 이름 전부. 실측
//! (2026-09-06) **15** 개다(설치 파일 11 + 체크섬 4). 릴리스 페이지에 실제로 뜨는 목록이
//! 그것이라, 사용자가 고르는 것과 모수가 정확히 같다.
//!
//! # 이 가드가 단정하지 않는 것
//!
//! - **가이드가 그 파일을 제대로 설명하는지.** 이름이 한 번 나오면 통과다.
//! - **영어 번역(`site/content/en/`).** 원본이 정본이라 여기서 안 본다.
//! - **절차가 맞는지.** 위에 적은 대로 그 축에는 공통 어휘가 없다.
//!
//! # 채널
//!
//! `doc-guards.yml` — main push · PR 마다 경로 필터 없이 돈다. 이 축을 재는 채널은 그 하나다.

use std::path::{Path, PathBuf};
use tasty_doc_guards::temp_scratch::Scratch;

/// 가이드가 **한 행으로 묶어** 분류하는 산출물. 자리는 개별 파일이 아니라 **그 행**이다.
///
/// 이것은 "가이드에 일부러 안 싣는다" 가 아니다 — 싣혀 있는데 **전체 이름으로** 안 적혔을
/// 뿐이다. 그 사실을 예외로 적으면 명부가 거짓말을 하게 되므로 부류를 따로 둔다.
///
/// ★ 이 명부에 새 줄을 더해서 통과시키지 마라. 묶기가 정당한 경우는 **가이드가 이미 그
/// 가족을 한 행으로 다루고 있을 때**뿐이고, 그때도 새 가족이지 새 파일이 아니다. 파일
/// 하나가 늘었으면 답은 가이드에 그 이름을 적는 것이다.
const COMPRESSED_ROWS: &[(&str, &[&str])] = &[(
    "가이드의 설치 파일 표에서 Linux aarch64 행 하나가 이 넷을 함께 분류한다 — x64 네 줄과 \
     형태가 같아 접미사만 적었다. 그 행을 넷으로 펴면 이 등록은 사라진다",
    &[
        "Tasty-{ver}-aarch64.AppImage",
        "tasty-{ver}-1.aarch64.rpm",
        "tasty-{ver}-linux-arm64.tar.gz",
        "tasty_{ver}-1_arm64.deb",
    ],
)];

/// 훑어야 할 최소 산출물 수 — **모수가 살아 있다는 증거**.
///
/// 실측 15(2026-09-06). 여유를 두고 8 로 둔다 — 래칫이 아니라 **생존 바닥**이다.
const MIN_ARTIFACTS: usize = 8;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// 체크섬 파일인가 — **모수 밖**이다.
///
/// 사용자가 고르는 설치 파일이 아니라 고른 뒤 검증에 쓰는 부속이고, 가이드도 그렇게
/// 다룬다(`SHA256SUMS-*.txt` 한 줄이 넷을 한꺼번에 가리킨다). 이름으로 넷을 요구하면
/// 표에 아무도 안 읽을 네 줄이 는다.
///
/// 경계를 두되 조용히 두지 않는다 — 이 가족이 가이드에 **한 번은** 언급되는지를
/// [`the_checksum_family_is_classified_once_by_a_glob`] 이 따로 단정한다.
fn is_checksum(name: &str) -> bool {
    name.starts_with("SHA256SUMS")
}

/// 릴리스가 올리는 산출물 이름 — 버전 자리를 가이드 표기로 정규화해서 낸다.
fn released_artifacts(root: &Path) -> Vec<String> {
    let text = std::fs::read_to_string(root.join(".github/workflows/release.yml"))
        .expect("release.yml 을 읽지 못했다");
    let mut out = Vec::new();
    for line in text.lines() {
        if !line.contains("gh release upload") {
            continue;
        }
        // 마지막 인자가 `"dist/<이름>"` 이다.
        let Some(start) = line.rfind("\"dist/") else {
            continue;
        };
        let rest = &line[start + "\"dist/".len()..];
        let Some(end) = rest.find('"') else {
            continue;
        };
        out.push(rest[..end].replace("${VERSION}", "{ver}"));
    }
    out.sort();
    out.dedup();
    out
}

/// 한국어 가이드 원본 전체를 한 덩어리로.
/// `site/content` 의 최상위 갈래. **순회 밖에 있어야** 가지치기가 넓어져 한 갈래가
/// 통째로 빠진 것을 잡는다 — 순회가 본 것으로 이 목록을 만들면 빠진 갈래는 목록에서도
/// 빠진다. `en` 은 번역이라 순회가 일부러 건너뛰므로 여기 없다.
const GUIDE_BRANCHES: &[&str] = &[
    "agents",
    "customize",
    "getting-started",
    "help",
    "plugins",
    "remote",
    "using",
];

/// 가이드 본문 한 벌. **갈래마다 하나라도 닿았는지 확인하고 돌려준다.**
///
/// 이 함수가 돌려주는 문자열은 아래 판정들의 **우변**이다. 좌변(등록 명부·명령 목록)은
/// 코드 상수라 절대 안 비는데, 우변이 조용히 줄면 "가이드에 없다"·"가이드에 이미 있다"
/// 가 **둘 다 초록**이 된다. 그래서 두 자리를 막는다.
///
/// 1. `read_dir` 실패를 **안 삼킨다.** 예전 판은 `let Ok(..) else { return }` 이라
///    권한·경합으로 한 디렉토리를 못 읽으면 그 갈래가 통째로 빠진 채 초록이었다.
/// 2. `site/content` 의 **최상위 갈래마다** `.md` 를 하나라도 담았는지 본다.
///
/// **실측 2026-09-08(`12bc0f4b2`)**: 갈래 하나를 순회에서 빼고 세 파일을 돌리는 변이를
/// 7 갈래 × 3 파일 = 21 칸으로 재니 **15 칸이 초록**이었다. `help/` 와 `plugins/` 는
/// 세 파일 **전부**가 못 잡았다. 지금은 21 칸 전부가 이 함수에서 죽는다.
///
/// **명부를 순회 밖에 둔다 — 첫 판은 순회 안에서 갈래를 모았고 그것이 틀렸다.**
/// 순회가 본 갈래만 모으면 가지치기로 빠진 갈래는 **목록에도 안 들어가서** 확인 대상이
/// 아니게 된다. 위 21 칸 변이를 그 판에 대고 재니 15 칸이 그대로 초록이었다 — 좌변을
/// 재는 사본과 판정하는 사본이 같으면 그 둘이 함께 줄어든다(R1116 과 같은 형태다).
/// 그래서 [`GUIDE_BRANCHES`] 는 상수고, 그 명부가 낡는 것은 반대 방향 판정이 잡는다.
/// 하한(`>= N`)도 안 쓴다. 갈래 확인은 여유가 필요 없고, 순회가 통째로 죽는 것과 한
/// 갈래만 빠지는 것을 같은 판정으로 잡는다.
fn guide_text(root: &Path) -> String {
    guide_scan(root).0
}

/// 순회 본체. 본문 · **명부와 대조할 두 집합**을 함께 낸다.
///
/// 갈래 확인이 [`guide_text`] 안에 있으면 **합성 트리를 먹는 형제 시험이 깨진다** —
/// 그 트리에는 레포의 갈래 일곱이 없다. 그래서 순회와 판정을 갈랐다: 여기서는 읽기
/// 실패만 막고, 명부 대조는 레포를 상대로만 도는 별도 시험이 한다.
fn guide_scan(
    root: &Path,
) -> (
    String,
    std::collections::BTreeSet<String>,
    std::collections::BTreeSet<String>,
) {
    fn walk(
        dir: &Path,
        top: &Path,
        out: &mut String,
        branches: &mut std::collections::BTreeSet<String>,
        touched: &mut std::collections::BTreeSet<String>,
        current: Option<&str>,
    ) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| {
            panic!(
                "가이드 순회가 {} 를 못 읽었다 — {e}\n\
                 조용히 건너뛰면 그 갈래가 통째로 빠진 채 이 파일의 판정이 초록으로 \
                 나온다. 우변이 비면 \"가이드에 없다\" 도 \"가이드에 이미 있다\" 도 \
                 참이 된다.",
                dir.display()
            )
        });
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().map(|n| n == "en").unwrap_or(false) {
                    continue; // 번역은 별도 절차다.
                }
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let next = if dir == top {
                    branches.insert(name.clone());
                    Some(name)
                } else {
                    current.map(str::to_owned)
                };
                walk(&path, top, out, branches, touched, next.as_deref());
            } else if path.extension().and_then(|e| e.to_str()) == Some("md")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                if let Some(b) = current {
                    touched.insert(b.to_owned());
                }
                out.push_str(&text);
                out.push('\n');
            }
        }
    }
    let top = root.join("site/content");
    let mut out = String::new();
    let (mut branches, mut touched) = (
        std::collections::BTreeSet::new(),
        std::collections::BTreeSet::new(),
    );
    walk(&top, &top, &mut out, &mut branches, &mut touched, None);
    (out, branches, touched)
}

/// [`GUIDE_BRANCHES`] 의 판정 — 순회가 **갈래마다 하나라도 닿았는가**, 그리고 그 명부가
/// 낡지 않았는가. 두 방향이라 갈래가 빠져도, 늘어도 잡힌다.
#[test]
fn the_guide_walk_reaches_every_branch() {
    let (_, branches, touched) = guide_scan(&repo_root());
    let missing: Vec<&&str> = GUIDE_BRANCHES
        .iter()
        .filter(|b| !touched.contains(**b))
        .collect();
    assert!(
        missing.is_empty(),
        "가이드 순회가 이 갈래에서 `.md` 를 하나도 안 담았다: {missing:?}\n\
         명부 {} 개 중 {} 개만 닿았다. 가지치기가 넓어졌거나 그 디렉토리가 비었다 — \
         이 파일의 판정은 우변이 줄면 **더 조용히** 초록이 되므로, 갈래를 빼서 \
         통과시키지 마라.",
        GUIDE_BRANCHES.len(),
        touched.len()
    );
    let extra: Vec<&String> = branches
        .iter()
        .filter(|b| !GUIDE_BRANCHES.contains(&b.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "`site/content` 에 명부에 없는 갈래가 있다: {extra:?}\n\
         `GUIDE_BRANCHES` 에 추가해라 — 안 하면 그 갈래는 위 확인의 대상이 아니라서 \
         통째로 빠져도 초록이다."
    );
}

fn compressed() -> Vec<&'static str> {
    COMPRESSED_ROWS
        .iter()
        .flat_map(|(_, ns)| *ns)
        .copied()
        .collect()
}

#[test]
fn every_released_artifact_is_named_in_the_guide_or_covered_by_a_row() {
    let root = repo_root();
    let artifacts = released_artifacts(&root);
    assert!(
        artifacts.len() >= MIN_ARTIFACTS,
        "릴리스 산출물을 {}개밖에 못 찾았다(하한 {MIN_ARTIFACTS}) — 추출이 깨졌다.\n\
         ★ 이 수를 내려서 통과시키지 마라. 먼저 가른다 — `release.yml` 의 업로드 줄이 정말 \
         줄었나, 아니면 그 줄의 모양이 바뀌어 `released_artifacts` 가 못 읽나. 뒤쪽이면 \
         하한을 내리는 것은 고장을 초록으로 만드는 것이다.",
        artifacts.len()
    );

    let guide = guide_text(&root);
    let rows = compressed();
    let missing: Vec<&String> = artifacts
        .iter()
        .filter(|a| !is_checksum(a))
        .filter(|a| !guide.contains(a.as_str()))
        .filter(|a| !rows.contains(&a.as_str()))
        .collect();

    assert!(
        missing.is_empty(),
        "릴리스가 올리는 설치 파일인데 사용자 가이드(`site/content/`)가 그 이름을 한 번도 \
         안 적는다:\n  {}\n\n\
         `CLAUDE.md` 의 \"문서 갱신 (필수)\" 는 설치 절차가 바뀌면 가이드도 **같은 커밋에서** \
         갱신하라고 요구한다. 고치는 길 둘:\n\
           (가) 설치 가이드의 설치 파일 표에 그 이름을 적는다 — 독자는 릴리스 페이지의 \
         목록과 이 표를 눈으로 맞춘다. 이름이 없으면 자기 것이 어느 것인지 못 고른다.\n\
           (나) 가이드가 이미 그 **가족**을 한 행으로 다루고 있으면 `COMPRESSED_ROWS` 에 \
         그 행을 자리로 등록한다. ★ 파일 하나가 늘어난 경우는 여기 해당하지 않는다.",
        missing
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );
}

#[test]
fn no_compressed_row_entry_is_already_named_in_full() {
    let root = repo_root();
    let guide = guide_text(&root);
    let stale: Vec<&str> = compressed()
        .into_iter()
        .filter(|n| guide.contains(n))
        .collect();
    assert!(
        stale.is_empty(),
        "가이드가 이 이름을 이미 전체로 적는데 `COMPRESSED_ROWS` 에 남아 있다: {stale:?}\n\
         묶음 등록은 '전체 이름으로 안 적혔다' 는 사실의 기록이다. 적혔으면 그 줄을 지워라 — \
         안 지우면 다음 사람이 가이드를 낡은 것으로 읽는다."
    );
}

#[test]
fn every_compressed_row_entry_is_still_released() {
    let root = repo_root();
    let artifacts = released_artifacts(&root);
    let dead: Vec<&str> = compressed()
        .into_iter()
        .filter(|n| !artifacts.iter().any(|a| a == n))
        .collect();
    assert!(
        dead.is_empty(),
        "`COMPRESSED_ROWS` 가 이제 안 올라가는 산출물을 붙들고 있다: {dead:?}\n\
         출하가 끊긴 파일은 가이드에서도 빠져야 한다 — 명부만 지우고 표를 그대로 두면 \
         독자가 없는 파일을 찾는다."
    );
}

#[test]
fn the_checksum_family_is_classified_once_by_a_glob() {
    let root = repo_root();
    let artifacts = released_artifacts(&root);
    let checksums: Vec<&String> = artifacts.iter().filter(|a| is_checksum(a)).collect();
    assert!(
        !checksums.is_empty(),
        "체크섬 파일이 하나도 안 올라간다 — 모수 밖으로 두던 근거가 사라졌다. \
         `is_checksum` 경계를 다시 판단해라."
    );
    let guide = guide_text(&root);
    assert!(
        guide.contains("SHA256SUMS"),
        "체크섬 {}개를 모수 밖으로 두는 근거는 '가이드가 한 줄로 함께 가리킨다' 였는데, \
         가이드에 `SHA256SUMS` 가 한 번도 안 나온다. 경계가 아니라 누락이다.",
        checksums.len()
    );
}

#[test]
fn the_reader_answers_both_yes_and_no() {
    let root = repo_root();
    let artifacts = released_artifacts(&root);
    assert!(
        artifacts.iter().any(|a| a.contains(".AppImage")),
        "AppImage 를 못 읽었다 — 업로드 줄 파싱이 깨졌다"
    );
    assert!(
        artifacts.iter().any(|a| a.contains("{ver}")),
        "버전 자리를 정규화하지 못했다 — 가이드 표기와 대조가 성립하지 않는다"
    );
    assert!(
        !artifacts.iter().any(|a| a.contains("${VERSION}")),
        "정규화가 안 된 이름이 남았다"
    );
    assert!(is_checksum("SHA256SUMS-macos.txt"));
    assert!(!is_checksum("Tasty-{ver}-macos-arm64.dmg"));

    let guide = guide_text(&root);
    assert!(guide.contains("Tasty-{ver}-macos-arm64.dmg"), "예: 있음");
    assert!(
        !guide.contains("Tasty-{ver}-macos-x86_64.dmg"),
        "예: 없음 — 없는 것을 있다고 읽으면 이 가드는 아무것도 안 본다"
    );
}

/// **양성 대조 — 두 판독을 합성 입력으로 건다.**
///
/// 이 가드가 가진 수치 레버는 `MIN_ARTIFACTS` 하나뿐이고 그것은 하한이라 **좁아지는
/// 쪽만** 본다. 판독이 넓어지는 변이 — 마지막 인자 규칙을 놓치거나, 번역을 원본에
/// 섞거나 — 는 수를 늘리거나 본문을 늘리므로 하한이 조용하다. 그 방향은 이 대조만 본다.
///
/// 특히 `guide_text` 쪽은 **하한이 아예 없다.** 순회가 죽으면 본문이 비고, 그때
/// "가이드가 산출물 이름을 다 적는다" 는 시끄럽게 죽으므로 그 방향은 안전하다 — 위험한
/// 것은 반대쪽, 본문이 **넘치게** 모이는 쪽이다.
///
/// ★ 이름은 전부 합성이다(R1078). 진짜 산출물 이름으로 지으면 판독의 *메커니즘*이 아니라
/// 그 이름의 *현재 값*을 재게 되고, 릴리스 산출물이 정당하게 바뀌는 날 함께 죽는다.
#[test]
fn both_readers_answer_on_a_substituted_tree() {
    let probe = Scratch::new("released-artifact-reader");
    let dir = probe.path();

    // ── 판독 1: 릴리스가 올리는 산출물 이름 ─────────────────────────────
    let wf = dir.join(".github/workflows");
    std::fs::create_dir_all(&wf).expect("합성 워크플로 트리를 만들지 못했다");
    std::fs::write(
        wf.join("release.yml"),
        concat!(
            // 버전 자리가 가이드 표기로 접히는가.
            "        run: gh release upload \"$TAG\" \"dist/alpha-${VERSION}-probe.tar.gz\"\n",
            "        run: gh release upload \"$TAG\" \"dist/zeta-widget.msix\"\n",
            // `gh release upload` 가 없는 줄 — 모수 밖이다.
            "        run: echo \"dist/never-uploaded.zip\"\n",
            // 인자가 둘일 때 **마지막**을 쓰는가(`rfind`).
            "        run: gh release upload \"dist/first-arg.zip\" \"dist/omega-last.deb\"\n",
        ),
    )
    .expect("합성 release.yml 을 쓰지 못했다");

    let artifacts = released_artifacts(dir);
    assert_eq!(
        artifacts,
        vec![
            "alpha-{ver}-probe.tar.gz".to_string(),
            "omega-last.deb".to_string(),
            "zeta-widget.msix".to_string(),
        ],
        "판독이 합성 트리에서 다른 답을 냈다"
    );
    // 죽었을 때 **무엇이** 깨졌는지 가려 주는 세 줄.
    assert!(
        !artifacts.iter().any(|a| a.contains("never-uploaded")),
        "`gh release upload` 가 없는 줄을 세었다 — 모수가 워크플로 전체로 넓어졌다"
    );
    assert!(
        !artifacts.iter().any(|a| a == "first-arg.zip"),
        "인자가 둘일 때 마지막이 아니라 첫 번째를 읽었다"
    );
    assert!(
        !artifacts.iter().any(|a| a.contains("${VERSION}")),
        "버전 자리를 가이드 표기로 안 접었다 — 그러면 가이드와 절대 안 맞는다"
    );

    // ── 판독 2: 가이드 본문 ──────────────────────────────────────────────
    let content = dir.join("site/content");
    std::fs::create_dir_all(content.join("en")).expect("합성 가이드 트리를 만들지 못했다");
    std::fs::create_dir_all(content.join("sub")).expect("합성 하위 장을 만들지 못했다");
    std::fs::write(
        content.join("install.md"),
        "alpha-{ver}-probe.tar.gz 를 받아라\n",
    )
    .expect("합성 원본 실패");
    std::fs::write(
        content.join("sub").join("deep.md"),
        "deep-marker 가 여기 있다\n",
    )
    .expect("합성 하위 원본 실패");
    std::fs::write(content.join("en").join("install.md"), "translated-omega\n")
        .expect("합성 번역 실패");
    std::fs::write(content.join("notes.txt"), "md 가 아니다 sigma-decoy\n").expect("잡파일 실패");

    let guide = guide_text(dir);
    assert!(guide.contains("alpha-{ver}"), "원본 `.md` 를 안 읽었다");
    assert!(guide.contains("deep-marker"), "하위 디렉토리로 안 내려갔다");
    assert!(
        !guide.contains("translated-omega"),
        "번역(`en/`)이 원본에 섞였다 — 이 방향은 이 가드에 하한조차 없어 아무도 안 본다"
    );
    assert!(
        !guide.contains("sigma-decoy"),
        "`.md` 가 아닌 파일을 읽었다 — 같은 방향이다"
    );

    // ── 경계: 체크섬 가족은 모수 밖 ─────────────────────────────────────
    assert!(
        is_checksum("SHA256SUMS-probe.txt"),
        "체크섬 가족을 못 알아봤다"
    );
    assert!(
        !is_checksum("alpha-probe.tar.gz"),
        "설치 파일을 체크섬으로 셌다"
    );
}
