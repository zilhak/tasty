//! IPC 표의 모든 메서드는 **CLI 로 부를 수 있거나, 왜 못 부르는지의 근거를 갖는다.**
//!
//! 두 표면이 갈리는 것 자체가 결함이다 — `docs/identity.md` 원칙 2 는 에이전트 기능이
//! IPC 와 CLI **양면**으로 동작해야 한다고 못 박는다. 그런데 갈림은 조용히 생긴다:
//! IPC 핸들러를 하나 더 붙이는 것은 팔 한 줄이고, 그때 CLI 를 같이 손대지 않아도
//! 아무것도 빨개지지 않았다.
//!
//! ## 왜 면제 목록이 아니라 증거 표인가
//!
//! "CLI 로 못 부르는 것이 정답" 인 메서드가 실제로 있다(호출자가 plugin 이어야 성립하는
//! 것, 이미 다른 이름의 진입점이 있는 것). 그것들을 **이름만 적어 통과시키면** 그 줄은
//! 근거가 사라진 뒤에도 남는다 — 별칭이 사라져도, plugin 등급이 바뀌어도 초록이다.
//!
//! 그래서 각 행은 **그 판단을 지탱하는 데이터**를 함께 적고, 이 가드가 그 데이터를
//! 매번 다시 확인한다. 별칭 행은 대상 메서드가 실제로 CLI 로 닿을 때만 유효하고,
//! plugin-호출자 행은 표의 `plugin_callable` 이 참일 때만 유효하다. 근거가 죽으면
//! 행도 죽는다.
//!
//! ## 이 가드가 못 보는 것
//!
//! "CLI 로 닿는다" 의 판정은 **CLI 크레이트 소스에 그 메서드 이름 리터럴이 있는가** 다.
//! 로컬 실행 명령(`local/`)은 IPC 를 아예 안 타므로 리터럴이 없고, plugin 이 기여하는
//! 동적 명령(`tasty image …`)의 리터럴은 plugin 크레이트에 있다 — 둘 다 아래 표에
//! 그 사실을 근거로 적는다. 반대로, 디스패치가 아닌 자리(로그 문자열 등)에 이름이
//! 우연히 들어오면 이 가드는 그것을 진입점으로 오인한다. 그 방향의 오인은 행을
//! 지우게 만들 뿐 새 메서드를 놓치게 하지는 않는다.

use std::collections::BTreeSet;

use tasty_ipc::method_meta::{DEBUG_METHODS, METHOD_TABLE};

use super::repo_root;

const CLI_SRC: &str = "crates/tasty-cli/src";
const LOCAL_DIR: &str = "crates/tasty-cli/src/local";
const CRATES_DIR: &str = "crates";

/// CLI 소스에서 찾아낸 메서드 이름의 하한 — **연기 검사**다. 스캔이 죽으면 아래 대조는
/// "전부 CLI 에 없다" 가 되어 표와 어긋나 빨개지지만, 그 실패 문구는 원인을 가리키지
/// 못한다. 값의 근거: 2026-09-05 실측 286.
const MIN_CLI_LITERALS: usize = 200;

/// 왜 CLI 로 못 부르는지의 근거. 각 변형은 이 가드가 **다시 확인할 수 있는** 데이터를 든다.
#[derive(Clone, Copy)]
enum Why {
    /// CLI 잎이 IPC 를 타지 않고 그 자리에서 실행한다. 데이터: `local/` 의 파일 이름.
    LocalExec(&'static str),
    /// 진입점을 plugin 이 기여한다(`tasty image …`). 데이터: 그 plugin 크레이트 이름.
    PluginCli(&'static str),
    /// 호출자가 plugin 이어야 성립한다 — 응답이 호출자의 신원(자기 배너·자기 팝업·자기
    /// 설정)이나 호출자에게 push 되는 이벤트 수신처에 매여 있다. 셸에는 둘 다 없다.
    /// 데이터: 표의 `plugin_callable`.
    PluginCaller,
    /// 같은 일을 하는 진입점이 이미 다른 이름으로 있다. 데이터: 그 대상 메서드.
    AliasOf(&'static str),
    /// 사용자 행동이라 에이전트 표면에 두지 않는다(원칙 1·3). 데이터: release 표에 없다.
    UserAction,
}

/// CLI 진입점이 없는 메서드와 그 근거. **면제 목록이 아니다** — 아래 테스트들이 각
/// 행의 근거를 다시 확인하고, 근거가 죽은 행은 빨개진다.
const NO_CLI_ENTRY: &[(&str, Why)] = &[
    // 로컬 실행 — attach 와 원격 프로필/패스키는 CLI 가 그 자리에서 처리한다. 같은
    // 이름의 IPC 메서드는 원격/plugin 호출자를 위한 것이다.
    ("attach.acquire", Why::LocalExec("attach.rs")),
    ("attach.list", Why::LocalExec("attach.rs")),
    ("attach.release", Why::LocalExec("attach.rs")),
    ("remote.attach", Why::LocalExec("attach.rs")),
    ("remote.workspaces", Why::LocalExec("remote_workspaces.rs")),
    ("remote.passkey.add", Why::LocalExec("passkey.rs")),
    ("remote.passkey.get", Why::LocalExec("passkey.rs")),
    ("remote.passkey.list", Why::LocalExec("passkey.rs")),
    ("remote.passkey.remove", Why::LocalExec("passkey.rs")),
    ("remote.profile.add", Why::LocalExec("remote_profile.rs")),
    ("remote.profile.detect", Why::LocalExec("remote_profile.rs")),
    ("remote.profile.get", Why::LocalExec("remote_profile.rs")),
    ("remote.profile.import", Why::LocalExec("remote_profile.rs")),
    ("remote.profile.list", Why::LocalExec("remote_profile.rs")),
    (
        "remote.profile.list_local",
        Why::LocalExec("remote_profile.rs"),
    ),
    ("remote.profile.remove", Why::LocalExec("remote_profile.rs")),
    // plugin 이 기여하는 동적 CLI — 리터럴이 plugin 크레이트에 있다.
    ("image.export_png", Why::PluginCli("tasty-plugin-image")),
    ("image.list", Why::PluginCli("tasty-plugin-image")),
    ("image.next", Why::PluginCli("tasty-plugin-image")),
    ("image.open", Why::PluginCli("tasty-plugin-image")),
    ("image.paste", Why::PluginCli("tasty-plugin-image")),
    ("image.prev", Why::PluginCli("tasty-plugin-image")),
    ("image.save", Why::PluginCli("tasty-plugin-image")),
    ("markdown.navigate", Why::PluginCli("tasty-plugin-markdown")),
    // 호출자가 plugin 이어야 성립하는 것.
    ("banner.open", Why::PluginCaller),
    ("banner.close", Why::PluginCaller),
    ("popup.close", Why::PluginCaller),
    ("settings.get_plugin_setting", Why::PluginCaller),
    ("file_picker.trigger", Why::PluginCaller),
    ("git_viewer.query", Why::PluginCaller),
    ("host.shared_buffer.create", Why::PluginCaller),
    ("fs.pick_file", Why::PluginCaller),
    // 별칭 — 같은 일을 하는 진입점이 이미 있다.
    ("view.close", Why::AliasOf("window.close")),
    ("view.create", Why::AliasOf("window.create")),
    ("view.list", Why::AliasOf("window.list")),
    ("surface.send_to", Why::AliasOf("surface.send")),
    ("surface.send_combo", Why::AliasOf("surface.send_key")),
    // 사용자 행동.
    ("system.shutdown", Why::UserAction),
    ("window.focus", Why::UserAction),
    ("view.focus", Why::UserAction),
];

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} 을 읽지 못했다: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// 디렉터리 아래 모든 `.rs` 에서 메서드 이름 모양의 문자열 리터럴을 모은다.
fn method_literals(dir: &std::path::Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("디렉터리 항목을 읽을 수 없다").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                collect_literals(&read(&path), &mut out);
            }
        }
    }
    out
}

/// `"ns.method"` 모양의 리터럴만 뽑는다 — 소문자·숫자·`_`·`.` 로만 이뤄진 것.
fn collect_literals(src: &str, out: &mut BTreeSet<String>) {
    let mut rest = src;
    while let Some(open) = rest.find('"') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('"') else { break };
        let name = &after[..close];
        if !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
        {
            out.insert(name.to_string());
        }
        rest = &after[close + 1..];
    }
}

/// 표 둘을 합친 메서드 전체.
fn universe() -> BTreeSet<String> {
    METHOD_TABLE
        .iter()
        .chain(DEBUG_METHODS.iter())
        .map(|(m, _)| (*m).to_string())
        .collect()
}

/// 모든 메서드는 CLI 로 닿거나 근거 행을 갖는다 — **양방향**으로 본다.
#[test]
fn every_ipc_method_is_reachable_from_the_cli_or_carries_a_reason() {
    let cli = method_literals(&repo_root().join(CLI_SRC));
    assert!(
        cli.len() >= MIN_CLI_LITERALS,
        "CLI 소스에서 이름 리터럴을 {} 개밖에 못 찾았다(하한 {MIN_CLI_LITERALS}, \
         2026-09-05 실측 286) — 스캔이 죽었다. 이 상태로는 아래 대조가 \
         '전부 CLI 에 없다' 가 되어 원인을 못 가리킨다",
        cli.len()
    );

    let uni = universe();
    assert!(
        uni.len() > 200,
        "IPC 표가 {} 건뿐이다 — 표를 못 읽었다",
        uni.len()
    );
    let documented: BTreeSet<&str> = NO_CLI_ENTRY.iter().map(|(m, _)| *m).collect();

    let unreachable: Vec<&str> = uni
        .iter()
        .filter(|m| !cli.contains(*m))
        .map(String::as_str)
        .filter(|m| !documented.contains(m))
        .collect();
    assert!(
        unreachable.is_empty(),
        "IPC 표에는 있는데 CLI 로 부를 수 없고 사유도 없는 메서드가 있다. 두 표면이 \
         갈리면 에이전트는 IPC 를 직접 쏴야만 그 기능에 닿는다(`docs/identity.md` 원칙 2). \
         CLI 진입점을 만들거나, 못 만드는 이유를 `NO_CLI_ENTRY` 에 **근거와 함께** \
         적어라(이름만 적는 면제는 받지 않는다):\n  {}",
        unreachable.join("\n  ")
    );

    let stale: Vec<&str> = documented
        .iter()
        .filter(|m| cli.contains(**m))
        .copied()
        .collect();
    assert!(
        stale.is_empty(),
        "`NO_CLI_ENTRY` 가 CLI 진입점이 **생긴** 메서드를 아직 들고 있다. 근거가 사라진 \
         행은 지운다 — 남겨 두면 다음에 진입점이 없어져도 초록이다:\n  {}",
        stale.join("\n  ")
    );

    let unknown: Vec<&str> = documented
        .iter()
        .filter(|m| !uni.contains(**m))
        .copied()
        .collect();
    assert!(
        unknown.is_empty(),
        "`NO_CLI_ENTRY` 가 표에 없는 이름을 들고 있다 — 메서드가 사라졌거나 오타다:\n  {}",
        unknown.join("\n  ")
    );
}

/// 각 행의 **근거**가 지금도 성립한다.
#[test]
fn each_reason_still_has_its_evidence() {
    let root = repo_root();
    let cli = method_literals(&root.join(CLI_SRC));
    let uni = universe();
    let mut dead: Vec<String> = Vec::new();

    for (method, why) in NO_CLI_ENTRY {
        match why {
            Why::LocalExec(file) => {
                if !root.join(LOCAL_DIR).join(file).is_file() {
                    dead.push(format!(
                        "{method} — 로컬 실행 근거로 든 `{LOCAL_DIR}/{file}` 이 없다"
                    ));
                }
            }
            Why::PluginCli(krate) => {
                let dir = root.join(CRATES_DIR).join(krate).join("src");
                if !dir.is_dir() {
                    dead.push(format!("{method} — plugin 크레이트 `{krate}` 가 없다"));
                } else if !method_literals(&dir).contains(*method) {
                    dead.push(format!(
                        "{method} — `{krate}` 소스에 그 이름이 없다. plugin 이 그 명령을 \
                         더 이상 기여하지 않으면 이 메서드는 아무 데서도 못 부른다"
                    ));
                }
            }
            Why::PluginCaller => {
                let callable = METHOD_TABLE
                    .iter()
                    .chain(DEBUG_METHODS.iter())
                    .find(|(m, _)| m == method)
                    .is_some_and(|(_, meta)| meta.plugin_callable);
                if !callable {
                    dead.push(format!(
                        "{method} — plugin 호출자를 근거로 들었는데 표의 \
                         `plugin_callable` 이 false 다. plugin 도 셸도 못 부르면 \
                         그 메서드에는 호출자가 없다"
                    ));
                }
            }
            Why::AliasOf(target) => {
                if !uni.contains(*target) {
                    dead.push(format!("{method} — 별칭 대상 `{target}` 이 표에 없다"));
                } else if !cli.contains(*target) {
                    dead.push(format!(
                        "{method} — 별칭 대상 `{target}` 도 CLI 로 못 부른다. \
                         '이미 다른 이름으로 있다' 가 성립하지 않는다"
                    ));
                }
            }
            Why::UserAction => {
                let in_release = METHOD_TABLE.iter().any(|(m, _)| m == method);
                if in_release {
                    dead.push(format!(
                        "{method} — 사용자 행동이라 안 연다고 했는데 release 표에 있다. \
                         release IPC 로는 열려 있으면서 CLI 만 막은 셈이다"
                    ));
                }
            }
        }
    }

    assert!(
        dead.is_empty(),
        "`NO_CLI_ENTRY` 의 행이 근거를 잃었다. 근거가 죽은 행은 통과의 이유가 아니라 \
         다시 판단할 신호다:\n  {}",
        dead.join("\n  ")
    );
}

/// 판정기가 실제로 무엇을 보는지 못 박는다 — 대조군이 죽으면 위 둘은 조용히 통과한다.
mod evidence_mutations {
    use super::*;

    #[test]
    fn a_method_name_literal_is_seen_and_other_strings_are_not() {
        let mut out = BTreeSet::new();
        collect_literals(
            r#"("surface.send", meta()), let msg = "Surface not found"; let k = "ns.a_b1";"#,
            &mut out,
        );
        assert!(
            out.contains("surface.send"),
            "이름 리터럴을 놓쳤다: {out:?}"
        );
        assert!(
            out.contains("ns.a_b1"),
            "숫자·밑줄이 든 이름을 놓쳤다: {out:?}"
        );
        assert!(
            !out.contains("Surface not found"),
            "이름 모양이 아닌 문자열을 집었다: {out:?}"
        );
    }

    /// 별칭 근거는 **대상이 CLI 로 닿을 때만** 유효하다.
    #[test]
    fn an_alias_reason_needs_its_target_to_be_reachable() {
        let cli = method_literals(&repo_root().join(CLI_SRC));
        for (method, why) in NO_CLI_ENTRY {
            if let Why::AliasOf(target) = why {
                assert!(
                    cli.contains(*target),
                    "{method} 의 별칭 대상 {target} 이 CLI 에 없다"
                );
                assert!(
                    !cli.contains(*method),
                    "{method} 자신이 CLI 에 있다 — 별칭 행이 아니라 진입점이 있는 것이다"
                );
            }
        }
    }

    /// 표를 못 읽으면 `universe()` 가 비어 모든 대조가 공허하게 참이 된다.
    #[test]
    fn the_universe_is_not_empty() {
        assert!(universe().len() > 200, "IPC 표를 못 읽었다");
        assert!(
            !DEBUG_METHODS.is_empty(),
            "debug 표가 비었다 — 이 테스트는 debug 빌드에서만 의미가 있다"
        );
    }
}
