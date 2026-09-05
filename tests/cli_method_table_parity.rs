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
//! 사라진다) CLI 의 debug 커맨드 이름과 대조가 성립하지 않는다. 따라서 아래
//! `#![cfg(debug_assertions)]` 로 debug 빌드에서만 돈다 — `cargo test` 의 기본 프로필이
//! debug 라 그냥 돌리면 포함되고, 테스트를 `--release` 로 돌리면 이 파일은 통째로
//! 컴파일에서 빠져 아무것도 검증하지 않는다.
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

/// 한 줄에서 **값 위치**(호출 인자가 아닌) 리터럴을 전부 뽑는다. 이름 모양은 안 본다.
fn value_position_literals<'a>(line: &'a str, prev_lines: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut idx = 0usize;
    while let Some(rel) = line[idx..].find('"') {
        let start = idx + rel + 1;
        let Some(rel_end) = line[start..].find('"') else {
            break;
        };
        let end = start + rel_end;
        if !is_call_argument(&line[..start - 1], prev_lines) {
            out.push(&line[start..end]);
        }
        idx = end + 1;
    }
    out
}

/// 값 위치 리터럴 중 `ns.verb` 모양인 것만.
///
/// 점을 요구하는 이유는 **오타 탐지 방향**(이 이름이 표에 있나)에서는 모양 제한이 없으면
/// `"terminal"`·`"idle"` 같은 평범한 리터럴이 전부 후보가 되어 가드가 노이즈에 묻히기
/// 때문이다. 반대 방향(표의 이름이 CLI 에 있나)에서는 표와 교집합을 잡으므로 노이즈가
/// 무해해 [`value_position_literals`] 를 그대로 쓴다 — 무점 root 메서드(`split`·`tree`)가
/// 그 차이로 살아난다.
fn value_position_methods<'a>(line: &'a str, prev_lines: &[&str]) -> Vec<&'a str> {
    value_position_literals(line, prev_lines)
        .into_iter()
        .filter(|name| is_method_name(name))
        .collect()
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

// ─────────────────────────────────────────────────────────────────────────────
// 표 → CLI 방향: "release 표에 있는데 CLI 가 없는" 집합이 문서와 1:1 인가.
// ─────────────────────────────────────────────────────────────────────────────

const CLI_GAP_DOC: &str = "docs/dev-guide/api-conventions.md";

/// 문서에서 표를 특정하는 헤더 행.
const CLI_GAP_TABLE_HEADER: &str = "| 이유 | 메서드 | 왜 CLI 가 없나 |";

/// debug 절반의 표를 특정하는 헤더 행. release 표와 **다른 표**다 — `debug.*` 는
/// release 빌드에 아예 없으므로 "release IPC 에 있는데 CLI 가 없다" 와 같은 문장으로
/// 묶이지 않는다.
const CLI_GAP_DEBUG_TABLE_HEADER: &str = "| 이유 | debug 메서드 | 왜 CLI 가 없나 |";

/// 셀 안의 **모든 백틱 코드 스팬**. 한 셀에 메서드를 `·` 로 여럿 늘어놓는다.
///
/// 코드 스팬만 뽑으므로 백틱 밖의 사람이 읽는 단서는 자연히 잘린다 — 그래서 대조를
/// `starts_with` 로 느슨하게 할 이유가 없고 **정확 일치**로 본다. 느슨하면 문서 쪽
/// 접미사 오타(`attach.list_TYPO`)가 그대로 통과해 이 가드의 존재 이유가 반쪽이 된다.
/// (`permission_free_methods_docs_parity.rs` 와 같은 규약.)
fn code_spans(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// CLI 에서 도달 가능한 메서드 이름 전부.
///
/// 세 경로를 합친다 — request 소스의 값 위치 리터럴, 크레이트 전역의 `method: "…"` 필드,
/// 번들 plugin 매니페스트의 `ipc_method`(동적 plugin CLI 가 그 이름으로 부른다).
fn cli_reachable_methods(root: &Path) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();

    let mut request_files: Vec<PathBuf> =
        CLI_REQUEST_SOURCES.iter().map(|s| root.join(s)).collect();
    for dir in CLI_REQUEST_DIRS {
        gather_rs(&root.join(dir), &mut request_files);
    }
    for path in &request_files {
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        let lines: Vec<&str> = src.lines().collect();
        let in_tests = test_module_lines(&lines);
        for (i, line) in lines.iter().enumerate() {
            if in_tests[i] {
                continue;
            }
            for name in value_position_literals(line, &lines[..i]) {
                out.insert(name.to_string());
            }
        }
    }

    let mut all_files = Vec::new();
    gather_rs(&root.join(CLI_CRATE_ROOT), &mut all_files);
    for path in &all_files {
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        let lines: Vec<&str> = src.lines().collect();
        let in_tests = test_module_lines(&lines);
        for (i, line) in lines.iter().enumerate() {
            if !in_tests[i]
                && let Some(name) = field_method(line)
            {
                out.insert(name.to_string());
            }
        }
    }

    // 번들 plugin 매니페스트의 `ipc_method = "…"`.
    let mut manifests = Vec::new();
    collect_manifests(&root.join("crates"), &mut manifests);
    for path in &manifests {
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        // `ipc_method = "…"` 는 줄 맨 앞에도, 인라인 테이블
        // (`{ name = "open", ipc_method = "image.open", … }`) 안에도 온다.
        for line in src.lines() {
            let mut rest = line;
            while let Some(pos) = rest.find("ipc_method") {
                let after = &rest[pos + "ipc_method".len()..];
                rest = after;
                let Some(eq) = after.trim_start().strip_prefix('=') else {
                    continue;
                };
                let Some(q) = eq.trim_start().strip_prefix('"') else {
                    continue;
                };
                if let Some((name, tail)) = q.split_once('"') {
                    out.insert(name.to_string());
                    rest = tail;
                }
            }
        }
    }
    out
}

fn collect_manifests(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            collect_manifests(&p, out);
        } else if p.file_name().and_then(|n| n.to_str()) == Some("tasty-plugin.toml") {
            out.push(p);
        }
    }
}

/// 문서에서 `header` 로 시작하는 표의 메서드 이름을 전부 뽑는다.
///
/// 두 표(release · debug)가 같은 파서를 쓴다 — 두 벌로 두면 한쪽만 고쳐지는 순간
/// 대조 규약이 갈린다.
fn listed_methods(text: &str, header: &str) -> Vec<String> {
    let after_header = text
        .split_once(header)
        .unwrap_or_else(|| panic!("{CLI_GAP_DOC}: `{header}` 표 헤더를 찾지 못했다"))
        .1;
    after_header
        .lines()
        .skip(1) // `|---|---|---|` 구분선
        .take_while(|l| l.trim_start().starts_with('|'))
        .flat_map(|line| {
            let cells: Vec<&str> = line.trim().trim_matches('|').split('|').collect();
            if cells.len() != 3 {
                panic!("{CLI_GAP_DOC}: 표 행의 열 수가 3이 아니다: {line}");
            }
            if cells[0].trim().starts_with("---") {
                return Vec::new();
            }
            let methods = code_spans(cells[1]);
            assert!(
                !methods.is_empty(),
                "{CLI_GAP_DOC}: 표 행의 메서드 열이 비었다: {line}"
            );
            methods
        })
        .collect()
}

/// 계산된 집합과 문서 표를 **양방향으로** 대조한다.
fn assert_documented(
    actual: &std::collections::BTreeSet<String>,
    text: &str,
    header: &str,
    count_marker: &str,
    what: &str,
) {
    let listed = listed_methods(text, header);
    let listed_set: std::collections::BTreeSet<String> = listed.iter().cloned().collect();
    assert_eq!(
        listed.len(),
        listed_set.len(),
        "{CLI_GAP_DOC}: 표에 같은 메서드가 두 번 적혔다 — 한 메서드의 이유는 한 곳이어야 한다"
    );

    let undocumented: Vec<&String> = actual.difference(&listed_set).collect();
    assert!(
        undocumented.is_empty(),
        "{what} 있는데 CLI 진입점이 없고, {CLI_GAP_DOC} 에도 이유가 없다. \
         에이전트 기능이면 CLI 를 만들고(원칙 2), 원칙 밖이면 그 이유를 표에 적어라 \
         — 이유 없이 두면 누락과 구분되지 않는다:\n  {}",
        undocumented
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );

    let stale: Vec<&String> = listed_set.difference(actual).collect();
    assert!(
        stale.is_empty(),
        "{CLI_GAP_DOC} 표에 있으나 실제로는 CLI 진입점이 생겼거나 메서드가 사라졌다 — \
         행을 지워야 표가 참이 된다:\n  {}",
        stale
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join("\n  ")
    );

    assert!(
        text.contains(count_marker),
        "{CLI_GAP_DOC}: 표 앞 산문의 개수가 실제({})와 다르다 — `{count_marker}` 로 맞춰라",
        actual.len()
    );
}

/// release 표에 있는데 CLI 진입점이 없는 메서드가 문서 표와 **양방향으로** 맞는지.
///
/// 원칙 2("에이전트 기능은 IPC + CLI 양면")는 release IPC 표면 **전체**가 아니라
/// 에이전트 기능에 걸린다 — plugin 이 host 에게 자기 자원을 요청하는 서비스 메서드는
/// CLI 호출자가 애초에 없다. 그래서 이 집합이 비어 있어야 하는 것이 아니라, **각 항목이
/// 왜 밖인지가 문서에 남아 있어야** 한다. 이유 없이 남아 있으면 누락과 구분되지 않고,
/// 실제로 같은 감사 티켓이 두 번 올라왔다.
///
/// 이 가드는 **이유의 내용**은 보지 않는다(기계가 판정할 값이 아니다) — 목록이 문서와
/// 어긋나는 것만 본다.
#[test]
fn methods_without_a_cli_entry_point_are_documented() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let reachable = cli_reachable_methods(root);

    let actual: std::collections::BTreeSet<String> = METHOD_TABLE
        .iter()
        .map(|(name, _)| name.to_string())
        .filter(|name| !reachable.contains(name))
        .collect();

    let text = std::fs::read_to_string(root.join(CLI_GAP_DOC))
        .unwrap_or_else(|e| panic!("read {CLI_GAP_DOC}: {e}"));
    let marker = format!("총 {}개.", actual.len());
    assert_documented(
        &actual,
        &text,
        CLI_GAP_TABLE_HEADER,
        &marker,
        "release 표에",
    );
}

/// debug 절반도 같은 대조를 받는다.
///
/// `debug.*` 는 release IPC 표면에 없지만 **debug 빌드의 에이전트 표면**이고, 원칙 2 는
/// 거기서도 성립한다 — 실제로 이 축을 실행으로 재 보니 debug 메서드 14 개가 CLI 로
/// 부를 수 없었고, 그중 어느 것도 "왜 없는지" 가 어디에도 없었다. release 절반만 보는
/// 가드는 그 14 개를 한 번도 못 봤다.
///
/// 표를 둘로 나눈 이유: 두 집합의 문장이 다르다. release 쪽은 "release IPC 에 있는데
/// CLI 가 없다" 이고 debug 쪽은 "debug 빌드에만 있는데 그 빌드의 CLI 에도 없다" 다.
/// 한 표에 섞으면 어느 빌드 이야기인지가 행마다 달라진다.
#[test]
fn debug_methods_without_a_cli_entry_point_are_documented() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let reachable = cli_reachable_methods(root);

    let actual: std::collections::BTreeSet<String> = tasty_ipc::method_meta::DEBUG_METHODS
        .iter()
        .map(|(name, _)| name.to_string())
        .filter(|name| !reachable.contains(name))
        .collect();

    let text = std::fs::read_to_string(root.join(CLI_GAP_DOC))
        .unwrap_or_else(|e| panic!("read {CLI_GAP_DOC}: {e}"));
    let marker = format!("debug 표 기준 총 {}개.", actual.len());
    assert_documented(
        &actual,
        &text,
        CLI_GAP_DEBUG_TABLE_HEADER,
        &marker,
        "debug 표에",
    );
}
