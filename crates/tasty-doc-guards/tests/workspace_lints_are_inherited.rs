//! 워크스페이스 멤버가 루트의 lint 정책을 상속하는지 확인한다.
//! 각 매니페스트에 lints.workspace=true가 필요하며 자체 lint 표가 있는 것만으로는 상속이 아니다.
//! 멤버 목록은 cargo metadata로 읽어 workspace exclude를 검사하지 않는다.
//! metadata를 실행하거나 읽을 수 없으면 검사 실패로 처리한다.

/// 2026-09-07 멤버52개를 측정한 뒤 정상적인 크레이트 정리를 허용할 여유를 둔 하한이다.
const MIN_MEMBERS: usize = 40;

#[derive(Debug, PartialEq, Eq)]
enum Inheritance {
    /// `[lints]` 아래 `workspace = true` — 상속한다.
    Inherits,
    /// `[lints]` 계열 절이 아예 없다.
    NoSection,
    /// 절은 있는데 상속이 아니다(자기 값을 직접 쓴다).
    OwnValues,
}

/// 주석을 제외하고 lints 절 안의 workspace=true를 찾는다.
fn inheritance_of(manifest: &str) -> Inheritance {
    let mut in_section = false;
    let mut saw_section = false;
    for line in manifest.lines() {
        if line.starts_with('[') {
            in_section = line.starts_with("[lints]") || line.starts_with("[lints.");
            if in_section {
                saw_section = true;
            }
            continue;
        }
        if in_section && line.split('#').next().is_some_and(inherits_workspace) {
            return Inheritance::Inherits;
        }
    }
    if saw_section {
        Inheritance::OwnValues
    } else {
        Inheritance::NoSection
    }
}

/// `workspace = true` 한 줄인가. 공백 폭을 안 따진다.
fn inherits_workspace(line: &str) -> bool {
    let mut parts = line.splitn(2, '=');
    let (Some(key), Some(value)) = (parts.next(), parts.next()) else {
        return false;
    };
    key.trim() == "workspace" && value.trim() == "true"
}

/// cargo metadata --no-deps --offline에서 멤버 매니페스트 경로를 읽는다.
/// 빌드나 네트워크 접근 없이 실행하며 JSON의 manifest_path 값을 추출한다.
fn member_manifests() -> Result<Vec<String>, String> {
    let cargo = std::env::var("CARGO").map_err(|_| {
        "CARGO 환경변수가 없어 멤버 목록을 조회할 수 없다. cargo test로 실행한다.".to_string()
    })?;
    let out = std::process::Command::new(&cargo)
        .args([
            "metadata",
            "--no-deps",
            "--format-version",
            "1",
            "--offline",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .map_err(|e| format!("cargo metadata 를 실행하지 못했다: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "cargo metadata 가 실패했다(rc {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let json = String::from_utf8_lossy(&out.stdout);
    Ok(manifest_paths_in(&json))
}

/// `cargo metadata` 산출에서 `"manifest_path":"..."` 값을 뽑는다.
fn manifest_paths_in(json: &str) -> Vec<String> {
    let mut out = Vec::new();
    for chunk in json.split("\"manifest_path\":").skip(1) {
        let Some(rest) = chunk.trim_start().strip_prefix('"') else {
            continue;
        };
        if let Some(end) = rest.find('"') {
            out.push(rest[..end].to_string());
        }
    }
    out.sort();
    out
}

#[test]
fn every_workspace_member_inherits_the_shared_lints() {
    let manifests = match member_manifests() {
        Ok(v) => v,
        Err(why) => panic!("워크스페이스 멤버 목록을 읽지 못해 검사할 수 없다: {why}"),
    };

    println!(
        "[워크스페이스 lint 상속] 멤버 {} · 하한 {MIN_MEMBERS}",
        manifests.len()
    );
    assert!(
        manifests.len() >= MIN_MEMBERS,
        "멤버를 {}개만 찾았다(하한 {MIN_MEMBERS}). metadata 출력과 실제 멤버 변경을 확인한다.",
        manifests.len()
    );

    let mut no_section = Vec::new();
    let mut own_values = Vec::new();
    for path in &manifests {
        let Ok(src) = std::fs::read_to_string(path) else {
            panic!("멤버 매니페스트를 못 읽었다: {path} — 못 읽은 것을 통과로 세지 않는다.");
        };
        match inheritance_of(&src) {
            Inheritance::Inherits => {}
            Inheritance::NoSection => no_section.push(path.clone()),
            Inheritance::OwnValues => own_values.push(path.clone()),
        }
    }

    assert!(
        no_section.is_empty() && own_values.is_empty(),
        "워크스페이스 lint를 상속하지 않는 멤버다.\n\nlints 절 없음({}):\n{}\n절은 있지만 상속하지 않음({}):\n{}\n각 Cargo.toml에 [lints]와 workspace = true를 설정한다. 자체 정책이 필요하다면 루트 정책 변경을 따르지 않는 이유를 검토하고 예외 근거를 기록한다.",
        no_section.len(),
        joined(&no_section),
        own_values.len(),
        joined(&own_values),
    );
}

fn joined(paths: &[String]) -> String {
    if paths.is_empty() {
        "  (없음)".to_string()
    } else {
        paths
            .iter()
            .map(|p| format!("  {p}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[test]
fn the_detector_separates_inheritance_from_a_section_that_merely_exists() {
    assert_eq!(
        inheritance_of("[package]\nname = \"x\"\n\n[lints]\nworkspace = true\n"),
        Inheritance::Inherits,
        "`[lints] workspace = true` 는 상속이다"
    );
    assert_eq!(
        inheritance_of("[package]\nname = \"x\"\n"),
        Inheritance::NoSection,
        "절이 없으면 상속이 아니다"
    );
    assert_eq!(
        inheritance_of("[lints.clippy]\ndead_code = \"deny\"\n"),
        Inheritance::OwnValues,
        "자기 값을 쓰는 절은 상속이 아니다 — 루트 정책이 바뀌어도 안 따라간다"
    );
    assert_eq!(
        inheritance_of("[lints]\nworkspace = false\n"),
        Inheritance::OwnValues,
        "`false` 는 상속이 아니다"
    );
    assert_eq!(
        inheritance_of("# [lints]\n# workspace = true\n[package]\nname = \"x\"\n"),
        Inheritance::NoSection,
        "주석 안의 `[lints]` 를 절로 세면 빠진 크레이트가 통과한다"
    );
    assert_eq!(
        inheritance_of("[lints]\nworkspace = true # 상속\n"),
        Inheritance::Inherits,
        "값 뒤 주석이 있어도 상속이다"
    );
    assert_eq!(
        inheritance_of("[lints]\n\n[dependencies]\nworkspace = true\n"),
        Inheritance::OwnValues,
        "절 경계를 안 지키면 엉뚱한 절의 값이 상속으로 읽힌다"
    );
}

#[test]
fn the_member_extractor_reads_manifest_paths() {
    let json = r#"{"packages":[{"name":"a","manifest_path":"/x/a/Cargo.toml"},
                                {"name":"b","manifest_path":"/x/b/Cargo.toml"}],
                   "workspace_root":"/x"}"#;
    assert_eq!(
        manifest_paths_in(json),
        vec!["/x/a/Cargo.toml".to_string(), "/x/b/Cargo.toml".to_string()],
        "패키지마다 한 번씩 나오는 manifest_path 를 다 뽑아야 한다"
    );
    assert!(
        manifest_paths_in("{}").is_empty(),
        "빈 metadata에서 멤버 경로를 만들어냈다"
    );
}
