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
//! **면제도 같은 규칙을 받는다.** 테스트 픽스처는 상태를 손으로 세우는 것이 정상이라
//! 안 보는데, 그 "테스트 전용인가" 를 파일 **이름**(`tests*.rs` · `*_tests.rs`)으로
//! 물었었다. 이름은 성질이 아니다 — 출하되는 파일에 `tests_` 를 붙이면 그 파일은
//! **조용히** 판정에서 사라진다. 지금은 [ADR-0180] 의 판정기 하나
//! (`shipping_scope::test_only_files`: `#[cfg(test)] mod x;` 전이 폐쇄 + cargo 통합
//! 테스트 타깃)를 **부른다**. 같은 물음에 답을 둘로 만들지 않는다.
//!
//! 두 술어가 오늘 갈리는 파일은 90 이고 **전부 한 방향**이다(2026-09-06 실측, 모수
//! 1209): 이름으로는 안 걸리는데 선언상 출하 안 되는 것 90, 반대(이름으로 면제되는데
//! 실제로 출하되는 것)는 0. 즉 이름 술어가 오늘 **가리고 있는 것은 없다** — 이 교체의
//! 값은 위험한 방향이 앞으로도 0 으로 남는 것이 우연이 아니게 만드는 데 있다. 그
//! 우연이 깨지는 조건은 하나다: 출하되는 파일 이름이 `tests` 로 시작하거나 `_tests.`
//! 를 담는 순간.
//!
//! 대신 면제가 커진 만큼 판정 모수는 줄었다(본 파일 1173 → 1083). **면제는 언제나
//! 초록 방향**이라 그 값에 하한을 둔다 — 발견 수가 아니라 **면제 뒤 실제로 본 수**에.
//!
//! [ADR-0180]: ../../docs/adr/0180-test-only-files-is-the-canonical-shipping-judge.md
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
//! 결정·대안·경계는 [ADR-0179](../../docs/adr/0179-the-resolver-is-handed-the-table-not-a-callback.md).
//!
//! ## 이 좁힘이 언제 깨지는가 — 지금 안전한 조건
//!
//! 필드 항목은 **소유 크레이트 안에서만** 본다. 근거는 하나다: 밖에서는 필드가 안
//! 보여 컴파일러가 먼저 막는다. 그 전제는 위 `the_narrowed_scan_rests_on_the_fields_
//! being_private` 가 매번 읽는다.
//!
//! namespace 항목은 사정이 다르다 — 바늘이 필드가 아니라 **창구 함수**
//! (`namespaces_write`)이고, [ADR-0179] 로 표의 `Arc` 가 `tasty-ipc` 로 건너간다.
//! 그래서 이 항목이 지금 안 새는 조건은 셋이고, **그중 둘은 아직 못박혀 있지 않다**:
//!
//! - (가) `PluginManager.ipc_namespaces` 가 private
//!   — `the_narrowed_scan_rests_on_the_fields_being_private`.
//! - (나) `tasty-ipc` 가 보관 중인 `Arc` 를 release 에서 되돌려주지 않는다
//!   — `the_custody_crate_does_not_hand_the_handle_back_out_in_release`.
//! - (다) 소유 크레이트 **밖의 출하 코드**가 그 타입을 이름짓지 않는다
//!   — `the_shared_table_type_is_named_only_where_it_is_owned`.
//!
//! 셋 중 하나라도 깨지면 이 가드는 **빨개지지 않고 조용해진다.** 다른 크레이트가 `Arc` 를
//! 쥐면 쓰기는 `table.write().unwrap().register(…)` 형태가 되는데, 그 줄에는
//! `namespaces_write` 가 없다 — **창구는 이름이고 타입은 성질이다.** 그래서 (나)·(다)는
//! 이름이 아니라 **타입**을 바늘로 쓴다. 조건을 산문으로만 적어 두면 조건이 깨져도
//! 아무것도 빨개지지 않으므로, 셋 다 검사가 말하게 했다.
//!
//! 위 명부의 namespace 항목이 창구 **이름**을 바늘로 쓰는 것은 그대로다 — 그 이름을 안
//! 거치는 경로가 (나)·(다)이고, 그 둘을 세어 못박은 것이 위 두 검사다. 오늘 그 경로의
//! 출하 자리는 0 이고, 0 이 "안 본다" 가 아님은 같은 바늘이 테스트 자리를 집는 것으로
//! 보인다(양성 대조).
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

use std::path::PathBuf;

use tasty_doc_guards::shipping_scope;

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

/// 면제 뒤 **실제로 본** 파일 수의 하한 — 연기 검사. 하한을 발견 수가 아니라 판정 수에
/// 두는 이유는, 면제가 커지는 방향이 **언제나 더 초록**이기 때문이다. 발견 수만 세면
/// 면제가 전부를 삼켜도 통과한다. 값의 근거: 2026-09-06 실측 1083.
const MIN_SHIPPING_SCANNED: usize = 900;

/// 면제된 파일 수의 하한 — 반대 방향의 연기 검사다. 0 이면 면제가 죽은 것이고, 그때
/// 판정은 픽스처를 위반으로 세기 시작한다(시끄러운 실패라 조용하지는 않지만, 판정기가
/// 죽은 것을 판정기 자신이 못 보는 것은 같다). 값의 근거: 2026-09-06 실측 126.
const MIN_TEST_ONLY: usize = 60;

/// 항목별 범위가 admit 해야 하는 **유도 자리 아닌** 파일 수의 하한.
///
/// 2026-09-06 실측 **30**(`crates/tasty-host-plugin` 의 출하 `.rs` 31 중 home 제외).
/// 값의 근거는 "지금 몇 개인가" 가 아니라 **어떤 축소가 몇을 만드는가**다:
/// 범위가 모듈 디렉터리(`crates/tasty-host-plugin/src/manager/`)로 좁아지면 9, home 파일 하나로 좁아지면 0 —
/// 둘 다 변이로 재서 나온 수다(디렉터리의 파일 수를 세서 뺀 값이 아니다. 그렇게
/// 세면 12 가 나오는데, 면제된 파일이 빠지므로 판정이 실제로 보는 수는 9 다).
/// 20 은 그 둘을 모두 빨갛게 하면서 크레이트가 3 분의 1 줄어도 견딘다.
const MIN_PEERS_IN_SCOPE: usize = 20;

/// 필드 선언이 있는 파일 — 전제 검사가 읽는다.
const FIELD_DECL_FILE: &str = "crates/tasty-host-plugin/src/manager.rs";

/// 공유 namespace 표의 타입 이름 — **타입 바늘**이다. 이 이름을 쓸 수 있는 코드는
/// 표를 만들거나 쥐거나 바꿀 수 있다.
const TABLE_TYPE: &str = "IpcNamespaceRegistry";

/// 그 타입을 이름지어도 되는 크레이트: 정의하는 곳과 인스턴스를 소유하는 곳.
const TABLE_TYPE_HOMES: &[&str] = &["crates/tasty-ipc/", "crates/tasty-host-plugin/"];

/// 설치된 표를 보관하는 파일 — 손잡이를 되돌려주는 함수가 생기는지 여기서 본다.
const TABLE_CUSTODY_FILE: &str = "crates/tasty-ipc/src/method_meta.rs";

/// 이름에는 `tests` 가 없는데 선언상 출하되지 않는 실물 파일 — 면제 판정의 한쪽 팔.
const NAME_BLIND_TEST_ONLY_FILE: &str =
    "crates/tasty-doc-guards/tests/filtered_guards_are_not_totally_blind.rs";

/// 항목별 걸러내기가 기대는 **전제**: 유도 상태 필드가 전부 private 이다.
///
/// 필드 항목을 소유 크레이트 안에서만 보는 근거는 "밖에서는 컴파일러가 먼저 막는다"
/// 하나다. 그 전제가 깨지면 걸러내기는 **조용한 구멍**이 된다 — 밖에서 바꿀 수 있는데
/// 밖을 안 보는 상태다. 그래서 전제를 가정하지 않고 **매번 읽는다.**
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
             있는데 판정은 이 크레이트 안만 본다 — 위 항목별 걸러내기를 풀거나 필드를 \
             닫아라: {}",
            decls[0].trim()
        );
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

/// 면제 판정의 **두 팔을 실물로** 못박는다.
///
/// 면제는 언제나 초록 방향이라, 판정이 어느 쪽으로든 미끄러지면 이 가드는 조용해진다.
/// 그래서 합성 픽스처가 아니라 이 레포의 실제 파일 두 종류로 양쪽을 잡는다.
///
/// - **면제되면 안 되는 쪽**: 유도가 사는 파일들(`DERIVED[..].home`). 이들이 면제되면
///   아래 `homes_seen` 대조군이 죽어 판정 전체가 무의미해진다.
/// - **면제돼야 하는 쪽**: 이름에 `tests` 가 안 들어가는데 선언상 출하 안 되는 파일.
///   이 파일이 바로 이름 술어와 선언 술어가 갈리던 자리다(2026-09-06 실측: 갈리는
///   파일 90, 그중 이 가드의 바늘을 담은 것 3 — 전부 이런 형태였다).
#[test]
fn the_exemption_is_pinned_on_both_sides_by_real_files() {
    let root = repo_root();
    let sources = super::rust_sources();
    let test_only = shipping_scope::test_only_files(&root, &sources);

    for d in DERIVED {
        assert!(
            !test_only.contains(&PathBuf::from(d.home)),
            "유도가 사는 {} 가 면제됐다 — 그러면 대조군이 죽는다",
            d.home
        );
    }

    let by_name_not_exempt = PathBuf::from(NAME_BLIND_TEST_ONLY_FILE);
    assert!(
        sources.iter().any(|(p, _)| *p == by_name_not_exempt),
        "{NAME_BLIND_TEST_ONLY_FILE} 이 스캔에 없다 — 옮겼으면 이 상수도 함께 고쳐라. \
         (이 팔이 죽으면 면제가 이름 술어로 되돌아가도 아무도 모른다)"
    );
    let name = NAME_BLIND_TEST_ONLY_FILE
        .rsplit('/')
        .next()
        .unwrap_or(NAME_BLIND_TEST_ONLY_FILE);
    assert!(
        !name.starts_with("tests") && !name.contains("_tests.") && name != "tests.rs",
        "{NAME_BLIND_TEST_ONLY_FILE} 이 이름으로도 걸린다 — 두 술어가 갈리는 자리가 \
         아니라서 대조가 동어반복이 된다. 갈리는 실물을 다시 골라라"
    );
    assert!(
        test_only.contains(&by_name_not_exempt),
        "{NAME_BLIND_TEST_ONLY_FILE} 이 면제 밖이다 — 선언 판정이 이 형태를 놓쳤다"
    );
}

/// 공유 표의 타입을 **이름짓는 출하 자리**는 두 크레이트뿐이다.
///
/// 위 명부의 namespace 항목은 바늘이 **창구 이름**(`namespaces_write`)이다. 이름 바늘은
/// 언제나 "그 이름을 안 거치는 경로" 를 남긴다 — 표의 `Arc` 를 쥔 코드는
/// `table.write().unwrap().register(…)` 로 쓸 수 있고 그 줄에 창구 이름은 없다. 그래서
/// 그 경로를 여기서 **타입으로** 센다(타입 > 경로 > 이름).
///
/// 오늘 그런 출하 자리는 두 크레이트 밖에 **0** 이다. 0 은 "없다" 와 "안 본다" 를 안
/// 가르므로, 같은 바늘이 **테스트 자리는 실제로 집는지**를 같은 판정 안에서 함께 센다 —
/// 그것이 이 검사의 양성 대조다. 두 팔이 같은 needle 을 쓰고 **다른 답**을 낸다.
///
/// 이것이 모듈 문서의 (나)·(다) 를 코드가 말하게 한 것이다. 산문으로만 적어 두면
/// 그 조건이 깨져도 아무것도 빨개지지 않는다.
#[test]
fn the_shared_table_type_is_named_only_where_it_is_owned() {
    let root = repo_root();
    let sources = super::rust_sources();
    let test_only = shipping_scope::test_only_files(&root, &sources);

    let mut shipping_outside: Vec<String> = Vec::new();
    let mut test_only_outside: Vec<String> = Vec::new();
    for (path, src) in &sources {
        let rel = path.to_string_lossy().replace('\\', "/");
        if TABLE_TYPE_HOMES.iter().any(|h| rel.starts_with(h)) {
            continue;
        }
        // 주석과 **문자열 리터럴** 둘 다 가린다. 주석만 걷으면 바로 아래 `TABLE_TYPE`
        // 상수의 리터럴이 이 파일 자신을 집는다 — 그러면 바늘을 죽여도 자기 자신이
        // 계속 잡혀 양성 대조가 **영영 안 터진다**(동어반복). 실제로 그렇게 짰다가
        // 계측에서 잡았다.
        if !super::mask_non_code(src).contains(TABLE_TYPE) {
            continue;
        }
        if test_only.contains(path) {
            test_only_outside.push(rel);
        } else {
            shipping_outside.push(rel);
        }
    }

    assert!(
        !test_only_outside.is_empty(),
        "두 크레이트 밖에서 `{TABLE_TYPE}` 을 이름짓는 자리를 **하나도** 못 찾았다 — \
         테스트 자리조차 안 잡혔다는 뜻이라 아래의 0 은 판정이 아니다.\n\
         세계가 셋이고 이 실패만으로는 안 갈린다: (1) 타입 이름이 바뀌었다 \
         (2) 스캔이나 가리기가 죽었다 (3) 이 대조를 지탱하던 테스트 파일이 없어졌다.\n\
         (3) 이 가장 얇다 — 2026-09-06 실측으로 이 대조를 지탱하던 자리는 **하나**였다 \
         (그 파일 하나의 운명이 판정력 전체를 정한다는 뜻이다). 그때의 옳은 수선은 \
         이 단정을 **지우는 것이 아니라** 다른 테스트 전용 자리를 찾아 대조를 되살리는 \
         것이고, 되살릴 때 **몇 자리가 남았는지 세서 이 문장을 갱신해라.** 지울 거면 \
         아래 판정도 함께 지워라 — 대조 없는 0 은 판정이 아니다"
    );
    assert!(
        shipping_outside.is_empty(),
        "출하되는 코드가 소유 크레이트 밖에서 `{TABLE_TYPE}` 을 이름짓는다. 그러면 그 \
         코드는 표의 손잡이를 쥘 수 있고, 쓰기가 창구(`namespaces_write`)를 안 거치는 \
         형태가 되어 위 판정이 **빨개지지 않고 조용해진다**. 창구를 지나게 하거나, \
         이 판정의 바늘을 타입으로 옮겨라:\n  {}",
        shipping_outside.join("\n  ")
    );
}

/// 보관하는 크레이트가 손잡이를 **release 에서 되돌려주지 않는다.**
///
/// 앞 검사는 두 소유 크레이트 **밖**만 본다. 안쪽에서 새는 형태가 하나 남는데,
/// `tasty-ipc` 가 보관 중인 `Arc` 를 꺼내주는 `pub fn` 이 생기는 것이다. 그러면 그
/// 크레이트를 링크한 누구나 손잡이를 쥐고, 쓰기가 창구를 안 거치게 된다.
///
/// 지금 꺼내는 함수는 하나뿐이고 `#[cfg(test)]` 뒤에 있다. "뒤에 있는가" 는 줄 단위
/// cfg 판정이라 [ADR-0180] 의 판정기를 **부른다** — 속성 문자열을 눈으로 세면
/// `not(test)` 와 `any(test, …)` 두 방향으로 틀린다.
///
/// [ADR-0180]: ../../docs/adr/0180-test-only-files-is-the-canonical-shipping-judge.md
#[test]
fn the_custody_crate_does_not_hand_the_handle_back_out_in_release() {
    let src = std::fs::read_to_string(repo_root().join(TABLE_CUSTODY_FILE))
        .expect("보관 파일을 읽지 못했다 — 옮겼으면 이 상수도 함께 고쳐라");
    let masked = super::mask_non_code(&src).replace("\r\n", "\n");
    let lines: Vec<&str> = masked.lines().collect();
    let gated = tasty_doc_guards::cfg_predicate::cfg_gated_lines(&lines, "test");

    let mut returning: Vec<(usize, bool)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim_start();
        if t.starts_with("fn ") || t.contains(" fn ") {
            if let Some(at) = line.find("->")
                && line[at..].contains(TABLE_TYPE)
            {
                returning.push((i, gated[i]));
            }
        }
    }

    assert!(
        !returning.is_empty(),
        "{TABLE_CUSTODY_FILE} 에서 `{TABLE_TYPE}` 을 돌려주는 함수를 하나도 못 찾았다. \
         못 찾으면 아래 판정은 빈 순회라 그냥 통과한다 — 이 실패는 그 조용한 통과를 \
         막는 자리다.\n\
         세계가 셋이다: (1) 반환 타입을 집는 방식이 깨졌다(`fn` · `->` 매칭) \
         (2) 손잡이를 꺼내는 함수가 이름을 바꿨다 (3) 그런 함수가 정말 없어졌다.\n\
         (3) 이면 이 단정을 그대로 둘 수 없다 — **영영 빨갛고, 영영 빨간 검사는 \
         지워진다.** 그렇다고 지우지도 마라. 옳은 수선은 **대조를 옮기는 것**이다: \
         이 파일에서 반환 타입을 가진 함수를 아무거나 찾는 것으로 매처가 살아 있음을 \
         보이고, 본 판정을 '그 타입을 돌려주는 함수가 0 이다' 로 뒤집어라. 그러면 \
         함수가 다시 생기는 날 여기서 잡힌다"
    );
    let ungated: Vec<String> = returning
        .iter()
        .filter(|(_, g)| !g)
        .map(|(i, _)| format!("{TABLE_CUSTODY_FILE}:{} — {}", i + 1, lines[*i].trim()))
        .collect();
    assert!(
        ungated.is_empty(),
        "보관 중인 표의 손잡이를 release 에서도 꺼낼 수 있다. 그러면 이 크레이트를 \
         링크한 누구나 창구를 안 거치고 표를 바꿀 수 있고, 위 판정은 조용해진다 — \
         `#[cfg(test)]` 뒤로 넣거나, 꺼낼 필요가 없게 만들어라:\n  {}",
        ungated.join("\n  ")
    );
}

/// 유도 상태는 **유도가 사는 파일에서만** 바뀐다.
#[test]
fn derived_plugin_state_is_only_mutated_where_it_is_derived() {
    let root = repo_root();
    let sources = super::rust_sources();
    let test_only = shipping_scope::test_only_files(&root, &sources);
    assert!(
        test_only.len() >= MIN_TEST_ONLY,
        "테스트 전용으로 판정된 파일이 {} 개뿐이다(하한 {MIN_TEST_ONLY}, 발견 {}). \
         면제가 죽으면 픽스처가 위반으로 세진다 — 다만 그 실패는 시끄러운 쪽이라, \
         이 하한이 잡는 것은 **면제가 통째로 죽은 것**뿐이다. 레포가 정말 줄어서 \
         터진 것이면 낮춰도 되고, 그때 잃는 것은 없다(부분 사멸은 이 하한이 원래 \
         안 잡는다)",
        test_only.len(),
        sources.len()
    );

    let mut offenders: Vec<String> = Vec::new();
    let mut homes_seen = vec![false; DERIVED.len()];
    // 항목마다 **범위가 실제로 admit 한 home 아닌 파일 수.** `homes_seen` 만으로는
    // 범위 축소를 못 본다 — home 은 어떤 축소에도 자기 범위 안에 남기 때문이다
    // (2026-09-06 실측: 소유 prefix 를 home 파일 자신으로 바꾸면 `homes_seen` ·
    // `scanned` · `offenders` 가 하나도 안 움직인 채 가드가 진짜 위반을 놓쳤다).
    let mut peers_in_scope = vec![0usize; DERIVED.len()];
    let mut scanned = 0usize;
    for (path, src) in &sources {
        if test_only.contains(path) {
            continue;
        }
        scanned += 1;
        let rel = path.to_string_lossy().replace('\\', "/");
        let stripped = strip_comments(src);
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
            //
            // ★ 그래서 걸러내기는 **항목별**이고, `SCAN_ROOTS` 자체는 안 좁힌다.
            // 범위를 좁히면 자유 함수 부류가 **통째로 조용해진다** — 지금 그 부류가
            // 비어 있어도 마찬가지다. 없는 것에 맞춰 판정기를 좁히면 그것이 돌아올 때
            // 조용해진다.
            if d.field.starts_with('.') {
                let owner = d.home.rsplit_once("/src/").map(|(c, _)| c);
                if let Some(owner) = owner
                    && !rel.starts_with(owner)
                {
                    continue;
                }
            }
            if rel != d.home {
                peers_in_scope[n] += 1;
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

    assert!(
        scanned >= MIN_SHIPPING_SCANNED,
        "면제하고 남은 파일이 {scanned} 개뿐이다(하한 {MIN_SHIPPING_SCANNED}, 발견 {}). \
         두 수를 함께 봐라 — **발견도 같이 줄었으면** 레포가 줄어든 것이라 하한을 \
         내려도 잃는 것이 없고, **발견은 그대로인데 남은 수만 줄었으면** 면제가 판정 \
         범위를 삼킨 것이라 하한이 아니라 면제를 봐야 한다",
        sources.len()
    );

    // ★ 범위가 home 하나로 쪼그라들지 않았는가.
    //
    // 위 `homes_seen` 은 이 방향을 **원리적으로** 못 본다: 범위를 아무리 좁혀도 home
    // 자신은 언제나 그 안에 남으므로, "유도 자리가 보인다" 는 축소를 통과한다. 그래서
    // 대조군을 하나 더 세운다 — 물음이 "자리가 보이는가" 가 아니라 "**자리 아닌 곳도
    // 보는가**" 다.
    for (n, peers) in peers_in_scope.iter().enumerate() {
        if !DERIVED[n].field.starts_with('.') {
            continue; // 자유 함수 부류는 애초에 안 좁힌다 — 좁힘이 없으면 잴 것도 없다.
        }
        assert!(
            *peers >= MIN_PEERS_IN_SCOPE,
            "{} 의 판정 범위에 유도 자리 말고 남은 파일이 {peers} 개뿐이다(하한 \
             {MIN_PEERS_IN_SCOPE}). 이 항목은 소유 크레이트({}) 전체를 봐야 하는데 \
             그보다 좁게 보고 있다. 세계가 둘이고 목록이 아니라 **소유 크레이트에 \
             파일이 몇 개인가**로 갈린다:\n  \
             (1) 그 크레이트에 출하되는 `.rs` 가 아직 많다 → 좁힌 것은 판정기다. \
             `d.home` 이나 소유 prefix 유도(`rsplit_once(\"/src/\")`)를 봐라. \
             하한을 건드리지 마라 — 이 하한이 잡으려던 것이 바로 그 축소다.\n  \
             (2) 그 크레이트가 정말 이 크기로 줄었다 → 그때만 하한을 내린다. 그리고 \
             0 으로는 내리지 마라. 0 이면 이 대조군이 아무것도 안 주장한다.",
            DERIVED[n].what,
            DERIVED[n]
                .home
                .rsplit_once("/src/")
                .map(|(c, _)| c)
                .unwrap_or(DERIVED[n].home),
        );
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
