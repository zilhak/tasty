//! hook 명령이 **실패를 기록하는 디스패치**로만 발화하도록 막는 매니페스트 가드.
//!
//! **무엇을 지키는가.** `hook-failures.log` 기록은 plugin CLI 의 단발 디스패치
//! 한 곳에만 걸려 있다(`tasty_cli::run::run_dynamic_client`). 같은 파일의 다른 두
//! 디스패치 — `polling` 을 선언한 명령이 타는 반복 호출과 `auto_wait` 를 선언한
//! 명령이 타는 chain 호출 — 은 기록하지 않는다. 지금은 그래도 되는데, **hook 명령이
//! 그 둘 중 어느 것도 선언하지 않기 때문**이다.
//!
//! 그 전제는 소스가 아니라 **매니페스트**에 있다. `polling` / `auto_wait` 는
//! `contributes.cli.subcommands` 의 필드라, plugin 이 hook 서브커맨드에 그 줄을
//! 한 줄 얹는 순간 그 hook 의 실패는 로그에서 **조용히 사라진다** — 빌드도 테스트도
//! 초록이고, 셸 래퍼가 `|| true` 로 exit code 를 버리므로 호출한 쪽에도 흔적이 없다.
//! 그러니까 이 전제는 산문으로 적어 둘 게 아니라 재는 자리가 있어야 한다.
//!
//! **이름이 아니라 성질로 묻는다.** 대상 method 목록을 여기 베껴 적으면 다음에
//! 추가되는 hook 이 같은 이유로 빠진다. 판정은 프로덕션이 기록 대상을 고를 때 쓰는
//! 바로 그 술어 [`tasty_cli::hook_failure::is_hook_method`] 를 **그대로 불러서** 한다 —
//! 기록 범위가 넓어지거나 좁아지면 이 가드의 범위도 같이 움직인다.
//!
//! 고치는 방법은 둘 중 하나다: 그 hook 에서 `polling`/`auto_wait` 를 빼거나, 두
//! 디스패치에도 기록을 걸고 이 가드를 그에 맞게 고치거나. 뒤쪽을 고를 때 주의할
//! 점은 [`hook_failure_reason_stays_english`] 가 지키는 규약이다 — 그 두 경로는
//! 호출자에게 **번역된** 오류를 돌려주므로, 최종 오류만 보는 래퍼로 합치면 영어
//! 원본을 잃는다.
//!
//! [`hook_failure_reason_stays_english`]: ./hook_failure_reason_stays_english.rs

use std::path::{Path, PathBuf};

use tasty_cli::hook_failure::is_hook_method;
use tasty_plugin_manifest::{CliSubcommandDecl, Manifest};

/// 서브커맨드가 기록 없는 디스패치를 타는가. 탄다면 그 사유(선언한 필드 이름).
fn dispatch_without_recording(sub: &CliSubcommandDecl) -> Option<&'static str> {
    if sub.polling.is_some() {
        return Some("polling");
    }
    if sub.auto_wait.is_some() {
        return Some("auto_wait");
    }
    None
}

/// 번들 매니페스트 경로. 디렉토리 목록에서 직접 모은다 — 목록을 소스에 적으면
/// 새 plugin 이 조용히 스캔 밖으로 빠진다.
fn manifest_paths() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("crates");
    assert!(crates.is_dir(), "crates/ 가 사라졌다: {}", crates.display());
    let mut paths = Vec::new();
    for entry in std::fs::read_dir(&crates)
        .expect("crates/ 를 열 수 없다")
        .flatten()
    {
        let manifest = entry.path().join("tasty-plugin.toml");
        if manifest.is_file() {
            paths.push(manifest);
        }
    }
    paths.sort();
    paths
}

/// 갈래마다 따로 센 수. 하나로 합치면 한쪽이 죽어도 다른 쪽이 수를 채워 초록이 된다.
#[derive(Default)]
struct Census {
    manifests: usize,
    subcommands: usize,
    hook_subcommands: usize,
}

#[test]
fn no_hook_command_declares_a_dispatch_that_skips_recording() {
    let paths = manifest_paths();
    let mut census = Census::default();
    let mut violations = Vec::new();

    for path in &paths {
        let text = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("매니페스트를 읽을 수 없다 {}: {e}", path.display()));
        let manifest: Manifest = toml::from_str(&text)
            .unwrap_or_else(|e| panic!("매니페스트 파싱 실패 {}: {e}", path.display()));
        census.manifests += 1;
        for command in &manifest.contributes.cli {
            for sub in &command.subcommands {
                census.subcommands += 1;
                if !is_hook_method(&sub.ipc_method) {
                    continue;
                }
                census.hook_subcommands += 1;
                if let Some(field) = dispatch_without_recording(sub) {
                    violations.push(format!(
                        "{}  {} {} (ipc_method={}) 가 `{field}` 를 선언했다",
                        path.display(),
                        command.name,
                        sub.name,
                        sub.ipc_method
                    ));
                }
            }
        }
    }

    // 자기-공허 검사를 갈래마다 센다. 어느 한 갈래가 0 이면 이 초록은 "위반이 없다"
    // 가 아니라 "아무것도 안 봤다" 는 뜻이다.
    assert_eq!(
        census.manifests,
        paths.len(),
        "매니페스트를 {} 개 찾아 {} 개만 읽었다",
        paths.len(),
        census.manifests
    );
    assert!(
        census.manifests > 0,
        "번들 매니페스트를 하나도 못 찾았다 — 스캔 루트가 옮겨졌는지 확인해라"
    );
    assert!(
        census.subcommands > 0,
        "서브커맨드를 하나도 못 찾았다 — 매니페스트 스키마의 cli 경로가 바뀌었는지 확인해라"
    );
    assert!(
        census.hook_subcommands > 0,
        "hook 모양 서브커맨드를 하나도 못 찾았다({} 개 중) — 기록 대상 술어나 명명 관례가 \
         바뀌었으면 이 가드도 함께 옮겨라",
        census.subcommands
    );

    assert!(
        violations.is_empty(),
        "hook 명령의 실패는 hook-failures.log 가 유일한 흔적인데, `polling`/`auto_wait` 를 \
         선언한 명령은 기록을 걸지 않은 디스패치를 탄다 — 그 hook 의 실패는 아무 데도 안 \
         남는다:\n{}",
        violations.join("\n")
    );
}

/// 위 판정이 실제로 발화하는지 — 실물 매니페스트가 전부 결백할 때 초록이 나오는
/// 것과, 위반이 있을 때 빨강이 나오는 것은 다른 사건이다.
#[test]
fn a_hook_subcommand_that_declares_polling_is_a_violation() {
    let offender: CliSubcommandDecl = toml::from_str(
        r#"
        name = "hook"
        ipc_method = "claude.hook"
        args = "hook_args"
        polling = { state_field = "state", terminal_states = ["done"] }
        "#,
    )
    .expect("픽스처 파싱");
    assert!(is_hook_method(&offender.ipc_method));
    assert_eq!(dispatch_without_recording(&offender), Some("polling"));

    let chained: CliSubcommandDecl = toml::from_str(
        r#"
        name = "checklist-hook"
        ipc_method = "claude.checklist_hook"
        args = "hook_args"
        auto_wait = { method = "claude.wait", polling = { state_field = "state", terminal_states = ["done"] } }
        "#,
    )
    .expect("픽스처 파싱");
    assert!(is_hook_method(&chained.ipc_method));
    assert_eq!(dispatch_without_recording(&chained), Some("auto_wait"));
}

/// 반대 방향 — 오탐이 없는지. 기록을 타는 hook 도, 기록을 안 타지만 hook 이 아닌
/// 명령도 위반이 아니다. 뒤쪽이 중요하다: `spawn`/`tell` 이 chain 을 걸어도 그
/// 실패는 사용자가 stderr 로 보므로 이 가드의 대상이 아니다.
#[test]
fn a_plain_hook_and_a_blocking_non_hook_are_both_fine() {
    let plain: CliSubcommandDecl = toml::from_str(
        r#"
        name = "hook"
        ipc_method = "codex.hook"
        args = "hook_args"
        "#,
    )
    .expect("픽스처 파싱");
    assert!(is_hook_method(&plain.ipc_method));
    assert_eq!(dispatch_without_recording(&plain), None);

    let spawn: CliSubcommandDecl = toml::from_str(
        r#"
        name = "spawn"
        ipc_method = "claude.spawn"
        args = "spawn_args"
        polling = { state_field = "state", terminal_states = ["idle"] }
        "#,
    )
    .expect("픽스처 파싱");
    assert!(!is_hook_method(&spawn.ipc_method));
    assert!(dispatch_without_recording(&spawn).is_some());
}
