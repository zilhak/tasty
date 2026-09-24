//! 빌드한 라이브러리의 지문을 넣어 실행 시 현재 소스와 비교한다.
//! mtime은 내용이 같은 Git 작업에도 달라질 수 있어 사용하지 않는다.
//! 각 바이너리의 소스는 따로 비교하므로 공통 지문에서 src/bin은 제외한다.
//! 지문은 재빌드 누락을 찾기 위한 값이며 위조 방지용 해시가 아니다.

use std::path::PathBuf;

// 빌드 스크립트와 라이브러리가 같은 지문 계산 코드를 사용한다.
include!("src/fingerprint_rule.rs");

fn main() {
    // bin 소스 변경도 재빌드를 유발하지만 공통 지문에는 포함하지 않는다.
    println!("cargo::rerun-if-changed=src");
    let src =
        PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR")).join("src");
    let h = fingerprint(&src).expect("판정기 소스를 읽을 수 없다");
    println!("cargo::rustc-env=TASTY_DOC_GUARDS_LIB_FINGERPRINT={h}");
}
