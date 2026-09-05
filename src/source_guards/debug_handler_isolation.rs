//! debug 핸들러가 **선언의 cfg 로** 격리돼 있는지 본다.
//!
//! [debug-ipc](../../docs/dev-guide/debug-ipc.md) "디버그 코드 격리 정책" 의 판별식은
//! 파일 이름도 디렉토리도 아니다 — *"그 파일이 debug 핸들러만 담고 **모듈 선언에 cfg 가
//! 붙어 있는가**"* 다. 그래서 이 가드도 이름을 안 본다. 보는 것은 두 가지뿐이다:
//! `handler.rs` 의 선언이 그 모듈을 debug 로 게이트하는가, 그리고 그 파일 안에
//! debug 전용 item 이 있는가.
//!
//! # 무엇이 거짓이면 실패하는가
//!
//! **게이트되지 않은 핸들러 파일에 `#[cfg(debug_assertions)]` item 이 하나라도 있으면**
//! 실패한다. 그 형태가 격리를 깨는 방식이라서다 — 정책의 삭제 가능성 테스트("이 코드를
//! 통째로 지우고 컴파일 에러 몇 줄만 정리하면 디버그 기능이 깨끗이 사라지는가")가 파일
//! 단위로 성립하지 않게 된다. 일반 핸들러 안에 끼운 debug fn 은 그 파일을 지울 수 없게
//! 만들고, 그러면 지울 수 있는 것은 fn 하나씩이 된다.
//!
//! # 왜 이 규칙을 지금 세우는가
//!
//! CLAUDE.md 와 identity.md 가 오랫동안 *"debug 코드는 `debug/` 디렉토리로 모은다"* 고
//! 적고 있었는데 그 디렉토리는 존재한 적이 없고, debug-ipc.md 는 같은 대상에 대해 다른
//! 곳을 가리키고 있었다. 두 문장을 debug-ipc.md 에 맞춰 정리했으니, 정본이 된 그 규칙에
//! 채널을 붙인다 — **채널 없는 규칙이 거짓이 되는 데 걸린 시간이 이 축의 관측이다.**
//!
//! # 사거리
//!
//! 핸들러 디렉토리만 본다. `src` 전체로 넓히지 않는 이유는 규칙 자체가 *핸들러*에 대한
//! 것이기 때문이다 — 라우터(`handler.rs`)의 dispatch 팔, 창 수명주기, 플랫폼 코드에도
//! `#[cfg(debug_assertions)]` 는 정당하게 있고(실측: 각각 14 · 6 · 6), 그것들을 같은
//! 술어로 재면 참인 명제가 없다. 넓힌 술어를 세우려면 "핸들러인가" 를 디렉토리 밖에서도
//! 판정할 수단이 먼저 있어야 한다.
//!
//! 그래서 **`src/view/main/debug_input.rs` 는 이 술어의 대상이 아니다.** debug 전용
//! 파일이지만 IPC 핸들러가 아니라 view 입력이고, 규칙이 말하는 대상이 아니다 — 빠진
//! 것이 아니라 범위 밖이라는 뜻이다(적어 두지 않으면 다음 사람이 빠뜨린 것으로 읽고
//! 다시 센다).
//!
//! 그리고 이 스캔은 **선언을 텍스트로 읽는다.** 매크로가 `mod` 선언을 만들면 어떻게
//! 되는지는 변이로 쟀다(둘이 다르게 나온다):
//!
//! - 매크로 **본문에 선언이 그대로 적혀 있으면**(`() => { #[cfg(debug_assertions)]
//!   pub(crate) mod debug_plugin; }`) 스캔이 그 줄을 여전히 읽어 게이트를 본다 — 초록이고,
//!   게이트가 실제로 걸려 있으니 그 초록이 맞다.
//! - 매크로가 **이름을 인자로 받으면**(`decl_debug_mod!(debug_plugin)`) 선언이 안 보여
//!   그 파일은 게이트 안 된 것으로 세어지고, debug item 이 하나라도 있으면 **빨개진다.**
//!
//! 즉 이 스캔이 선언을 잃는 방향은 조용한 통과가 아니라 거짓 양성이다. 그리고 위 하한은
//! 이 부류를 보지 못한다 — 하한이 세는 것은 디렉토리의 파일 수라, 선언을 어떻게 적든
//! 안 변한다. 하한의 일은 스캔이 죽었는지를 보는 것뿐이다.
//!
//! # 두 조건이 지금은 겹친다
//!
//! "선언이 게이트했다" 와 "파일 자신이 `#![cfg(debug_assertions)]` 이다" 를 둘 다 인정
//! 한다. 오늘 트리에서는 둘이 같은 답을 낸다 — debug item 을 가진 두 파일이 양쪽을 다
//! 갖고 있고, 선언만 게이트된 셋(`debug_plugin` · `input_source` · `ime`)은 item 이 0 이다.
//! 그래도 선언 쪽을 먼저 읽는 이유는 그것이 **정책이 적은 판별식**이기 때문이다. 겹침은
//! 오늘의 사실이지 규칙이 아니고, `debug_plugin.rs` 에 debug item 을 하나 넣어 보면
//! 선언 경로만이 그것을 정당한 것으로 판정한다(변이로 확인).
//!
//! 그리고 **`#[cfg(not(debug_assertions))]`(release 전용)는 대상이 아니다.** 실제로
//! `workspace.rs` 에 넷 있는데, release 에서만 도는 안전 검사이지 debug 기능이 아니다.
//! 처음 재던 정규식이 그 넷을 위반으로 셌다 — `not(` 을 안 봤기 때문이다.

use std::collections::BTreeMap;

use super::{mask_non_code, repo_root};

const DISPATCH: &str = "src/adapters/ipc/handler.rs";
const HANDLER_DIR: &str = "src/adapters/ipc/handler";

/// 스캔이 죽었는지 보는 하한. 2026-09-05 실측 45 파일.
const MIN_HANDLER_FILES: usize = 30;

/// 모듈 이름 → 그 선언이 debug 게이트인가. `handler.rs` 의 선언만 읽는다.
fn declared_debug_gated(dispatch: &str) -> BTreeMap<String, bool> {
    let masked = mask_non_code(dispatch);
    let mut out = BTreeMap::new();
    let mut pending: Option<bool> = None;
    for line in masked.lines() {
        let t = line.trim();
        if t.starts_with("#[cfg(") {
            // 같은 선언에 여러 줄로 붙은 속성은 하나라도 debug 면 debug 다.
            let gated = t.contains("debug_assertions") && !t.contains("not(debug_assertions)");
            pending = Some(pending.unwrap_or(false) || gated);
            continue;
        }
        if let Some(name) = module_decl(t) {
            out.insert(name, pending.take().unwrap_or(false));
            continue;
        }
        if !t.is_empty() {
            pending = None;
        }
    }
    out
}

/// `mod x;` / `pub mod x;` / `pub(crate) mod x;` 한 줄에서 모듈 이름. 중괄호가 붙은
/// 인라인 모듈(`mod x {`)은 파일이 아니라 대상이 아니다.
fn module_decl(line: &str) -> Option<String> {
    let rest = line.strip_suffix(';')?;
    let rest = rest.strip_prefix("pub(crate) ").unwrap_or(rest);
    let rest = rest.strip_prefix("pub ").unwrap_or(rest);
    let name = rest.strip_prefix("mod ")?.trim();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    Some(name.to_string())
}

/// 마스킹된 소스에서 **debug 전용** 속성이 붙은 줄 번호. `not(debug_assertions)` 는
/// release 전용이라 대상이 아니고, 파일 수준 inner 속성(`#![cfg(...)]`)도 아니다.
fn debug_only_items(masked: &str) -> Vec<usize> {
    masked
        .lines()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim();
            t.starts_with("#[cfg(")
                && t.contains("debug_assertions")
                && !t.contains("not(debug_assertions)")
        })
        .map(|(i, _)| i + 1)
        .collect()
}

/// 파일 자신이 통째로 debug 인가(`#![cfg(debug_assertions)]`).
fn file_level_debug(masked: &str) -> bool {
    masked
        .lines()
        .any(|l| l.trim().starts_with("#![cfg(") && l.contains("debug_assertions"))
}

#[test]
fn a_debug_handler_is_isolated_by_its_declaration_not_its_name() {
    let root = repo_root();
    let dispatch = std::fs::read_to_string(root.join(DISPATCH))
        .unwrap_or_else(|e| panic!("{DISPATCH} 를 읽을 수 없다: {e}"));
    let gated = declared_debug_gated(&dispatch);

    let dir = root.join(HANDLER_DIR);
    let mut scanned = 0usize;
    let mut violations: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("핸들러 디렉토리를 읽어야 한다") {
        let path = entry.expect("디렉토리 항목").path();
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("파일 이름")
            .to_string();
        let src = std::fs::read_to_string(&path).expect("핸들러 파일을 읽어야 한다");
        let masked = mask_non_code(&src);
        scanned += 1;

        // 선언이 게이트했거나 파일 자신이 통째로 debug 면 debug 핸들러 파일이다.
        if *gated.get(&stem).unwrap_or(&false) || file_level_debug(&masked) {
            continue;
        }
        let lines = debug_only_items(&masked);
        if !lines.is_empty() {
            violations.push(format!("{HANDLER_DIR}/{stem}.rs: {lines:?}"));
        }
    }

    assert!(
        scanned >= MIN_HANDLER_FILES,
        "핸들러 파일을 {scanned} 개밖에 못 읽었다(하한 {MIN_HANDLER_FILES}, 2026-09-05 \
         실측 45). 스캔이 죽었다 — 0 은 통과가 아니라 측정 실패다"
    );
    assert!(
        violations.is_empty(),
        "게이트되지 않은 핸들러 파일 안에 debug 전용 item 이 있다. 그 파일은 이제 통째로 \
         지울 수 없어져 격리의 삭제 가능성 테스트가 파일 단위로 안 선다. `#![cfg(debug_assertions)]` \
         만 건 형제 모듈로 빼거나, 모듈 선언에 cfg 를 걸어라(docs/dev-guide/debug-ipc.md \
         \"디버그 코드 격리 정책\"): {violations:?}"
    );
}

#[cfg(test)]
mod detector {
    use super::*;

    #[test]
    fn it_reads_the_gate_from_the_declaration() {
        let d = declared_debug_gated(
            "mod plain;\n\
             #[cfg(debug_assertions)]\n\
             mod dbg;\n\
             #[cfg(all(debug_assertions, feature = \"gui\"))]\n\
             pub mod dbg_gui;\n\
             #[cfg(feature = \"gui\")]\n\
             pub(crate) mod gui_only;\n",
        );
        assert_eq!(d.get("plain"), Some(&false));
        assert_eq!(d.get("dbg"), Some(&true));
        assert_eq!(d.get("dbg_gui"), Some(&true));
        assert_eq!(d.get("gui_only"), Some(&false));
    }

    #[test]
    fn a_release_only_attribute_is_not_a_debug_item() {
        assert!(debug_only_items("#[cfg(not(debug_assertions))]\nfn f() {}").is_empty());
        assert_eq!(
            debug_only_items("#[cfg(debug_assertions)]\nfn f() {}"),
            vec![1]
        );
    }

    #[test]
    fn a_file_level_attribute_is_not_an_item() {
        let src = "#![cfg(debug_assertions)]\nfn f() {}";
        assert!(file_level_debug(src));
        assert!(debug_only_items(src).is_empty());
    }

    /// 주석·문자열 안의 같은 글자를 코드로 세지 않는다 — 이 파일 자신이 그 글자를
    /// 문서에 담고 있어서, 마스킹이 없으면 가드가 자기 자신을 잡는다.
    #[test]
    fn the_scan_separates_code_from_comments_and_literals() {
        let masked = mask_non_code(
            "// #[cfg(debug_assertions)]\n\
             let s = \"#[cfg(debug_assertions)]\";\n\
             #[cfg(debug_assertions)]\n",
        );
        assert_eq!(debug_only_items(&masked), vec![3]);
    }

    /// 인라인 모듈은 파일이 아니라 선언 대상이 아니다.
    #[test]
    fn an_inline_module_is_not_a_file_declaration() {
        assert_eq!(module_decl("mod x {"), None);
        assert_eq!(module_decl("mod x;"), Some("x".to_string()));
    }
}
