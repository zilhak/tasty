//! `lang/*.toml` 은 `src/lib.rs` 에서 `include_str!("../../../lang/<code>.toml")` 로
//! 컴파일 타임 임베드된다. lang 파일만 고치고 소스를 안 건드렸을 때 rlib 이 재컴파일되지
//! 않으면 **stale 번역 테이블**이 바이너리에 남고, `t()` 가 새 키를 못 찾아 raw 키
//! 문자열을 그대로 노출한다.
//!
//! # 이 파일이 그 무효화를 **만들지는 않는다** (실측 2026-09-07)
//!
//! rustc 가 `include_str!` 대상을 dep-info 에 이미 기록한다 —
//! `target/debug/deps/tasty_i18n-*.d` 전부에 `lang/{en,ko,ja}.toml` 세 줄이 있다. 그래서
//! lang 파일 하나를 `touch` 하면 이 build script 가 **없어도** rlib 이 다시 만들어진다.
//!
//! # 그런데 지시를 안 내면 더 나빠진다 — 그래서 남긴다
//!
//! 칸 셋 · 팔 셋으로 쟀다(`cargo build -p tasty-i18n`, mtime 을 이름으로 관측):
//!
//! ```text
//!                        무변경 재빌드   ko.toml touch   lang 에 새 파일 신설
//! 현행(파일 셋 감시)      안 움직임       rlib 움직임      안 움직임
//! main() 비움            **rlib 움직임**  rlib 움직임      안 움직임
//! 디렉토리 감시          안 움직임       rlib 움직임      **rlib 움직임**
//! ```
//!
//! - `main()` 을 비우면 `rerun-if-changed` 가 하나도 안 나가고, 그러면 **무변경 빌드마다**
//!   build script 가 다시 돌아 크레이트가 재빌드된다. 그 병은 이 레포가 다른 곳에서 이미
//!   한 번 앓았다(없는 파일을 감시해 무변화 빌드가 37 초 걸리던 자리).
//! - 디렉토리(`../../lang`)를 걸면 **새 파일이 생겼을 뿐인데** 재빌드가 돈다. 새 파일은
//!   `include_str!` 이 집기 전까지 산출물에 안 들어가므로 그 재빌드는 값이 아니라 비용이다.
//!
//! ⇒ 파일 셋을 이름으로 거는 지금 형태가 셋 중 유일하게 **세 팔 모두 옳다.**
//!
//! # 그 목록을 여기 다시 적지는 않는다
//!
//! 한때 여기에 `["en", "ko", "ja"]` 가 손으로 적혀 있었고, `src/lib.rs` 의 목록과 같아야
//! 한다는 것을 지키는 것이 없었다. 넷째 언어를 넣으며 여기를 빠뜨리면 그 언어의 lang
//! 파일만 고쳤을 때 rlib 이 재컴파일되지 않고 **stale 번역 테이블**이 남는다 — 이 파일이
//! 막겠다고 적어 둔 바로 그 실패다.
//!
//! 그래서 `lang/` 에 **실재하는 파일**을 그대로 건다. 위 표의 "디렉토리 감시" 와는 다르다 —
//! 거는 것은 여전히 파일 이름들이고 디렉토리 자체가 아니라서, 새 파일이 생겼다는 것만으로는
//! 재빌드가 돌지 않는다(그 팔의 값은 위에서 이미 쟀다). 목록이 `src/lib.rs` 의 정본과
//! 맞는지는 `tests/i18n_key_parity.rs` 의 `builtin_codes_match_the_language_files_on_disk`
//! 가 정본과 디스크를 견주는 쪽에서 받친다.

use std::path::Path;

fn main() {
    let dir = Path::new("../../lang");
    let entries = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("lang 디렉토리를 못 읽었다 ({}): {e}", dir.display()));

    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".toml"))
        .collect();
    names.sort();

    // 하나도 못 걸면 `rerun-if-changed` 가 전혀 안 나가고, 그러면 **무변경 빌드마다**
    // 이 스크립트가 다시 돌아 크레이트가 재빌드된다(위 표의 "main() 비움" 칸). 조용히
    // 그 상태로 떨어지지 않도록 여기서 멈춘다 — 어차피 `include_str!` 도 못 읽는다.
    assert!(
        !names.is_empty(),
        "lang/*.toml 이 하나도 없다 — 감시할 것이 없으면 무변경 빌드마다 재빌드가 돈다"
    );

    for name in names {
        println!("cargo:rerun-if-changed=../../lang/{name}");
    }
}
