//! builtin plugin 의 매니페스트(`tasty-plugin.toml`) `version` 이 같은 크레이트의
//! `Cargo.toml` `version` 과 정확히 일치하는지 검증한다. 불일치 시 fail.
//!
//! 배경: 매니페스트 version 은 `plugin.list` / 업그레이드 판정(`upgrade_builtins`)이
//! 노출·비교하는 값인데, 과거엔 Cargo.toml 만 patch 자동 +1 되고 매니페스트는 방치돼
//! 드리프트(예: markdown Cargo 0.1.11 vs manifest 0.1.1)가 쌓였다. 버전 정책이 이제
//! 둘의 lockstep 갱신을 요구하므로(§버전 정책 > Plugin), 이 테스트가 그 집행 채널이다.
//! `cargo test --workspace` 가 강제한다 — 그 잡은 수동 전용이라 자동 채널은 아니다
//! (`docs/dev-guide/ci-gates.md`).
//!
//! 참고: 매니페스트가 없는 라이브러리/인프라 크레이트(sdk, protocol, manifest 등)는
//! 번들 대상이 아니므로 스캔에서 자동 제외된다(파일 부재 = skip).

use std::path::Path;

/// `version = "..."` 최상위 필드의 따옴표 안 값을 추출한다.
/// `manifest_version` / `api_version` 처럼 접미 `version` 은 `^version = ` 앵커로 배제.
fn extract_version(contents: &str, file: &Path) -> String {
    for line in contents.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("version") {
            // `version` 뒤 첫 비공백이 `=` 여야 최상위 필드 (manifest_version 등 배제)
            let rest = rest.trim_start();
            if let Some(after_eq) = rest.strip_prefix('=') {
                let after_eq = after_eq.trim();
                let value = after_eq
                    .trim_start_matches('"')
                    .trim_end_matches('"')
                    .to_string();
                assert!(!value.is_empty(), "빈 version 필드: {}", file.display());
                return value;
            }
        }
    }
    panic!("version 필드를 찾지 못함: {}", file.display());
}

#[test]
fn manifest_version_matches_cargo_version() {
    let crates_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates");
    let mut checked = 0usize;
    let mut mismatches: Vec<String> = Vec::new();

    for entry in std::fs::read_dir(&crates_dir).expect("crates 디렉토리 read_dir 실패") {
        let entry = entry.expect("crates dir entry 읽기 실패");
        let dir = entry.path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if !name.starts_with("tasty-plugin-") {
            continue;
        }
        let manifest = dir.join("tasty-plugin.toml");
        if !manifest.exists() {
            // 번들 매니페스트 없는 라이브러리/인프라 크레이트 → 대상 아님.
            continue;
        }
        let cargo = dir.join("Cargo.toml");
        let manifest_v = extract_version(
            &std::fs::read_to_string(&manifest).expect("매니페스트 read 실패"),
            &manifest,
        );
        let cargo_v = extract_version(
            &std::fs::read_to_string(&cargo).expect("Cargo.toml read 실패"),
            &cargo,
        );
        checked += 1;
        if manifest_v != cargo_v {
            mismatches.push(format!(
                "  {name}: manifest={manifest_v} vs Cargo={cargo_v}"
            ));
        }
    }

    assert!(
        checked > 0,
        "스캔된 builtin plugin 이 0 개 — 경로/네이밍이 바뀌었는지 확인"
    );
    assert!(
        mismatches.is_empty(),
        "매니페스트 version 이 Cargo.toml 과 드리프트됨 (버전 정책 §Plugin: lockstep 갱신 + 재서명 필요):\n{}",
        mismatches.join("\n")
    );
}
