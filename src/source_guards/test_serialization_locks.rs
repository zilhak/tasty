//! 프로세스 전역을 만지는 테스트는 **그 전역의 직렬화 락을 잡는다.**
//!
//! `cargo test` 는 한 바이너리의 테스트를 병렬로 돌린다. 그래서 프로세스 전역(`static`
//! 변수·환경변수·cwd)을 만지는 테스트끼리는 서로의 상태를 덮어써 순서 의존 flake 가 난다.
//! 처방은 락 하나로 직렬화하는 것이고, 이 레포는 그렇게 하고 있다.
//!
//! **다만 락은 잡는 쪽끼리만 막는다.** 하나라도 락 밖에서 그 전역을 만지면 직렬화가 통째로
//! 무효가 된다 — 그래서 이 규칙은 "이 락을 쓰자" 가 아니라 **"이 전역을 만지는 테스트가
//! 전부 이 락을 잡는다"** 는 전수 명제다. 전수 명제인데 강제가 없으면, 다음에 그 전역을
//! 만지는 테스트를 더하는 사람이 락의 존재를 알 길이 없다.
//!
//! # 재고 나서 안 것 — 명부가 이름으로는 안 모인다
//!
//! 2026-09-05 실측. 이름(`*TEST_LOCK`)으로 세면 **4** 개인데, 모양(`static …: Mutex<()>`)
//! 으로 세면 **11** 개다. 나머지 일곱은 `SERIAL` · `ENV_LOCK` · `CWD_LOCK` · `TEST_SERIAL` ·
//! `GLOBALS` · `HOME_ENV_LOCK` · `TASTY_HOME_ENV_LOCK` 이라 이름 규칙이 없다. 그리고 열하나
//! 전부가 **같은 문장을 주석으로만** 갖고 있었다("이 전역을 만지는 테스트는 이 락을 잡아라").
//! 채널이 있던 것은 둘뿐이다(`tasty-host-plugin` 의 홈 env 스캔, `tasty-cli` 의 cwd 가드).
//!
//! 그래서 명부의 완전성은 **모양으로** 판정한다 — [`every_serialization_lock_is_listed`].
//!
//! # 무엇을 재고 무엇을 안 재는가
//!
//! 재는 것은 **전역 변수**를 지키는 여섯이다. `static` 하나를 여러 테스트가 만지는 형태라
//! "만지는가" 를 이름으로 판정할 수 있다.
//!
//! 안 재는 것은 **환경변수·cwd** 를 지키는 다섯이다. 그쪽은 만지는 자리가 `set_var` 나
//! `set_current_dir` 이고 그 대상이 문자열 키라, 같은 술어로는 "무엇을 만지는가" 가 안
//! 갈린다. 명부에는 남긴다 — 빠진 것이 아니라 **규칙이 다른 축**이라는 뜻이고, 그 다섯 중
//! 둘은 이미 자기 채널을 갖고 있다.
//!
//! # 이 판정이 틀리는 방향
//!
//! 이름을 **언급**하면 만진 것으로 센다. 실제로는 안 만지는데 이름만 나오는 테스트가 있으면
//! 락을 잡으라고 요구한다 — 거짓 양성이고, 시끄럽게 틀린다. 반대로 그 전역을 만지면서
//! 이름을 하나도 안 쓰는 경로(함수 두 겹 너머의 간접 접근)는 못 본다. 그래서 접근면에는
//! **전역을 쓰는 함수 이름들도 함께** 적는다.

use std::collections::BTreeSet;

use super::{
    mask_non_code, repo_root, rust_sources, rust_sources_with_integration_tests, word_positions,
};

/// 락이 지키는 것을 만지는 테스트를 어디까지 찾는가.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    /// 락이 선언된 파일 안만. 락이 모듈 밖으로 안 보이는 경우다.
    File,
    /// 크레이트 전체. 락이나 접근면이 `pub`/`pub(crate)` 이라 다른 파일의 테스트도 닿는다.
    Crate,
}

struct Serialized {
    /// 락이 선언된 파일(레포 상대).
    file: &'static str,
    /// 락 이름.
    lock: &'static str,
    /// 락을 **잡는** 표현. 락 이름 자체와, 락을 잡아 가드를 돌려주는 헬퍼들.
    acquire: &'static [&'static str],
    /// 이 락이 지키는 이름들 — 전역 자신과 그것을 읽고 쓰는 함수들.
    guarded: &'static [&'static str],
    scope: Scope,
    /// 왜 직렬화가 필요한가. 락마다 다르다 — 뭉뚱그리면 어느 것이 진짜 경합이고 어느
    /// 것이 습관인지가 지워진다.
    why: &'static str,
}

const SERIALIZED: &[Serialized] = &[
    Serialized {
        file: "src/core/surface_registry/webview_kind.rs",
        lock: "WEBVIEW_KIND_TEST_LOCK",
        acquire: &["WEBVIEW_KIND_TEST_LOCK"],
        guarded: &[
            "WEBVIEW_KINDS",
            "register_webview_kind",
            "is_webview_kind",
            "reset_for_test",
        ],
        scope: Scope::Crate,
        why: "락이 `pub` 이고 실제로 다른 파일의 테스트가 등록한다 — `state` 의 픽스처가 \
              markdown kind 를 등록하는데 그것이 `!is_webview_kind(\"markdown\")` 단언 \
              중에 끼어들면 단언이 깨진다",
    },
    Serialized {
        file: "src/platform/stall_watchdog.rs",
        lock: "GLOBALS",
        acquire: &["GLOBALS"],
        guarded: &["SEQ", "PAUSED"],
        scope: Scope::File,
        why: "워치독의 시퀀스·일시정지 상태가 프로세스 전역이라, 그것을 실제로 바꾸는 \
              테스트끼리 순서에 의존한다",
    },
    Serialized {
        file: "src/webhook/registry.rs",
        lock: "TEST_SERIAL",
        acquire: &["TEST_SERIAL", "serial()"],
        guarded: &["STATE", "sweep"],
        scope: Scope::File,
        why: "웹훅 레지스트리가 프로세스 싱글턴이고 `sweep` 이 만료 엔트리를 **전부** \
              지운다 — 한 테스트의 sweep 이 다른 테스트의 엔트리를 먼저 지운다",
    },
    Serialized {
        file: "crates/tasty-ipc/src/method_meta_tests.rs",
        lock: "TEST_LOCK",
        acquire: &["TEST_LOCK", "test_lock()"],
        guarded: &[
            "register_plugin_prefix",
            "unregister_plugin_prefix",
            "clear_plugin_prefixes_for_tests",
            "plugin_prefixes",
        ],
        scope: Scope::Crate,
        why: "plugin prefix 레지스트리가 런타임 전역이라, 등록과 해제가 겹치면 다른 \
              테스트가 보는 표가 달라진다",
    },
    Serialized {
        file: "crates/tasty-host-plugin/src/manager/tests_namespace_mirror.rs",
        lock: "TEST_LOCK",
        acquire: &["TEST_LOCK", "test_lock()"],
        guarded: &[
            "register_plugin_prefix",
            "unregister_plugin_prefix",
            "clear_plugin_prefixes_for_tests",
        ],
        scope: Scope::Crate,
        why: "위와 **같은 전역**을 다른 크레이트의 테스트 바이너리에서 만진다. 프로세스가 \
              다르므로 락도 따로다 — 한쪽만 잡아서는 이쪽 바이너리가 안 지켜진다",
    },
    Serialized {
        file: "crates/tasty-themes/src/plugin_defaults.rs",
        lock: "TEST_LOCK",
        acquire: &["TEST_LOCK", "reset()"],
        guarded: &["PLUGIN_DEFAULTS", "USER_DEFINED_KINDS"],
        scope: Scope::Crate,
        why: "테마 기본값 전역을 `reset()` 으로 비우고 채우는 형태라, 병렬로 돌면 서로의 \
              초기화가 상대의 단언 중간에 끼어든다",
    },
];

/// 같은 모양이지만 지키는 것이 전역 **변수**가 아닌 락. 명부에는 남기고 규칙에서는 뺀다 —
/// 빠진 것이 아니라 술어가 다르다는 뜻이다(모듈 문서 "무엇을 재고 무엇을 안 재는가").
const OTHER_LOCKS: &[(&str, &str, &str)] = &[
    (
        "src/test_support.rs",
        "TASTY_HOME_ENV_LOCK",
        "`TASTY_HOME` 환경변수. 획득·복원을 `TastyHomeGuard` 가 함께 맡는다",
    ),
    (
        "crates/tasty-host-plugin/src/test_support.rs",
        "HOME_ENV_LOCK",
        "홈 관련 두 환경변수. **자기 채널이 있다** — 같은 모듈의 소스 스캔 테스트가 \
         '두 키를 만지는 유일한 지점이 이 모듈' 을 못박는다",
    ),
    (
        "crates/tasty-settings/src/general.rs",
        "SERIAL",
        "`TASTY_HOME` 을 실제로 만질 수밖에 없는 소수의 테스트(상대 경로 해석 자체가 \
         검증 대상)와 cwd 오염 canary",
    ),
    (
        "crates/tasty-telemetry/src/agent_id.rs",
        "ENV_LOCK",
        "`TASTY_AGENT_ID` 환경변수",
    ),
    (
        "crates/tasty-cli/src/cwd_resolve.rs",
        "CWD_LOCK",
        "프로세스 cwd. **자기 채널이 있다** — `set_current_dir` 재진입 가드가 같은 \
         크레이트에 있다",
    ),
    (
        "tests/attach_common/mod.rs",
        "WRITE_LOCK",
        "attach 소켓의 쓰기 쪽. 전역 변수가 아니라 **하나의 스트림**을 지킨다 — \
         heartbeat 스레드와 본 프레임이 섞이면 프로토콜이 깨진다",
    ),
    (
        "tests/e2e_tests.rs",
        "WINDOW_EXCLUSIVE",
        "창을 만드는 시나리오. 지키는 것이 프로세스 안의 값이 아니라 **GUI 창이라는 \
         프로세스 밖 자원**이라 읽기/쓰기 두 차선으로 가른다",
    ),
];

/// 규칙이 실제로 보는 테스트 수의 하한 — **연기 검사**다. 스캐너가 죽거나 `#[test]` 를
/// 못 자르면 0 이 되고, 0 은 "위반 없음" 으로 읽혀 조용히 통과한다.
///
/// 이 하한이 주장하는 것은 **스캐너의 죽음**뿐이다. "락을 안 잡는 새 테스트가 는다" 는
/// 부류는 수가 **느는** 방향이라 하한이 원리적으로 못 본다 — 그쪽은 아래 규칙이 본다.
/// 값의 근거: 2026-09-05 실측 17.
const MIN_GUARDED_TESTS: usize = 10;

/// `#[test]` 가 붙은 함수의 (이름, 본문). 입력은 마스킹된 소스여야 한다.
fn test_fns(masked: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for at in word_positions(masked, "#[test]") {
        let Some(f) = masked[at..].find("fn ") else {
            continue;
        };
        let after = at + f + 3;
        let Some(paren) = masked[after..].find('(') else {
            continue;
        };
        let name = masked[after..after + paren].trim().to_string();
        let Some(open) = masked[after..].find('{') else {
            continue;
        };
        let start = after + open;
        let mut depth = 0usize;
        let mut end = start;
        for (i, c) in masked[start..].char_indices() {
            match c {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + i;
                        break;
                    }
                }
                _ => {}
            }
        }
        out.push((name, masked[start..end].to_string()));
    }
    out
}

fn crate_root(file: &str) -> String {
    file.split_once('/')
        .filter(|(head, _)| *head == "crates")
        .and_then(|(_, rest)| rest.split_once('/'))
        .map_or_else(|| "src".to_string(), |(name, _)| format!("crates/{name}"))
}

/// 본문이 그 이름을 **단어로** 담는가. 부분 문자열로 보면 `STATE` 가
/// `STATE_POISON_REPORTED` 를 잡는다(실측으로 거짓 양성이 났다).
fn mentions(body: &str, names: &[&str]) -> bool {
    names.iter().any(|n| {
        n.strip_suffix("()").map_or_else(
            || !word_positions(body, n).is_empty(),
            |bare| body.contains(&format!("{bare}(")),
        )
    })
}

#[test]
fn every_test_that_touches_a_serialized_global_holds_its_lock() {
    let mut seen = 0usize;
    let mut violations: Vec<String> = Vec::new();
    let sources = rust_sources();
    for entry in SERIALIZED {
        let root = crate_root(entry.file);
        for (path, text) in &sources {
            let rel = path.to_string_lossy().replace('\\', "/");
            let in_scope = match entry.scope {
                Scope::File => rel == entry.file,
                Scope::Crate => rel.starts_with(&root),
            };
            if !in_scope {
                continue;
            }
            let masked = mask_non_code(text);
            for (name, body) in test_fns(&masked) {
                if !mentions(&body, entry.guarded) {
                    continue;
                }
                seen += 1;
                if !mentions(&body, entry.acquire) {
                    violations.push(format!("{rel}::{name} — `{}` 를 안 잡는다", entry.lock));
                }
            }
        }
    }
    assert!(
        seen >= MIN_GUARDED_TESTS,
        "직렬화 대상 테스트를 {seen} 개밖에 못 찾았다(하한 {MIN_GUARDED_TESTS}, \
         2026-09-05 실측 17). 스캐너가 죽었다 — 0 은 '위반 없음' 이 아니라 측정 실패다"
    );
    assert!(
        violations.is_empty(),
        "프로세스 전역을 만지는 테스트가 그 전역의 직렬화 락을 안 잡는다. 락은 **잡는 \
         쪽끼리만** 막으므로, 하나만 밖에 있어도 그 락이 지키던 직렬화가 통째로 무효가 \
         된다(다른 테스트들이 조용히 flaky 해진다): {violations:#?}"
    );
}

/// 명부가 **모양으로** 완전한가 — 새 직렬화 락은 반드시 이 파일에 들어온다.
///
/// 이름으로 세면 안 모인다(모듈 문서: 이름 4 대 모양 11). 그래서 `static …: Mutex<()>`
/// 선언을 전수로 걷어 명부와 맞댄다. 명부 밖의 락은 규칙이 아무것도 안 보는 자리다.
#[test]
fn every_serialization_lock_is_listed() {
    let listed: BTreeSet<(&str, &str)> = SERIALIZED
        .iter()
        .map(|e| (e.file, e.lock))
        .chain(OTHER_LOCKS.iter().map(|(f, l, _)| (*f, *l)))
        .collect();
    let mut found: BTreeSet<(String, String)> = BTreeSet::new();
    // 이 가드의 대상은 **테스트 자신**이라 모수가 다르다 — 출하 코드만 보면
    // `tests/` 의 통합 테스트가 통째로 안 보이고, 전수 명제가 전수가 아니게 된다.
    for (path, text) in rust_sources_with_integration_tests() {
        let rel = path.to_string_lossy().replace('\\', "/");
        for line in mask_non_code(&text).lines() {
            let t = line.trim();
            let Some(rest) = t.strip_prefix("static ").or_else(|| {
                t.strip_prefix("pub static ")
                    .or_else(|| t.split_once("static ").map(|(_, r)| r))
            }) else {
                continue;
            };
            let Some((name, ty)) = rest.split_once(':') else {
                continue;
            };
            // 모양으로 판정하는 자리라 **모양이 곧 모수**다. `Mutex<()>` 만 보면
            // `RwLock<()>` 로 쓴 락이 안 보인다 — 실측으로 하나 있었다.
            let ty = ty.replace(' ', "");
            if ty.contains("Mutex<()>") || ty.contains("RwLock<()>") {
                found.insert((rel.clone(), name.trim().to_string()));
            }
        }
    }
    assert!(
        found.len() >= listed.len(),
        "직렬화 락을 {} 개밖에 못 찾았다(명부 {}). 스캔이 죽었다",
        found.len(),
        listed.len()
    );
    let missing: Vec<String> = found
        .iter()
        .filter(|(f, l)| !listed.contains(&(f.as_str(), l.as_str())))
        .map(|(f, l)| format!("{f}::{l}"))
        .collect();
    assert!(
        missing.is_empty(),
        "명부에 없는 직렬화 락이 있다. 새 락은 새 전수 명제를 만든다 — 무엇을 지키는지와 \
         그것을 만지는 테스트가 어디까지 있는지를 `SERIALIZED` 에 적어라. 전역 변수가 \
         아니라 환경·cwd 를 지키는 것이면 사유와 함께 `OTHER_LOCKS` 에 적어라: {missing:?}"
    );
}

/// 명부의 이름들이 **실재하는가.** 락이나 접근면이 사라지면 위 규칙은 대상이 없는 채로
/// 초록이 된다 — 이름이 낡는 것과 위반이 없는 것은 다르다.
#[test]
fn each_entry_names_something_that_exists() {
    for entry in SERIALIZED {
        let text = std::fs::read_to_string(repo_root().join(entry.file))
            .unwrap_or_else(|e| panic!("{} 을 읽지 못했다: {e}", entry.file));
        let masked = mask_non_code(&text);
        assert!(
            !word_positions(&masked, entry.lock).is_empty(),
            "{} 에 `{}` 이 없다 — 락이 사라졌거나 이름이 바뀌었다",
            entry.file,
            entry.lock
        );
        assert!(
            entry.why.chars().count() >= 20,
            "`{}` 의 사유가 너무 짧다 — 락마다 다른 경합을 적는 자리다",
            entry.lock
        );
        for name in entry.guarded {
            let root = crate_root(entry.file);
            let anywhere = rust_sources().into_iter().any(|(p, t)| {
                p.to_string_lossy().replace('\\', "/").starts_with(&root)
                    && !word_positions(&mask_non_code(&t), name).is_empty()
            });
            assert!(
                anywhere,
                "`{}` 이 지킨다는 `{name}` 이 {root} 어디에도 없다 — 접근면이 낡았다",
                entry.lock
            );
        }
    }
    for (file, lock, why) in OTHER_LOCKS {
        let text = std::fs::read_to_string(repo_root().join(file))
            .unwrap_or_else(|e| panic!("{file} 을 읽지 못했다: {e}"));
        assert!(
            !word_positions(&mask_non_code(&text), lock).is_empty(),
            "{file} 에 `{lock}` 이 없다"
        );
        assert!(why.chars().count() >= 10, "`{lock}` 의 사유가 비었다");
    }
}
