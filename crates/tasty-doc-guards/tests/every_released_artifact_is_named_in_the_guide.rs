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
fn guide_text(root: &Path) -> String {
    fn walk(dir: &Path, out: &mut String) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().map(|n| n == "en").unwrap_or(false) {
                    continue; // 번역은 별도 절차다.
                }
                walk(&path, out);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                out.push_str(&text);
                out.push('\n');
            }
        }
    }
    let mut out = String::new();
    walk(&root.join("site/content"), &mut out);
    out
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
