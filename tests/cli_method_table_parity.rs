//! CLI 가 IPC 로 **보내는** 메서드 이름이 전부 권한 표에 실재하는지 검증한다.
//!
//! `crates/tasty-cli` 는 커맨드를 `JsonRpcRequest` 로 조립하면서 메서드 이름을 소스의
//! 문자열 리터럴로 박는다. 그 문자열이 `METHOD_TABLE` / `DEBUG_METHODS` / `PREFIX_RULES`
//! 어디에도 없으면 호스트는 `UnknownMethod` 로 거부한다 — 그리고 그것은 **런타임에만**
//! 드러난다. 오타 한 글자가 컴파일도 테스트도 통과해 사용자에게 도달한다.
//!
//! **같은 축의 다른 가드가 이 방향을 안 본다** (겹치지 않는다):
//!
//! | 가드 | 보는 방향 | 이 오타를 잡나 |
//! |---|---|---|
//! | `cli_naming_count_drift` | 표의 네임스페이스별 **개수** | 못 잡는다 — CLI 는 입력이 아니고, 이름이 틀려도 개수는 안 변한다 |
//! | `ipc_router_table_parity` | **라우터 팔** → 표 | 못 잡는다 — CLI 는 라우터의 앞단이라 스캔 대상 밖이다 |
//! | `permission_free_methods_docs_parity` | 표 → **문서** | 못 잡는다 — 표에 없는 이름은 문서에도 없으니 대조에 안 걸린다 |
//! | **이 가드** | **CLI 문자열** → 표 | 잡는다 |
//!
//! 파라미터 **키** 축은 여기서 안 본다 — 이름이 맞아도 키가 틀리면 핸들러가
//! `Missing required '...' parameter` 로 거절한다. 그 축은
//! `src/adapters/ipc/handler/cli_entry_tests.rs` 가 CLI 가 조립한 params 를 프로덕션
//! 핸들러에 그대로 먹여 닫는다.
//!
//! **[`CLI_REQUEST_SOURCES`] / [`CLI_REQUEST_DIRS`] 에서 빠진 위치는 그대로 사각지대다** —
//! `ipc_router_table_parity.rs` 의 `ROUTER_SOURCES` 가 처음 목록에서 파일을 빠뜨려
//! 메서드 하나를 통과시킨 선례가 있다. 그래서 `request/` 는 **디렉토리째 재귀**로 걷고,
//! 그 밖에서 요청을 직접 만드는 곳은 `method: "…"` 필드 형태를 크레이트 전체에서 훑는다.
//!
//! release 빌드에서는 `DEBUG_METHODS` 가 설계상 비어 있어(`debug.*` 가 release IPC 표면에서
//! 사라진다) CLI 의 debug 커맨드 이름과 대조가 성립하지 않는다. 따라서 debug 빌드에서만
//! 돈다 — CI 의 `cargo test --workspace --locked` 가 debug 다.
#![cfg(debug_assertions)]

use std::path::{Path, PathBuf};

use tasty_ipc::method_meta::{METHOD_TABLE, method_meta};

/// 값 위치(튜플 원소) 메서드 리터럴을 훑을 고정 소스.
const CLI_REQUEST_SOURCES: &[&str] = &["crates/tasty-cli/src/request.rs"];

/// 같은 목적으로 **재귀로 걷는** 디렉토리. 커맨드 그룹이 늘 때마다 사람이 위 목록에
/// 손으로 추가하는 걸 잊으면 사각지대가 생기므로 통째로 건다.
const CLI_REQUEST_DIRS: &[&str] = &["crates/tasty-cli/src/request"];

/// `method: "…"` 필드 형태를 훑을 루트. `request/` 밖에서 요청을 직접 조립하는 곳이
/// 실제로 있다(`local/remote_check.rs` 의 `system.info`, `plugin.rs` 의
/// `plugin.audit_follow`) — 그쪽도 오타가 나면 똑같이 런타임에만 드러난다.
const CLI_CRATE_ROOT: &str = "crates/tasty-cli/src";

/// **보내지 않는** 센티널 — `(이름, 사유)`.
///
/// `command_to_request` 의 일부 갈래는 IPC 앞단에서 이미 로컬 처리돼 이 함수에 도달하지
/// 않는다. 그래도 팔을 남기는 이유는 각 지점의 주석에 적혀 있다(컴파일러가 보장하는
/// 미도달을 런타임 panic 으로 바꾸지 않기 위해서다). 그 자리에 두는 이름은 표에 없는
/// 것이 **정상**이므로 여기 사유와 함께 등재한다.
///
/// 목록은 실제보다 넓으면 안 된다 — 등재했는데 소스에 없으면 실패한다(아래 stale 검사).
const NOT_SENT_SENTINELS: &[(&str, &str)] = &[
    (
        "port.noop",
        "`tasty port` 는 run.rs 가 IPC 전에 로컬 처리한다",
    ),
    (
        "remote.check.noop",
        "`remote check` 는 run_client 가 SSH 터널 + 자체 IPC 로 선처리한다",
    ),
    (
        "remote.workspaces.noop",
        "`remote workspaces` 는 run_client 가 remote_browse 로 선처리한다",
    ),
    (
        "remote.new_workspace.noop",
        "`remote new-workspace` 는 run_client 가 remote_create 로 선처리한다",
    ),
    (
        "tool.ssh.noop",
        "`tasty tool ssh` 는 dispatch::classify 가 클라이언트 주도 실행으로 가져간다",
    ),
    (
        "tool.remote_profile.noop",
        "`tasty tool remote-profile` 은 dispatch::classify 가 가져간다(에이전트 조작은 remote.profile.* IPC)",
    ),
    (
        "tool.attach.noop",
        "`tasty tool attach` 는 dispatch::classify 가 가져간다",
    ),
    (
        "tool.passkey.noop",
        "`tasty tool passkey` 는 dispatch::classify 가 가져간다",
    ),
];

/// `ns.method` 형태(소문자·`_`·`.`, 점 하나 이상)인지. `ipc_router_table_parity` 와 같은
/// 판정이라 `"2.0"`(jsonrpc 버전) 같은 리터럴이 구조적으로 배제된다.
fn is_method_name(name: &str) -> bool {
    name.contains('.')
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '.')
        && name.starts_with(|c: char| c.is_ascii_lowercase())
}

/// 리터럴이 **함수 호출 인자**인가.
///
/// 이 크레이트에서 메서드 이름과 같은 모양의 리터럴을 쓰는 다른 용도는 i18n 키
/// (`tasty_i18n::t_args("cli.agent.…", …)`)뿐이고, 그것들은 전부 호출 인자다. 반면 메서드
/// 이름은 `("ns.verb", params)` 튜플의 첫 원소라 호출 인자가 아니다 — 이 구조 차이로
/// 가르면 `cli.` 같은 **접두사 allowlist 가 필요 없다**(접두사로 거르면 새 접두사가
/// 생길 때마다 목록이 늘고, 접미사 오타를 통과시키는 느슨한 매칭이 된다).
///
/// 여는 괄호가 앞 줄 끝에 있는 여러 줄 호출도 본다 — 실제로 i18n 키 넷이 그 형태다.
fn is_call_argument(before: &str, prev_lines: &[&str]) -> bool {
    let mut head = before.trim_end();
    if head.is_empty() {
        match prev_lines.iter().rev().find(|l| !l.trim().is_empty()) {
            Some(l) => head = l.trim(),
            None => return false,
        }
    }
    let Some(open) = head.strip_suffix('(') else {
        return false;
    };
    // `t(` / `t_args(` / `json!(` 처럼 여는 괄호 바로 앞이 식별자면 호출이다.
    // `(` 만 있거나 `=> (` 면 튜플이다.
    open.trim_end()
        .chars()
        .next_back()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_' || c == '!')
}

/// 한 줄에서 값 위치의 메서드 리터럴을 전부 뽑는다.
fn value_position_methods<'a>(line: &'a str, prev_lines: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    while let Some(rel) = line[idx..].find('"') {
        let start = idx + rel + 1;
        let Some(rel_end) = line[start..].find('"') else {
            break;
        };
        let end = start + rel_end;
        let name = &line[start..end];
        if is_method_name(name) && !is_call_argument(&line[..start - 1], prev_lines) {
            out.push(name);
        }
        idx = end + 1;
    }
    out
}

/// `method: "ns.verb"` 필드 초기화에서 이름을 뽑는다. `jsonrpc: "2.0"` 같은 다른 필드는
/// 필드명으로 배제된다.
fn field_method(line: &str) -> Option<&str> {
    let pos = line.find("method:")?;
    let after = line[pos + "method:".len()..].trim_start();
    let rest = after.strip_prefix('"')?;
    let (name, _) = rest.split_once('"')?;
    is_method_name(name).then_some(name)
}

/// `#[cfg(test)] mod …` 블록에 속하는 줄 번호를 표시한다.
///
/// 테스트 픽스처는 **보내지 않는** 이름을 자유롭게 쓴다 — 실제로 plugin 이 런타임에
/// 등록하는 이름(`claude.wait_by_surface`)과 합성 이름(`x.wait`)이 픽스처에 있다.
/// 이 가드의 대상은 "프로덕션 CLI 가 실제로 보내는 이름" 이므로 그 블록을 구조적으로
/// 뺀다 — 이름을 allowlist 에 적어 빼면 같은 이름이 프로덕션 자리에 와도 통과한다.
///
/// 중괄호 깊이로 블록 끝을 찾으므로 파일 중간의 테스트 모듈도 정확히 잡힌다.
fn test_module_lines(lines: &[&str]) -> Vec<bool> {
    let mut skip = vec![false; lines.len()];
    let mut i = 0usize;
    while i < lines.len() {
        let t = lines[i].trim();
        let is_mod_decl = t.starts_with("mod ")
            || t.starts_with("pub mod ")
            || t.starts_with("pub(crate) mod ")
            || t.starts_with("pub(super) mod ");
        let gated = lines[..i]
            .iter()
            .rev()
            .find(|l| !l.trim().is_empty())
            .is_some_and(|l| l.trim() == "#[cfg(test)]");
        if is_mod_decl && gated && t.ends_with('{') {
            let mut depth = 0i32;
            while i < lines.len() {
                skip[i] = true;
                depth += lines[i].matches('{').count() as i32;
                depth -= lines[i].matches('}').count() as i32;
                i += 1;
                if depth <= 0 {
                    break;
                }
            }
            continue;
        }
        i += 1;
    }
    skip
}

fn gather_rs(path: &Path, out: &mut Vec<PathBuf>) {
    if path.is_file() {
        if path.extension().and_then(|e| e.to_str()) == Some("rs") {
            out.push(path.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        gather_rs(&entry.path(), out);
    }
}

fn rel_of(file: &Path, root: &Path) -> String {
    file.strip_prefix(root)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

#[test]
fn every_cli_method_string_is_registered_in_method_table() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));

    // 값 위치 스캔 대상: 고정 소스 + `request/` 재귀.
    let mut request_files: Vec<PathBuf> =
        CLI_REQUEST_SOURCES.iter().map(|s| root.join(s)).collect();
    for dir in CLI_REQUEST_DIRS {
        gather_rs(&root.join(dir), &mut request_files);
    }
    request_files.sort();
    request_files.dedup();

    let mut found: Vec<(String, String)> = Vec::new(); // (메서드, 위치)
    for path in &request_files {
        let rel = rel_of(path, root);
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("CLI request 소스를 읽을 수 없다: {rel}: {e}"));
        let lines: Vec<&str> = src.lines().collect();
        let in_tests = test_module_lines(&lines);
        for (i, line) in lines.iter().enumerate() {
            if in_tests[i] {
                continue;
            }
            for name in value_position_methods(line, &lines[..i]) {
                found.push((name.to_string(), format!("{rel}:{}", i + 1)));
            }
        }
    }

    // `method: "…"` 필드 스캔 대상: 크레이트 전체.
    let mut all_files = Vec::new();
    gather_rs(&root.join(CLI_CRATE_ROOT), &mut all_files);
    all_files.sort();
    for path in &all_files {
        let rel = rel_of(path, root);
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        let lines: Vec<&str> = src.lines().collect();
        let in_tests = test_module_lines(&lines);
        for (i, line) in lines.iter().enumerate() {
            if in_tests[i] {
                continue;
            }
            if let Some(name) = field_method(line) {
                found.push((name.to_string(), format!("{rel}:{}", i + 1)));
            }
        }
    }

    assert!(
        found.len() > 200,
        "CLI 메서드 리터럴을 {} 개밖에 못 찾았다(하한 200) — 스캔 패턴이나 소스 목록이 \
         깨졌을 가능성이 크다. 조용한 미스캔은 위양성보다 나쁘므로 여기서 실패시킨다",
        found.len()
    );

    let mut unknown: Vec<String> = Vec::new();
    let mut sentinel_hit: Vec<&str> = Vec::new();
    for (name, at) in &found {
        if let Some((s, _)) = NOT_SENT_SENTINELS.iter().find(|(s, _)| s == name) {
            sentinel_hit.push(s);
            continue;
        }
        if method_meta(name).is_none() {
            unknown.push(format!("  {at} — `{name}`"));
        }
    }
    unknown.sort();
    unknown.dedup();
    assert!(
        unknown.is_empty(),
        "CLI 가 보내는 메서드 이름이 METHOD_TABLE/DEBUG_METHODS/PREFIX_RULES 어디에도 없다 \
         — 호스트가 UnknownMethod 로 거부하고, 그건 런타임에만 드러난다. 오타면 고치고, \
         새 메서드면 표에 등재하라. IPC 로 보내지 않는 센티널이면 NOT_SENT_SENTINELS 에 \
         사유와 함께 등재하라:\n{}",
        unknown.join("\n")
    );

    // 역방향 — 센티널 목록이 실제보다 넓으면 그 이름이 나중에 진짜 메서드 자리에 와도
    // 조용히 통과한다.
    let stale: Vec<&str> = NOT_SENT_SENTINELS
        .iter()
        .map(|(s, _)| *s)
        .filter(|s| !sentinel_hit.contains(s))
        .collect();
    assert!(
        stale.is_empty(),
        "NOT_SENT_SENTINELS 에 있으나 소스에 없다 — 갈래가 사라졌으면 목록에서도 지울 것:\n  {}",
        stale.join("\n  ")
    );
}

/// 무점(root) 메서드는 위 스캔의 사각지대라, 반대 방향으로 막는다.
///
/// [`is_method_name`] 이 점을 요구하므로 `"split"` · `"tree"` 같은 root 메서드는 값 위치
/// 스캔에 안 잡힌다. 점을 빼면 `"terminal"` · `"idle"` 같은 평범한 리터럴이 전부 후보가
/// 되어 가드가 노이즈에 묻힌다.
///
/// 대신 **표 → CLI** 방향으로 본다: 표의 무점 메서드는 그 리터럴이 CLI request 소스에
/// 정확히 있어야 한다. `"tree"` 를 `"tre"` 로 오타내면 `"tree"` 가 사라져 여기서 떨어진다.
///
/// 이 방향이 성립하는 근거는 명명 규칙이다 — `docs/dev-guide/api-conventions.md` 가
/// root 등록을 `split` · `tree` **둘로 닫아** 두고 "새 메서드는 이 예외에 동참 금지" 라고
/// 못박았다. 그래서 무점 메서드는 곧 "자주 쓰는 짧은 CLI 명령" 이고, CLI 진입점이 없는
/// 무점 메서드는 존재하지 않는다. 규칙을 깨는 무점 메서드가 새로 생기면 여기서 실패하는데,
/// 그건 오탐이 아니라 **그 규칙을 다시 논의하라는 신호**다.
#[test]
fn root_level_methods_are_reachable_from_the_cli() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut request_files: Vec<PathBuf> =
        CLI_REQUEST_SOURCES.iter().map(|s| root.join(s)).collect();
    for dir in CLI_REQUEST_DIRS {
        gather_rs(&root.join(dir), &mut request_files);
    }
    let corpus: String = request_files
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .collect();

    let root_methods: Vec<&str> = METHOD_TABLE
        .iter()
        .map(|(name, _)| *name)
        .filter(|name| !name.contains('.'))
        .collect();
    assert!(
        !root_methods.is_empty(),
        "표에 무점 메서드가 하나도 없다 — 추출이 깨졌거나 규칙이 바뀌었다"
    );

    let unreachable: Vec<&str> = root_methods
        .iter()
        .copied()
        .filter(|name| !corpus.contains(&format!("\"{name}\"")))
        .collect();
    assert!(
        unreachable.is_empty(),
        "표의 root(무점) 메서드가 CLI request 소스에 리터럴로 없다 — 오타로 이름이 어긋났거나, \
         명명 규칙이 닫아 둔 root 예외에 CLI 없는 메서드가 새로 들어왔다:\n  {}",
        unreachable.join("\n  ")
    );
}
