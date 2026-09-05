//! `PluginManager.packages` 를 바꾸는 자리가 **유도표를 같이 다시 만드는가.**
//!
//! ## 무엇이 실제로 났나
//!
//! `ipc_namespaces`(어느 prefix 를 어느 plugin 이 갖는가)는 [ADR-0173] 이후
//! **설치된 매니페스트에서 유도되는 표**다. 유도는 `PluginManager::refresh_packages`
//! 안에서만 돈다. 그런데 `plugin.remove` 는 그 함수를 안 거치고 `packages` 를 손으로
//! `retain` 했다. 그래서 **지운 plugin 의 prefix 가 표에 남았고**, 그 이름의 호출이
//! `-32002 plugin '<id>' is not running` 으로 거절됐다 — 설치조차 안 돼 있는데.
//! 호스트가 같은 이름에 구현을 갖고 있으면(`image.list`·`image.open`·`markdown.navigate`)
//! 그 구현이 그 상태에서 통째로 가려진다.
//!
//! 실측(2026-09-05, gui 격리 홈): `plugin.remove com.tasty.image` 뒤 `plugin.list` 는
//! 8 개(image 없음)인데 `image.list` 는 `-32002` 를 답했다. 고친 뒤 같은 자리에서
//! `image.open {}` 이 `-32602 missing 'surface_id'` 를 **plugin SDK 의 `host call …
//! failed:` 래퍼 없이** 답한다 — 래퍼의 유무가 "누가 답했나" 를 가른다.
//!
//! ## 모수 — 이름이 아니라 성질로 잡았다
//!
//! 술어는 **"이 값이 다른 것으로부터 계산될 수 있고, 계산 함수가 실재하는가"** 다.
//! `refresh_`/`rebuild_` 같은 이름은 세지 않는다 — 타이머 동기화와 파일 복사가 그
//! 이름을 쓰고, 반대로 유도인데 그 이름이 아닌 것도 있다.
//!
//! 그 술어로 host 의 plugin 상태를 훑으면 캐시된 유도 상태는 다섯이고, 그중 넷이
//! **공개 필드**라 밖에서 직접 바꿀 수 있다(아래 명부). 다섯째
//! (`plugin_permissions`)는 `pub(super)` 라 세터를 거쳐야만 바뀐다 — **캡슐화가
//! 이 부류의 상위 처방**이고, 그래서 명부에 없다. 읽을 때마다 계산하는 것
//! (`plugin_tool_items()` 등)은 낡을 수가 없어 부류 밖이다.
//!
//! ## 무엇을 구조로 닫았고, 무엇을 못 닫았나
//!
//! 가드보다 **닫는 쪽이 싸다** — 잊을 수 있는 규율을 없애는 것이 규율을 지키게 하는
//! 것보다 낫다. 그래서 닫을 수 있는 것은 닫았다(전부 blast radius 를 컴파일러로 재서):
//!
//! | 상태 | 밖에서 필요한 것 | 지금 |
//! |------|------------------|------|
//! | `extensions` | 읽기 2 | private + `extension_state` · `extensions_iter` |
//! | `ipc_namespaces` | 읽기 3(전부 `resolve`) | private + `owns_namespace` · `namespace_belongs_to_other` |
//! | `packages` | 읽기 14 | private + `packages()` |
//! | `plugin_permissions` | 없음 | private (원래 `pub(super)` 였다) |
//!
//! **못 닫았던 하나(`method_meta` 의 prefix 미러)는 이제 없다.** 그것은 필드가 아니라
//! 다른 크레이트(`tasty-ipc`)의 프로세스 전역 **사본**이었고, 쓰기 함수가
//! `tasty-host-plugin` 에서 불려야 해서 `pub` 일 수밖에 없었다(러스트에는 "이 크레이트에만
//! 공개" 가 없다). 닫는 방법은 가시성이 아니라 **사본을 없애는 것**이었다 — `tasty-ipc` 가
//! host 가 든 표의 `Arc` 를 부팅 때 그대로 받는다. 표가 하나면 "두 표가 어긋난다" 는
//! 결함이 존재할 자리가 없고, 미러 쓰기 함수 셋(`register_plugin_prefix` ·
//! `unregister_plugin_prefix` · `doc(hidden) pub clear_plugin_prefixes_for_tests`)이
//! 함께 사라졌다.
//!
//! 주입할 것을 **함수(resolver 클로저)가 아니라 데이터(표 핸들)** 로 고른 것이 핵심이다.
//! 함수를 주입하면 `method_meta()` 안에서 host 코드가 돌아 유도 자리의 `&mut self` 와
//! 겹칠 수 있다(재진입). 데이터면 `method_meta()` 안에서 도는 host 코드가 없다.
//!
//! 그리고 **순서 결함((ㄴ) 부류)은 텍스트로 못 잡는다** — "유도 호출이 원본의 마지막
//! 쓰기 뒤에 오는가" 는 흐름 판정이다. 그 부류는 실행 시점으로 옮겼다:
//! `PluginManager::debug_assert_extensions_fresh` 가 lifecycle 조작 끝에서 유도를
//! 다시 계산해 비교하고, release 에서는 본문이 사라진다. 그 단정이 **실제로 터지는지**는
//! `manager/tests_derived_freshness.rs` 가 `#[should_panic]` 으로 못 박는다.
//!
//! ## 왜 테스트가 아니라 텍스트인가
//!
//! 두 표의 정합은 값으로 물을 수 있지만, **물으려면 그 상태를 만들어야 한다** — 설치된
//! plugin 이 있는 매니저에서 제거를 태워야 하고, 그건 디스크와 프로세스를 요구한다.
//! 반면 "유도를 안 거치고 원본을 바꾼 자리가 있는가" 는 소스로 답이 난다. 실제로 이
//! 결함은 두 조합의 유닛 스위트를 통과했고 실행 확인에서만 드러났다.
//!
//! [ADR-0173]: ../../docs/adr/0173-namespace-resolution-reads-the-manifest-not-the-process-table.md

use std::path::{Path, PathBuf};

use super::{repo_root, strip_comments};

/// 유도 상태 하나 — 밖에서 바꾸면 표가 낡는다.
struct Derived {
    /// 사람이 읽는 이름. 실패 메시지에만 쓴다.
    what: &'static str,
    /// 필드 이름(`.` 포함). 자유 함수 형태면 빈 문자열이고 `verbs` 가 이름 전체다.
    field: &'static str,
    /// 그 필드를 바꾸는 형태. `field` 바로 뒤에 붙는다.
    verbs: &'static [&'static str],
    /// 유도가 사는 파일 — 여기서만 바꿀 수 있다.
    home: &'static str,
}

const HOST_PLUGIN_LIFECYCLE: &str = "crates/tasty-host-plugin/src/manager/lifecycle.rs";

const DERIVED: &[Derived] = &[
    Derived {
        // 이것도 **구조로 닫혔다** — 밖에는 읽기 창구 `packages()` 만 있다. 명부에
        // 남기는 이유는 크레이트 **안**이고, 실제 결함이 났던 자리(`plugin.remove`)는
        // 이제 밖이라 컴파일러가 먼저 막는다.
        what: "설치 목록(디스크에서 재발견된다)",
        field: ".packages",
        verbs: &[
            ".retain(", ".push(", ".clear(", ".remove(", ".insert(", " =",
        ],
        home: HOST_PLUGIN_LIFECYCLE,
    },
    Derived {
        // **구조로 닫혔다** — 크레이트 밖에서는 필드가 안 보이고, 밖이 묻던 것은
        // `owns_namespace` · `namespace_belongs_to_other` 두 물음으로 나간다.
        // 표가 락 뒤로 들어가면서 쓰기는 `namespaces_write()` 하나를 지나야 한다 —
        // 그래서 바늘이 필드 이름이 아니라 **그 창구**다.
        what: "namespace 소유 표(packages 에서 유도)",
        field: "namespaces_write",
        verbs: &["("],
        home: HOST_PLUGIN_LIFECYCLE,
    },
    Derived {
        // 이 필드는 **구조로 닫혔다** — `manager` 모듈 밖에서는 아예 안 보인다(읽기는
        // `extension_state` · `extensions_iter` 로 나간다). 그래서 이 항목이 지키는
        // 범위는 크레이트 **안**뿐이다. 닫을 수 있는 것은 닫고, 가드는 남는 것만 본다.
        what: "확장 집합(packages + config 에서 유도)",
        field: ".extensions",
        verbs: &[".recompute(", " ="],
        home: "crates/tasty-host-plugin/src/manager/queries.rs",
    },
];

/// 필드를 소유한 크레이트. 스캔 범위이자, 아래 전제 검사가 읽는 자리다.
const OWNING_CRATE: &str = "crates/tasty-host-plugin";

/// 스캔 대상 — **필드가 보이는 범위**다.
///
/// 예전에는 `src` 와 `crates` 전체를 훑었다. 그때는 필드가 `pub` 이라 실제로 밖에서
/// 바꿀 수 있었고, 실제 결함도 밖(`src/app/plugin_glue/lifecycle.rs`)에서 났다.
/// 지금은 셋 다 private 이라 **밖에서는 컴파일러가 먼저 막는다** — 그 범위를 텍스트로
/// 또 보는 것은 같은 것을 더 약한 수단으로 다시 보는 일이고, 이름이 같은 남의 지역
/// 필드를 집는다(실측: `crates/tasty-doc-guards` 의 지역 구조체 `out.packages.insert(…)`
/// 가 걸렸다 — plugin 유도 상태와 무관하다).
///
/// **좁히면 모수가 준다**(1400+ → 39). 모수를 줄이는 변경은 언제나 더 초록이므로 이유가
/// 명시적이어야 한다: 뺀 범위는 **더 강한 수단으로 덮여 있다**. 그 전제가 참인지는
/// [`the_narrowed_scan_rests_on_the_fields_being_private`] 가 매번 확인한다 — 누가
/// 필드를 열면 그 테스트가 먼저 빨개지고, 그때 범위를 다시 넓히라고 말한다.
const SCAN_ROOTS: &[&str] = &[OWNING_CRATE];

/// 훑는 `.rs` 파일 수의 하한 — **연기 검사**. 값의 근거: 2026-09-06 실측 39
/// (범위를 소유 크레이트로 좁히기 전에는 1400 이상이었다).
const MIN_SCANNED_FILES: usize = 25;

/// 필드 선언이 있는 파일 — 전제 검사가 읽는다.
const FIELD_DECL_FILE: &str = "crates/tasty-host-plugin/src/manager.rs";

/// 좁힌 스캔이 기대는 **전제**: 유도 상태 필드가 전부 private 이다.
///
/// 이 전제가 깨지면 좁힘은 조용한 구멍이 된다 — 밖에서 바꿀 수 있는데 밖을 안 보는
/// 상태다. 그래서 전제를 가정하지 않고 **매번 읽는다.**
#[test]
fn the_narrowed_scan_rests_on_the_fields_being_private() {
    let src = std::fs::read_to_string(repo_root().join(FIELD_DECL_FILE))
        .expect("필드 선언 파일을 읽지 못했다 — 옮겼으면 이 상수도 함께 고쳐라");
    let masked = super::mask_non_code(&src);
    // 명부의 필드 이름(선행 `.` 을 뗀 것) 중 **필드인 것**만 본다. `namespaces_write`
    // 는 창구 함수라 선언 형태가 다르고, 그것이 private 인지는 여기서 묻지 않는다.
    for field in [
        "packages",
        "ipc_namespaces",
        "extensions",
        "plugin_permissions",
    ] {
        let decls: Vec<&str> = masked
            .lines()
            .filter(|l| {
                let t = l.trim_start();
                t.starts_with(&format!("{field}:"))
                    || (t.starts_with("pub") && t.contains(&format!(" {field}:")))
            })
            .collect();
        assert_eq!(
            decls.len(),
            1,
            "`{field}` 선언을 {} 개 찾았다(1 이어야 한다) — 선언이 옮겨졌으면 이 검사는 \
             아무것도 안 보고 통과한다",
            decls.len()
        );
        assert!(
            !decls[0].trim_start().starts_with("pub"),
            "`{field}` 가 다시 열렸다. 그러면 이 크레이트 **밖**에서도 유도를 우회할 수 \
             있는데 스캔은 이 크레이트만 본다 — `SCAN_ROOTS` 를 다시 넓히거나 필드를 \
             닫아라: {}",
            decls[0].trim()
        );
    }
}

/// 테스트 전용 파일은 안 본다 — 픽스처가 상태를 손으로 세우는 것이 정상이다.
fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    name.starts_with("tests") || name.contains("_tests.") || name == "tests.rs"
}

fn rust_files(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries {
        let entry = entry.expect("디렉터리 항목을 읽을 수 없다");
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// 그 줄이 이 유도 상태를 바꾸는가.
fn mutates(d: &Derived, line: &str) -> bool {
    let Some(at) = line.find(d.field) else {
        return false;
    };
    // 정의는 호출이 아니다 — `pub fn register_plugin_prefix(…)` 를 세면 유도 함수가
    // 사는 크레이트가 영원히 자기 위반이 된다.
    if line[..at].contains("fn ") {
        return false;
    }
    let after = &line[at + d.field.len()..];
    // `.packages_of()` 같은 더 긴 이름을 배제한다.
    if after.starts_with(|c: char| c.is_alphanumeric() || c == '_') {
        return false;
    }
    d.verbs.iter().any(|v| after.starts_with(v))
}

/// 유도 상태는 **유도가 사는 파일에서만** 바뀐다.
#[test]
fn derived_plugin_state_is_only_mutated_where_it_is_derived() {
    let root = repo_root();
    let mut files = Vec::new();
    for r in SCAN_ROOTS {
        rust_files(&root.join(r), &mut files);
    }
    assert!(
        files.len() >= MIN_SCANNED_FILES,
        "훑은 .rs 가 {} 개뿐이다(하한 {MIN_SCANNED_FILES}) — 스캔이 죽으면 아래 판정은 \
         볼 것이 없어 그냥 통과한다",
        files.len()
    );

    let mut offenders: Vec<String> = Vec::new();
    let mut homes_seen = vec![false; DERIVED.len()];
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if is_test_file(&rel) {
            continue;
        }
        let src = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
            .replace("\r\n", "\n");
        let stripped = strip_comments(&src);
        for (n, d) in DERIVED.iter().enumerate() {
            // ★ 필드 항목은 **그 필드를 소유한 크레이트 안에서만** 본다.
            //
            // 위 명부의 주석 셋이 이미 "구조로 닫혔다" 고 적어 뒀다 — 그 필드들은
            // 크레이트 밖에서 아예 안 보이고, 밖의 위반은 가드가 아니라 **컴파일러가**
            // 먼저 막는다. 그래서 밖까지 훑는 것은 판정력을 안 주고 이름 충돌만 산다.
            //
            // 실제로 샀다: `crates/tasty-doc-guards/src/workflow_triggers.rs` 의
            // 무관한 지역 구조체가 `packages` 필드를 갖고 있어 `.packages.insert(` 가
            // 걸렸다. 이름이 같을 뿐 그 표가 아니다 — **이름이 아니라 성질로 판정한다.**
            //
            // 자유 함수 항목(`field` 가 `.` 로 시작하지 않는 것)은 어디서든 부를 수
            // 있어 전 범위를 그대로 훑는다. 그 구분은 이 파일이 이미 쓰던 것이다.
            if d.field.starts_with('.') {
                let owner = d.home.rsplit_once("/src/").map(|(c, _)| c);
                if let Some(owner) = owner
                    && !rel.starts_with(owner)
                {
                    continue;
                }
            }
            let hit = stripped.lines().enumerate().filter(|(_, l)| mutates(d, l));
            if rel == d.home {
                if hit.count() > 0 {
                    homes_seen[n] = true;
                }
                continue;
            }
            for (i, line) in hit {
                offenders.push(format!("{rel}:{} [{}] — {}", i + 1, d.what, line.trim()));
            }
        }
    }

    // 유도 자리에서 아무 변형도 안 보이면 판정이 죽은 것이다 — 이름이 바뀌었거나
    // 자리가 옮겨졌는데 조용히 통과하는 것이 이 부류의 원래 사고다.
    for (n, seen) in homes_seen.iter().enumerate() {
        assert!(
            *seen,
            "{} 의 유도 자리({})에서 변형을 하나도 못 찾았다 — 대조군이 죽었다. \
             자리가 옮겨졌으면 DERIVED 를 같이 고쳐라",
            DERIVED[n].what, DERIVED[n].home
        );
    }

    assert!(
        offenders.is_empty(),
        "유도되는 plugin 상태를 유도가 사는 파일 밖에서 바꾼다. 그러면 표가 낡는다 — \
         실제로 났다: `plugin.remove` 가 설치 목록만 손으로 지워 지운 plugin 의 prefix 가 \
         소유 표에 남았고, 그 이름의 호출이 `-32002 … is not running` 으로 거절됐다 \
         (설치조차 안 돼 있는데). 유도 함수를 불러라.\n  {}",
        offenders.join("\n  ")
    );
}

/// 판정이 실제로 그 형태를 집는다 — 대조군.
///
/// 검사 대상 줄을 **문자열로 조립한다.** 리터럴로 적으면 이 파일이 자기 스캔에 걸려
/// 자기 대조군을 위반으로 센다(실제로 처음에 그렇게 났다). 같은 이유로 다른 가드도
/// needle 을 통째로 안 적는다.
#[test]
fn the_mutation_shapes_are_recognised_and_reads_are_not() {
    let pkgs = &DERIVED[0];
    let f = pkgs.field;
    assert!(mutates(
        pkgs,
        &format!("        mgr{f}.retain(|p| p.id != x);")
    ));
    assert!(mutates(pkgs, &format!("    self{f} = packages;")));
    assert!(
        !mutates(pkgs, &format!("    for pkg in &mgr{f} {{")),
        "읽기를 쓰기로 셌다"
    );
    assert!(
        !mutates(pkgs, &format!("    let n = mgr{f}.len();")),
        "읽기를 쓰기로 셌다"
    );
    assert!(
        !mutates(pkgs, &format!("    mgr{f}_of(id).clear();")),
        "더 긴 이름을 이 필드로 셌다"
    );

    // 나머지 두 항목의 바늘도 여기서 한 번씩 건드린다 — 명부가 늘 때 대조군이
    // 따라오게 하려는 것이다.
    for d in DERIVED.iter().filter(|d| d.field.starts_with('.')) {
        let f = d.field;
        let verb = d.verbs[0];
        assert!(
            mutates(d, &format!("        mgr{f}{verb});")),
            "{} 의 첫 바늘이 안 걸린다",
            d.what
        );
        assert!(
            !mutates(d, &format!("        let x = mgr{f}.len();")),
            "{} 에서 읽기를 쓰기로 셌다",
            d.what
        );
    }
}

/// 주석 안의 같은 형태는 위반이 아니다.
#[test]
fn a_mutation_inside_a_comment_is_not_counted() {
    let d = &DERIVED[0];
    let f = d.field;
    let src = format!("fn f() {{\n    // mgr{f}.retain(|p| true);\n    ok();\n}}\n");
    let stripped = strip_comments(&src);
    assert!(
        !stripped.lines().any(|l| mutates(d, l)),
        "주석 안의 형태를 위반으로 셌다 — 결함을 설명할수록 나빠지는 판정이다"
    );
}

/// 명부의 유도 자리가 전부 실재하는 파일이다.
#[test]
fn every_derivation_home_is_a_real_file() {
    for d in DERIVED {
        assert!(
            repo_root().join(d.home).is_file(),
            "{} 의 유도 자리 {} 가 파일이 아니다",
            d.what,
            d.home
        );
    }
}

/// 이 파일 자신이 스캔에 잡히면 안 되는데, **면제가 아니라 조립으로** 그렇게 한다.
///
/// 면제 목록으로 빼면 이 파일이 나중에 진짜 위반을 들여도 안 보인다. 그래서 여기서는
/// 리터럴을 안 쓰는 쪽을 택했고, 그 사실이 유지되는지를 못 박는다.
#[test]
fn this_file_carries_no_whole_mutation_literal() {
    let me = repo_root().join("src/source_guards/derived_plugin_tables_are_not_bypassed.rs");
    let src = std::fs::read_to_string(&me).expect("이 파일을 읽어야 한다");
    let stripped = strip_comments(&src);
    for d in DERIVED {
        let n = stripped.lines().filter(|l| mutates(d, l)).count();
        assert_eq!(
            n, 0,
            "이 파일이 자기 판정({})에 {n} 줄 걸린다 — 대조군을 리터럴로 적었다면 \
             조립으로 바꿔라",
            d.what
        );
    }
}
