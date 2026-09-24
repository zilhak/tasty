//! lang의 TOML 파일마다 재빌드 감시를 등록한다. 디렉터리 자체는 감시하지 않는다.
//! 내장 파일은 include_str!의 의존성 추적으로도 감지된다. 여기서는 감시 목록을
//! 실제 파일에서 구하며, 내장 언어 목록과 디스크의 일치는 i18n_key_parity가 검사한다.

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

    // 빈 감시 목록을 성공으로 처리하지 않는다.
    assert!(
        !names.is_empty(),
        "lang/*.toml 파일이 없어 재빌드 감시 목록을 만들 수 없다"
    );

    for name in names {
        println!("cargo:rerun-if-changed=../../lang/{name}");
    }
}
