//! release.yml의 업로드 명령에서 읽은 설치 파일명이 한국어 사용자 가이드에 나오는지 확인한다.
//! 버전 자리는 {ver}로 맞춰 비교한다. 각 업로드 줄에서 마지막 dist/ 경로만 읽으며 셸 실행은 분석하지 않는다.
//! 이름이 한 번 나오면 기재된 것으로 본다. 설명의 품질, 설치·제거 절차의 정확성, 영어 번역은 검사하지 않는다.
//! doc-guards.yml의 경로 필터 없는 main push·PR에서 실행된다.

use std::path::{Path, PathBuf};
use tasty_doc_guards::temp_scratch::Scratch;

/// 가이드가 여러 설치 파일을 한 행으로 묶어 설명한 경우를 기록한다.
/// 파일이 새로 추가됐다는 이유만으로 면제하지 않고 실제로 묶어 설명한 행이 있어야 한다.
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

/// 2026-09-06 실측15개에 여유를 둔 하한8. 수집 누락을 찾는 보조 검사다.
const MIN_ARTIFACTS: usize = 8;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// 체크섬은 설치 파일 선택지가 아니므로 개별 이름 대신 SHA256SUMS 묶음의 안내가 있는지 따로 확인한다.
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

/// 한국어 가이드의 최상위 분류. 수집 누락을 찾도록 순회 결과와 독립된 목록으로 둔다.
const GUIDE_BRANCHES: &[&str] = &[
    "agents",
    "customize",
    "getting-started",
    "help",
    "plugins",
    "remote",
    "using",
];

/// 가이드 본문을 수집한다. 최상위 분류별 수집 여부는 별도 검사에서 확인한다.
fn guide_text(root: &Path) -> String {
    guide_scan(root).0
}

/// 합성 트리에도 같은 순회를 쓰도록 수집과 실제 분류 목록의 대조를 분리한다.
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
                "가이드 디렉터리를 읽지 못했다: {} — {e}. 수집 누락 상태로 설명 여부를 판단하지 않도록 실패시킨다.",
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

/// 분류 추가와 기존 분류의 수집 누락을 양방향으로 확인한다.
#[test]
fn the_guide_walk_reaches_every_branch() {
    let (_, branches, touched) = guide_scan(&repo_root());
    let missing: Vec<&&str> = GUIDE_BRANCHES
        .iter()
        .filter(|b| !touched.contains(**b))
        .collect();
    assert!(
        missing.is_empty(),
        "다음 가이드 분류에서 Markdown을 수집하지 못했다: {missing:?}. 등록 {}개 중 {}개를 읽었다. 경로 제외와 디렉터리 상태를 확인한다.",
        GUIDE_BRANCHES.len(),
        touched.len()
    );
    let extra: Vec<&String> = branches
        .iter()
        .filter(|b| !GUIDE_BRANCHES.contains(&b.as_str()))
        .collect();
    assert!(
        extra.is_empty(),
        "site/content에 미등록 분류가 있다: {extra:?}. GUIDE_BRANCHES에 추가해 수집 누락 검사에 포함한다."
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
        "릴리스 산출물을 {}개만 읽었다(하한 {MIN_ARTIFACTS}). 실제 업로드 목록 감소와 released_artifacts의 판독 실패를 구별한다.",
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
        "사용자 가이드에 설치 파일명이 없다:\n  {}\n설치 파일 표에 이름을 추가한다. 이미 여러 파일을 한 행으로 설명했다면 해당 행과 사유를 COMPRESSED_ROWS에 등록한다. 새 파일 하나의 추가는 묶음 예외의 이유가 아니다.",
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
        "가이드에 전체 이름을 쓴 파일이 묶음 예외에 남았다: {stale:?}. 해당 등록을 제거한다."
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
        "더 이상 업로드하지 않는 파일이 묶음 예외에 남았다: {dead:?}. 등록과 가이드 표를 함께 정리한다."
    );
}

#[test]
fn the_checksum_family_is_classified_once_by_a_glob() {
    let root = repo_root();
    let artifacts = released_artifacts(&root);
    let checksums: Vec<&String> = artifacts.iter().filter(|a| is_checksum(a)).collect();
    assert!(
        !checksums.is_empty(),
        "업로드 목록에 체크섬 파일이 없다. is_checksum의 분류와 검사 범위를 재검토한다."
    );
    let guide = guide_text(&root);
    assert!(
        guide.contains("SHA256SUMS"),
        "개별 이름 검사를 제외한 체크섬 {}개에 대한 SHA256SUMS 안내가 가이드에 없다.",
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

/// 번역이나 비 Markdown 문서의 혼입은 본문을 늘려 누락 검사를 통과시킬 수 있으므로 합성 입력에서 제외 여부를 확인한다.
#[test]
fn both_readers_answer_on_a_substituted_tree() {
    let probe = Scratch::new("released-artifact-reader");
    let dir = probe.path();

    let wf = dir.join(".github/workflows");
    std::fs::create_dir_all(&wf).expect("합성 워크플로 트리를 만들지 못했다");
    std::fs::write(
        wf.join("release.yml"),
        concat!(
            "        run: gh release upload \"$TAG\" \"dist/alpha-${VERSION}-probe.tar.gz\"\n",
            "        run: gh release upload \"$TAG\" \"dist/zeta-widget.msix\"\n",
            "        run: echo \"dist/never-uploaded.zip\"\n",
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
        "영어 번역이 한국어 가이드 수집에 섞였다"
    );
    assert!(
        !guide.contains("sigma-decoy"),
        "`.md` 가 아닌 파일을 읽었다 — 같은 방향이다"
    );

    assert!(
        is_checksum("SHA256SUMS-probe.txt"),
        "체크섬 가족을 못 알아봤다"
    );
    assert!(
        !is_checksum("alpha-probe.tar.gz"),
        "설치 파일을 체크섬으로 셌다"
    );
}
