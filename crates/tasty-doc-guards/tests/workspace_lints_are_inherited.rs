//! 워크스페이스 멤버가 전부 공통 lint 를 **상속하는지** 본다.
//!
//! 루트 `Cargo.toml` 의 `[workspace.lints]` 는 이 레포의 lint 정책 한 벌이다
//! (`dead_code = "deny"` · `cognitive_complexity = "deny"` · `disallowed_methods = "deny"` 등).
//! 그런데 그 정책은 **자동으로 내려가지 않는다** — 멤버마다 `[lints] workspace = true`
//! 한 줄을 적어야 상속된다. 그 한 줄이 빠지면 그 크레이트만 정책 밖에 서고,
//! **컴파일도 테스트도 초록이다.** 빠졌다는 사실이 값으로 안 남는다.
//!
//! 실측 2026-09-07: `crates/tasty-doc-guards` 가 정확히 그 상태였다 — 가드를 담은
//! 크레이트가 `dead_code = "deny"` 를 안 받고 있었고, 그 사실은 어디에도 안 적혀
//! 있었다. 켜 보니 새 진단이 한 줄도 안 늘었다(비용 0). 즉 **막고 있던 것이 아니라
//! 그냥 빠져 있었다.** 이 가드는 그 형태가 조용히 다시 생기는 것을 막는다.
//!
//! ── 물음이 둘이라 술어도 둘이다 ─────────────────────────────────────────
//! "`[lints]` 절이 있나" 와 "**상속하나**" 는 다른 물음이다. 절만 보면
//! `[lints.clippy]` 아래에 자기 값을 직접 쓴 크레이트가 통과하는데, 그것은 상속이
//! 아니라 **다른 답**이다 — 루트 정책이 바뀌어도 그 크레이트는 안 따라간다.
//! 그래서 이 가드는 절 유무가 아니라 **`workspace = true` 여부**로 판정하고,
//! 실패문에서 두 갈래를 갈라 적는다(아예 없음 / 있는데 상속이 아님).
//!
//! 자기 값을 쓰는 크레이트가 나중에 정당한 이유로 생기면 **그때 면제할지가 판단**이다.
//! 이 문단이 그 판단의 근거다: 면제한다는 것은 "루트 정책이 바뀌어도 이 크레이트는
//! 안 따라간다" 를 받아들인다는 뜻이고, 그 대가를 아는 사람만 면제할 수 있다.
//!
//! ── 모수는 `cargo metadata` 의 멤버다. 디렉토리가 아니다 ─────────────────
//! `crates/*/` 를 훑으면 **루트 `exclude` 에 든 크레이트가 위반으로 잡힌다.**
//! `crates/tasty-plugin-sdk-wasm` 이 그 경우이고(자체 `Cargo.lock` 을 쓰는 POC 격리),
//! `site/` 도 같다. 둘 다 `[lints]` 가 없지만 `--workspace` 가 애초에 안 보므로
//! 상속할 대상이 아니다 — 그것들을 잡으면 **가드가 옳은 상태를 막는다.**
//! 좁히는 오류보다 넓히는 오류가 위험한 자리라, 모수를 cargo 에게 묻는다.
//!
//! ── 물을 수 없으면 통과가 아니라 실패다 ──────────────────────────────────
//! `cargo metadata` 가 실패하면 이 가드는 **판정 불가로 죽는다.** 모수를 못 얻은
//! 상태의 "위반 0" 은 "없다" 가 아니라 "안 봤다" 이고, 그 둘을 같은 칸에 쓰면
//! 채널은 도는데 아무것도 안 보는 상태가 초록으로 보인다. 셸 게이트들이 판정기
//! 부재를 다루는 방식과 같다(`scripts/check-file-size.sh` 의 exit 2).

/// 멤버 수의 하한 — 모수가 비면 "전부 상속한다" 는 언제나 참이다.
///
/// 값의 근거: 2026-09-07 실측 **52**(`cargo metadata --no-deps` 의 패키지 수).
/// 하한을 실측보다 낮게 잡는 것은 크레이트가 정당하게 줄어들 수 있기 때문이고,
/// 절반 아래로 떨어지면 그것은 감소가 아니라 **수집이 죽은 것**이다.
///
/// **이 수를 내려서 초록을 만들지 마라.** 아래 판정은 이 모수를 순회하므로,
/// 수집이 절반 죽으면 절반만 검사하면서 초록이 된다.
const MIN_MEMBERS: usize = 40;

/// 한 멤버의 `Cargo.toml` 이 워크스페이스 lint 를 어떻게 다루는가.
#[derive(Debug, PartialEq, Eq)]
enum Inheritance {
    /// `[lints]` 아래 `workspace = true` — 상속한다.
    Inherits,
    /// `[lints]` 계열 절이 아예 없다.
    NoSection,
    /// 절은 있는데 상속이 아니다(자기 값을 직접 쓴다).
    OwnValues,
}

/// `Cargo.toml` 원문에서 상속 여부를 읽는다.
///
/// TOML 주석은 `#` 로 시작하므로 절 머리를 **줄 맨 앞**에서만 찾는다 — 주석 안의
/// `[lints]` 언급을 절로 세지 않기 위해서다(이 레포는 규칙 본문을 주석에 적는다).
/// 절의 범위는 다음 절 머리까지다.
fn inheritance_of(manifest: &str) -> Inheritance {
    let mut in_section = false;
    let mut saw_section = false;
    for line in manifest.lines() {
        if line.starts_with('[') {
            // `[lints]` 는 상속을 쓸 수 있는 절이고, `[lints.clippy]` 같은 하위 절은
            // 자기 값을 쓰는 형태다. 둘 다 "절은 있다" 로 센다.
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

/// 워크스페이스 멤버의 `Cargo.toml` 경로 전부 — cargo 에게 묻는다.
///
/// `--no-deps` 라 의존 해석을 하지 않고 멤버 매니페스트만 읽으므로, `cargo test`
/// 안에서 불려도 빌드 디렉토리 잠금을 기다리지 않는다(실측 2026-09-07: 23ms).
/// `--offline` 은 네트워크가 없는 러너에서 이 가드가 대기하지 않게 한다.
///
/// JSON 을 파싱하지 않고 `"manifest_path"` 값만 뽑는다 — 이 크레이트는 의존이 0 인
/// 것이 존재 이유라(ADR-0138) serde 를 들일 수 없다. `--no-deps` 에서 그 키는
/// 패키지마다 정확히 한 번 나오므로 출현 수가 곧 멤버 수다.
fn member_manifests() -> Result<Vec<String>, String> {
    let cargo = std::env::var("CARGO").map_err(|_| {
        "CARGO 환경변수가 없다 — cargo 가 띄운 시험이 아니다. 모수를 못 얻었으므로 판정하지 않는다."
            .to_string()
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
        Err(why) => panic!(
            "모수를 못 얻었다 — {why}\n\n\
             이 상태의 '위반 0' 은 '없다' 가 아니라 '안 봤다' 다. 통과로 읽지 않는다."
        ),
    };

    println!(
        "[워크스페이스 lint 상속] 멤버 {} · 하한 {MIN_MEMBERS}",
        manifests.len()
    );
    assert!(
        manifests.len() >= MIN_MEMBERS,
        "워크스페이스 멤버를 {} 개만 찾았다(하한 {MIN_MEMBERS}) — 수집이 죽으면 \
         아래 판정은 빈 집합을 훑고 조용히 통과한다.",
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
        "워크스페이스 공통 lint 를 상속하지 않는 멤버가 있다.\n\n\
         `[lints]` 절이 아예 없다({}):\n{}\n\
         절은 있는데 상속이 아니다({}) — 자기 값을 직접 쓴다:\n{}\n\n\
         루트 `Cargo.toml` 의 `[workspace.lints]` 는 자동으로 안 내려간다. 그 멤버의 \
         `Cargo.toml` 에 다음 두 줄을 더해라:\n\
         \n    [lints]\n    workspace = true\n\n\
         자기 값을 쓰는 것이 의도라면 그것은 '루트 정책이 바뀌어도 이 크레이트는 안 \
         따라간다' 를 받아들이는 것이다 — 이 파일의 머리 주석을 읽고, 면제가 필요하면 \
         여기에 그 근거와 함께 예외를 적어라. 조용히 빠지는 것과 알고 빼는 것은 다르다.",
        no_section.len(),
        joined(&no_section),
        own_values.len(),
        joined(&own_values),
    );
}

/// 목록을 실패문에 넣는 모양으로 만든다. 비면 그 자리가 비어 보이지 않게 표시한다.
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

/// 위 판정이 **무엇이든 가를 수 있는지** 를 같은 함수로 확인한다.
/// 한 방향만 재면 "위반 0" 과 "판정이 죽었다" 가 구별되지 않는다.
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
    // 이 갈래가 이 가드의 존재 이유다 — 절만 세는 술어는 여기서 통과한다.
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
    // 주석 안의 언급은 절이 아니다 — 이 레포는 규칙 본문을 주석에 적는다.
    assert_eq!(
        inheritance_of("# [lints]\n# workspace = true\n[package]\nname = \"x\"\n"),
        Inheritance::NoSection,
        "주석 안의 `[lints]` 를 절로 세면 빠진 크레이트가 통과한다"
    );
    // 값 뒤 주석은 값을 가리지 않는다.
    assert_eq!(
        inheritance_of("[lints]\nworkspace = true # 상속\n"),
        Inheritance::Inherits,
        "값 뒤 주석이 있어도 상속이다"
    );
    // 다음 절로 넘어간 뒤의 `workspace = true` 는 lint 상속이 아니다.
    assert_eq!(
        inheritance_of("[lints]\n\n[dependencies]\nworkspace = true\n"),
        Inheritance::OwnValues,
        "절 경계를 안 지키면 엉뚱한 절의 값이 상속으로 읽힌다"
    );
}

/// 모수 추출이 실제로 동작하는지 — `cargo metadata` 산출 모양에 대한 양성 대조.
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
        "없는 것을 있다고 하면 하한 검사가 무뎌진다"
    );
}
