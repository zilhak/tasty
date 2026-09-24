//! 설치 가이드의 Linux 설치 경로·Vulkan 의존 이름과 Windows 설치 경로를 패키징 소스와 대조한다.
//! WiX의 디렉터리 이름은 이어 붙여 경로를 만들고 가이드에서 부분문자열로 찾는다.
//! 접미사가 다른 이름도 통과할 수 있으며 설치·제거 절차의 정확성, macOS 경로, glibc 요구 버전은 검사하지 않는다.
//! 저장소에서 직접 대조할 값이 없는 항목은 이유를 기록한다.
//! doc-guards.yml의 경로 필터 없는 main push·PR에서 실행된다.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/tasty-doc-guards 위로 두 단계가 레포 루트다")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("모수 파일을 읽지 못했다: {} — {e}", p.display()))
}

fn guide() -> String {
    read("site/content/getting-started/install.md")
}

/// 이 검사에서 소스 값과 대조하지 못하는 설치 정보와 이유.
struct NotInAnySource {
    fact: &'static str,
    why: &'static str,
}

const NOT_IN_ANY_SOURCE: &[NotInAnySource] = &[NotInAnySource {
    fact: "GLIBC_2.39",
    why: "glibc 요구 버전은 빌드 환경의 영향을 받으며 이 검사는 패키징 소스에서 그 값을 추출하지 않는다. 러너나 빌드 환경을 바꿀 때는 산출물의 요구 버전을 별도로 확인해야 한다.",
}];

#[test]
fn the_linux_binary_path_the_guide_states_is_the_packaging_destination() {
    let manifest = read("Cargo.toml");

    // deb는 디렉터리 목적지와 원본 파일명을 합치고 rpm은 전체 목적지 경로를 사용하므로 각각 확인한다.
    assert!(
        manifest.contains(r#"["target/release/tasty", "usr/bin/", "755"]"#),
        "`[package.metadata.deb]` 의 실행 파일 asset 줄을 못 찾았다 — deb 설치 위치가 \
         바뀌었으면 가이드도 함께 바뀌어야 한다."
    );
    assert!(
        manifest.contains(r#"dest = "/usr/bin/tasty""#),
        "`[package.metadata.generate-rpm]` 의 `dest = \"/usr/bin/tasty\"` 를 못 찾았다 — \
         rpm 설치 위치가 바뀌었으면 가이드도 함께 바뀌어야 한다."
    );

    assert!(
        guide().contains("/usr/bin/tasty"),
        "패키징이 실행 파일을 `/usr/bin/tasty` 에 놓는데 설치 가이드가 그 경로를 안 적는다."
    );
}

#[test]
fn the_gpu_dependency_names_the_guide_states_are_the_packaging_ones() {
    let manifest = read("Cargo.toml");
    // 배포판별 의존 이름과 recommends·requires 구분을 유지한다.
    for (needle, where_) in [
        (r#"recommends = "libvulkan1""#, "libvulkan1"),
        (r#"vulkan-loader = "*""#, "vulkan-loader"),
    ] {
        assert!(
            manifest.contains(needle),
            "`Cargo.toml` 에서 `{needle}` 을 못 찾았다 — 패키지 이름이 바뀌었으면 \
             가이드의 그 이름도 함께 바뀌어야 한다."
        );
        assert!(
            guide().contains(where_),
            "패키징이 `{where_}` 를 의존으로 적는데 설치 가이드가 그 이름을 안 적는다."
        );
    }
}

/// WiX의 두 디렉터리 Name과 실행 파일명을 이어 Windows 설치 경로의 끝부분을 구한다.
fn wix_exe_path_tail() -> String {
    let wxs = read("wix/main.wxs");
    let name_after = |id: &str| -> String {
        let at = wxs.find(&format!("Id='{id}'")).unwrap_or_else(|| {
            panic!("`wix/main.wxs` 에 `Id='{id}'` 가 없다 — 설치 트리가 바뀌었다")
        });
        let rest = &wxs[at..];
        let n = rest
            .find("Name='")
            .unwrap_or_else(|| panic!("`Id='{id}'` 뒤에 `Name=` 가 없다"));
        let s = &rest[n + "Name='".len()..];
        s[..s.find('\'').expect("닫는 따옴표")].to_string()
    };
    let app = name_after("APPLICATIONFOLDER");
    let bin = name_after("Bin");
    // 고정 파일명을 경로에 붙이기 전에 WiX에 해당 이름이 있는지 확인한다.
    assert!(
        wxs.contains("Name='tasty.exe'"),
        "`wix/main.wxs` 에서 실행 파일 `Name='tasty.exe'` 를 못 찾았다 — 실행 파일 이름이 \
         바뀌었으면 가이드의 Windows 경로도 함께 바뀌어야 한다."
    );
    let exe = "tasty.exe";
    format!("{app}\\{bin}\\{exe}")
}

#[test]
fn the_windows_path_the_guide_states_is_the_wix_name_chain() {
    let tail = wix_exe_path_tail();
    assert_eq!(
        tail, "tasty\\bin\\tasty.exe",
        "WiX 의 `Name=` 사슬이 `{tail}` 로 바뀌었다 — 가이드의 Windows 설치 경로도 함께 바뀌어야 한다."
    );
    assert!(
        guide().contains(&tail),
        "WiX 가 실행 파일을 `...\\{tail}` 에 놓는데 설치 가이드의 경로가 그것과 다르다."
    );
}

#[test]
fn every_fact_without_a_source_is_registered_with_its_reason() {
    // 가이드가 이 사실을 아직 언급하는지와 사유의 최소 길이만 확인한다. 소스에 새 기준이 생겼는지는 자동으로 판정하지 않는다.
    for e in NOT_IN_ANY_SOURCE {
        assert!(
            guide().contains(e.fact),
            "`{}` 이 설치 가이드에 없다 — 사라졌으면 이 등록도 지워라.",
            e.fact
        );
        assert!(
            e.why.split_whitespace().count() >= 10,
            "`{}` 의 사유가 너무 짧다 — 왜 원본이 없는지와 무엇이 그 값을 정하는지 적어라.",
            e.fact
        );
    }
}

#[test]
fn the_guide_page_is_actually_being_read() {
    // 비교 대상 세 파일의 기본 구조가 남아 있는지 확인한다.
    let g = guide();
    assert!(
        g.len() > 2000 && g.contains("## 설치 위치"),
        "설치 가이드가 {} 바이트다 — 페이지가 통째로 옮겨졌으면 위 단정들이 무엇을 \
         대조하는지가 달라진다.",
        g.len()
    );
    assert!(read("Cargo.toml").contains("[package.metadata.deb]"));
    assert!(read("wix/main.wxs").contains("APPLICATIONFOLDER"));
}
