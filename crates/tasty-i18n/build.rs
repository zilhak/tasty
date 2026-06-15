//! `lang/*.toml` 은 `src/lib.rs` 에서 `include_str!("../../../lang/<code>.toml")`
//! 로 컴파일 타임 임베드된다. 이 경로들은 크레이트 디렉토리 **밖**(워크스페이스
//! 루트)에 있어 cargo 가 변경을 신뢰성 있게 추적하지 못한다 — lang 파일만 고치고
//! 소스를 안 건드리면 rlib 이 재컴파일되지 않아 **stale 번역 테이블**이 바이너리에
//! 남고, `t()` 가 새 키를 못 찾아 raw 키 문자열을 그대로 노출한다.
//!
//! 각 lang 파일에 명시적 `rerun-if-changed` 를 걸어 변경 시 반드시 재컴파일되게 한다.
//! (경로는 `CARGO_MANIFEST_DIR` = `crates/tasty-i18n` 기준 상대 경로.)

fn main() {
    for code in ["en", "ko", "ja"] {
        println!("cargo:rerun-if-changed=../../lang/{code}.toml");
    }
}
