//! 번들 plugin 이 IPC namespace 를 점유하면, 그 이름 아래 **host 가 구현한 메서드**는
//! 외부 호출에서 plugin 으로 forward 된다. plugin 의 inbound dispatch 에 그 이름의
//! arm 이 없으면 host 구현은 **외부에서만** 안 닿는다 — plugin 이 설치돼 있으면 막히고
//! 빠지면 열리는, 설치 상태에 따라 흔들리는 표면이 된다.
//!
//! 실측(2026-09-05, 두 조합 × plugin 유무 3 세계 실행 census):
//!
//! | 세계 | `markdown.navigate` 응답 |
//! |------|--------------------------|
//! | gui, plugin 없음 | `-32602 missing field surface_id` (host arm 이 답했다) |
//! | gui, plugin 설치 | `-32601 method 'markdown.navigate' not found` (plugin 이 답했다) |
//! | headless | `-32601` (host arm 자체가 gui 게이트) |
//!
//! 같은 이름이 **누가 답하느냐에 따라** 다른 결과를 냈다. `image.open`/`image.list` 는
//! 같은 형태인데 plugin 이 self-call trampoline 로 host 에 돌려주고 있어 어느 세계에서도
//! host arm 에 닿는다. 즉 관례가 둘이었던 것이 아니라 하나였고 이탈이 하나였다.
//!
//! ## 왜 dispatch 본문만 보는가 (이 가드의 핵심)
//!
//! 이탈하던 시점에도 `"markdown.navigate"` 라는 **문자열은 plugin 소스에 있었다** —
//! plugin 이 host 로 *거는* `host.call("markdown.navigate", …)` 자리다. 파일 전체에서
//! 리터럴을 세는 판정은 그래서 그때도 초록이었다. 방향이 반대인 두 자리가 같은 문자열을
//! 쓰므로, **inbound dispatch 함수의 본문**만 잘라 보는 것이 이 판정의 전부다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use tasty_ipc::method_meta::METHOD_TABLE;

use super::repo_root;

const CRATES_DIR: &str = "crates";
const PLUGIN_CRATE_PREFIX: &str = "tasty-plugin-";
const MANIFEST_NAME: &str = "tasty-plugin.toml";

/// plugin 이 host→plugin 호출을 받는 자리. SDK trait 의 메서드 이름이다.
const DISPATCH_FN: &str = "fn handle_ipc_method";

/// namespace 를 선언한 번들 plugin 수의 하한 — **연기 검사**다.
/// 값의 근거: 2026-09-05 실측 2 (image, markdown).
const MIN_NAMESPACE_PLUGINS: usize = 2;

/// 호스트 메서드 수의 하한. 표가 비면 아래 포함 판정이 빈 집합끼리라 그냥 통과한다.
/// 값의 근거: 2026-09-05 실측 `METHOD_TABLE.len()` = 276.
const MIN_HOST_METHODS: usize = 200;

fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} 을 읽지 못했다: {e}", path.display()))
        .replace("\r\n", "\n")
}

/// `crates/tasty-plugin-*/` 중 매니페스트를 가진 디렉터리.
fn bundled_plugin_dirs() -> Vec<PathBuf> {
    let root = repo_root().join(CRATES_DIR);
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&root).expect("crates 디렉터리를 읽을 수 없다") {
        let entry = entry.expect("디렉터리 항목을 읽을 수 없다");
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(PLUGIN_CRATE_PREFIX) && path.join(MANIFEST_NAME).is_file() {
            out.push(path);
        }
    }
    out.sort();
    out
}

/// 매니페스트가 선언한 `[[contributes.ipc_namespace]]` prefix — **실제 파서로** 읽는다.
/// 정규식으로 긁으면 주석 처리된 블록이나 다른 테이블의 `prefix =` 를 같이 집는다.
fn declared_prefixes(dir: &std::path::Path) -> Vec<String> {
    let text = read(&dir.join(MANIFEST_NAME));
    let manifest: tasty_plugin_manifest::Manifest = toml::from_str(&text)
        .unwrap_or_else(|e| panic!("{}/{MANIFEST_NAME} 파싱 실패: {e}", dir.display()));
    manifest
        .contributes
        .ipc_namespace
        .iter()
        .map(|d| d.prefix.clone())
        .collect()
}

/// `fn handle_ipc_method` 의 본문을 중괄호 균형으로 잘라낸다.
///
/// 들여쓰기에 의존하지 않는다 — rustfmt 스타일이 바뀌어도 같은 것을 자른다.
/// 문자열 안의 중괄호는 세지 않는다(`"{}"` 포맷 리터럴이 흔하다).
fn dispatch_body(src: &str) -> Option<String> {
    let at = src.find(DISPATCH_FN)?;
    let open = src[at..].find('{')? + at;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut escaped = false;
    for (i, c) in src[open..].char_indices() {
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(src[open..open + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

/// plugin 크레이트의 `src/` 전체에서 inbound dispatch 본문들을 모은다.
fn dispatch_bodies(dir: &std::path::Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.join("src")];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries {
            let entry = entry.expect("디렉터리 항목을 읽을 수 없다");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs")
                && let Some(body) = dispatch_body(&read(&path))
            {
                out.push((path, body));
            }
        }
    }
    out
}

/// 본문에 나타나는 `"<prefix>.…"` 문자열 리터럴.
fn handled_methods(body: &str, prefix: &str) -> BTreeSet<String> {
    let needle = format!("\"{prefix}.");
    let mut out = BTreeSet::new();
    let mut rest = body;
    while let Some(at) = rest.find(&needle) {
        let after = &rest[at + 1..];
        if let Some(end) = after.find('"') {
            out.insert(after[..end].to_string());
            rest = &after[end..];
        } else {
            break;
        }
    }
    out
}

/// prefix → 그 아래 host 가 등재한 메서드.
fn host_methods_by_prefix() -> BTreeMap<String, BTreeSet<String>> {
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (name, _) in METHOD_TABLE {
        if let Some((p, _)) = name.split_once('.') {
            out.entry(p.to_string()).or_default().insert((*name).into());
        }
    }
    out
}

/// 번들 plugin 이 점유한 namespace 아래의 host 메서드는 **전부** 그 plugin 의 inbound
/// dispatch 가 받는다.
#[test]
fn every_host_method_under_a_bundled_namespace_is_handled_by_that_plugin() {
    assert!(
        METHOD_TABLE.len() >= MIN_HOST_METHODS,
        "호스트 메서드가 {} 건뿐이다(하한 {MIN_HOST_METHODS}, 2026-09-05 실측 276). \
         표가 비면 아래 포함 판정은 빈 집합끼리라 그냥 통과한다",
        METHOD_TABLE.len()
    );
    let by_prefix = host_methods_by_prefix();

    let mut namespaces = 0usize;
    let mut missing: Vec<String> = Vec::new();
    for dir in bundled_plugin_dirs() {
        for prefix in declared_prefixes(&dir) {
            namespaces += 1;
            let Some(host) = by_prefix.get(&prefix) else {
                // host 가 그 이름 아래 아무것도 구현하지 않았다 — 가려질 것이 없다.
                continue;
            };
            let bodies = dispatch_bodies(&dir);
            assert!(
                !bodies.is_empty(),
                "{} 에서 `{DISPATCH_FN}` 본문을 하나도 못 잘랐다 — 대조군이 죽었다. \
                 SDK trait 의 이름이 바뀌었는지 확인해라",
                dir.display()
            );
            let handled: BTreeSet<String> = bodies
                .iter()
                .flat_map(|(_, b)| handled_methods(b, &prefix))
                .collect();
            for m in host.difference(&handled) {
                missing.push(format!(
                    "{m} — host 가 구현했는데 {} 의 inbound dispatch 에 arm 이 없다",
                    dir.file_name().unwrap_or_default().to_string_lossy()
                ));
            }
        }
    }

    assert!(
        namespaces >= MIN_NAMESPACE_PLUGINS,
        "ipc_namespace 를 선언한 번들 plugin 이 {namespaces} 개뿐이다(하한 \
         {MIN_NAMESPACE_PLUGINS}, 2026-09-05 실측 2). 매니페스트 탐색이 죽었다"
    );
    assert!(
        missing.is_empty(),
        "번들 plugin 이 점유한 namespace 아래에서 host 구현이 외부 호출에 안 닿는다.\n\
         plugin 이 설치돼 있으면 막히고 빠지면 열리는 표면이 된다 — \
         `image.open`/`image.list` 처럼 self-call trampoline arm 을 두거나, host 가 \
         그 이름을 구현하지 않게 해라.\n  {}",
        missing.join("\n  ")
    );
}

/// 판정이 **inbound dispatch 본문만** 본다.
///
/// 이 가드가 잡아야 했던 실제 이탈은 같은 문자열이 파일 안 다른 자리(plugin → host 로
/// *거는* `host.call`)에 있었다. 파일 전체를 세면 그때도 초록이었다 — 그래서 자르기가
/// 실제로 좁혀졌는지를 여기서 못 박는다.
#[test]
fn the_cut_is_the_dispatch_body_not_the_whole_file() {
    let src = "\
fn other_before() { emit(\"ns.before\"); }
fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> R {
    match ctx.method.as_str() {
        \"ns.inside\" => ok(),
        other => not_found(other),
    }
}
fn other_after() { host.call(\"ns.after\", p); }
";
    let body = dispatch_body(src).expect("본문을 잘라야 한다");
    let found = handled_methods(&body, "ns");
    assert!(found.contains("ns.inside"), "본문 안의 이름은 잡아야 한다");
    assert!(
        !found.contains("ns.before") && !found.contains("ns.after"),
        "본문 밖의 같은 prefix 리터럴을 집었다 — 자르기가 안 좁혀졌다: {found:?}"
    );
    assert!(
        !body.contains("other_after"),
        "본문이 함수 끝을 넘어 이어졌다"
    );
}

/// 중괄호 세기가 문자열 안의 `{`/`}` 에 속지 않는다.
#[test]
fn braces_inside_string_literals_do_not_close_the_body() {
    let src = "\
fn handle_ipc_method(&mut self) -> R {
    emit(format!(\"{}\", x));
    emit(\"}\");
    match m { \"ns.inside\" => ok() }
}
fn after() { emit(\"ns.after\"); }
";
    let body = dispatch_body(src).expect("본문을 잘라야 한다");
    let found = handled_methods(&body, "ns");
    assert!(
        found.contains("ns.inside") && !found.contains("ns.after"),
        "문자열 안 중괄호에 속아 본문이 일찍 끊기거나 넘쳤다: {found:?}"
    );
}
