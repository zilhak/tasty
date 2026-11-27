//! 이 크레이트의 normal·build 의존 폐포가 serde 수준에 머무는지 확인한다. 다른 client가 tokio·저장소·GUI 의존 없이
//! 이 타입을 쓰는 전제다. 모든 target의 의존을 함께 보므로 OS별 의존도 잡는다.

use std::collections::BTreeSet;
use std::process::Command;

/// 허용하는 패키지 이름. serde·serde_json과 그 derive 도구, plugin-protocol의 type 계층이다.
/// 의존이 늘어 실패하면 새 패키지가 데스크톱 의존을 끌어오지 않는지 확인한 뒤에만 더한다.
const ALLOWED: &[&str] = &[
    "bytemuck",
    "bytemuck_derive",
    "itoa",
    "memchr",
    "proc-macro2",
    "quote",
    "serde",
    "serde_core",
    "serde_derive",
    "serde_json",
    "syn",
    "tasty-attach-protocol",
    "tasty-plugin-protocol",
    "tasty-type-appearance",
    "tasty-type-geometry",
    "unicode-ident",
    "zmij",
];

#[test]
fn normal_and_build_dependencies_stay_within_the_allowed_set() {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let out = Command::new(cargo)
        .args([
            "tree",
            "-p",
            env!("CARGO_PKG_NAME"),
            "-e",
            "normal,build",
            "--target",
            "all",
            "--prefix",
            "none",
            "--format",
            "{p}",
            "--offline",
            "--locked",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree를 실행하지 못했다");
    assert!(
        out.status.success(),
        "cargo tree가 실패했다:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    let names: BTreeSet<&str> = text
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .collect();
    assert!(
        names.contains(env!("CARGO_PKG_NAME")) && names.contains("serde"),
        "cargo tree 출력에서 이 크레이트와 serde를 읽지 못했다:\n{text}"
    );
    let extra: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| !ALLOWED.contains(name))
        .collect();
    assert!(
        extra.is_empty(),
        "허용 목록 밖의 normal·build 의존이 생겼다: {extra:?}\n전체 목록: {names:?}"
    );
}
