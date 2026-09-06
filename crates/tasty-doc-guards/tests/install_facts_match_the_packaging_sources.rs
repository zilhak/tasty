//! 가이드가 적는 **설치 사실**이 패키징 소스와 어긋나지 않는다.
//!
//! # 이 축은 앞서 한 번 잘못 판정했다
//!
//! 2026-09-06 에 "설치 절차 본문은 소스가 WiX 선언과 스크립트 내부 변수이고 가이드는
//! 산문이라 **공통 어휘가 없다**" 로 접었다. 2026-09-07 에 다시 재니 **셋으로 갈렸다** —
//! 어휘가 그대로 같은 것, 한 홉 변환으로 같은 것, 소스에 아예 없는 것. 앞 판정은 셋째
//! 하나를 보고 전체를 덮은 것이었다.
//!
//! | 가이드가 적는 것 | 소스 | 어휘 |
//! |---|---|---|
//! | `/usr/bin/tasty` | `Cargo.toml` 의 deb·rpm `assets` 의 `dest` | **그대로** |
//! | `libvulkan1` | `[package.metadata.deb]` 의 `recommends` | **그대로** |
//! | `vulkan-loader` | `[package.metadata.generate-rpm.requires]` 의 키 | **그대로** |
//! | `C:\Program Files\tasty\bin\tasty.exe` | `wix/main.wxs` 의 `Name=` 사슬 | **한 홉**(사슬을 잇는다) |
//! | `GLIBC_2.39` | 없다 | — CI 러너 이미지가 정한다. 저장소 문자열 0 |
//!
//! 앞 넷을 잰다. 다섯째는 아래 `NOT_IN_ANY_SOURCE` 에 사유와 함께 등록한다 — 모수에서
//! 빼면 "그런 물음이 있었다" 는 것까지 사라진다.
//!
//! # 왜 놓아도 되는가
//!
//! 이 축은 **사실 대응**이지 표기 선택이 아니다. 단축키 축에서 가드를 접은 이유는 설정
//! 어휘(`alt+up`)와 읽는 표기(`Alt+↑`)가 서로 다른 두 어휘이고 가이드 쪽이 사람을 위해
//! 일부러 다르기 때문인데, 설치 경로에는 그런 두 어휘가 없다. `/usr/bin/tasty` 는 하나뿐이고
//! 어긋나면 그냥 틀린 것이다. ⇒ 빨개졌을 때 가장 싼 초록화가 **가이드를 참값으로 고치는 것**
//! 이고, 그건 보호 대상을 안 깎는다.
//!
//! # 안 덮는 것 (재고 적는다)
//!
//! - **부분문자열이다.** 가이드가 `vulkan-loader-dev` 라고 적어도 `vulkan-loader` 를 품으므로
//!   통과한다. 변이로 확인했다 — `vulkan-loader` 를 `vulkan-loaderX` 로 바꾼 변이는 **안 죽었고**,
//!   `vulkan-svc` 로 바꾼 변이는 죽었다. 낱말 경계를 보게 만들 수 있지만 가이드가 그 이름을
//!   문장 안에서 어떻게 감싸는지가 자유로워서 경계 규칙이 곧 표기 규칙이 된다 — 그건 단축키
//!   축에서 접은 것과 같은 압력이다.
//! - **설치 *절차*(순서·명령)는 안 본다.** `sudo apt remove tasty` 같은 줄의 원본은 패키지
//!   이름 하나뿐이고 나머지는 배포판 관례라 저장소에 없다. 이 가드가 보는 것은 절차가 아니라
//!   **설치 사실**(어디에 놓이나 · 무엇에 의존하나)이다.
//! - **macOS 경로**(`/Applications/Tasty.app/Contents/MacOS/tasty`)는 아직 안 든다. 원본이
//!   `scripts/build-macos-dmg.sh` 의 번들 조립과 `scripts/install-macos.sh` 의 복사 대상에
//!   흩어져 있어 한 홉이 아니라 여러 홉이다. 잴 수 있는지부터 다시 재야 한다.
//!
//! # 채널
//!
//! `doc-guards.yml` — main push · PR 마다 경로 필터 없이 돈다.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/tasty-doc-guards 위로 두 단계가 레포 루트다")
        .to_path_buf()
}

/// 읽기 실패는 건너뛰지 않는다 — 파일이 사라지면 모수가 조용히 비고, 그때의 "미스 0" 은
/// 일치했다는 뜻이 아니라 아무것도 안 읽었다는 뜻이다.
fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("모수 파일을 읽지 못했다: {} — {e}", p.display()))
}

fn guide() -> String {
    read("site/content/getting-started/install.md")
}

/// 소스에 원본이 없는 설치 사실과 그 사유. **모수에서 빼지 않고 여기 적는다** — 빼면
/// 그런 물음이 있었다는 것까지 사라진다.
struct NotInAnySource {
    fact: &'static str,
    why: &'static str,
}

const NOT_IN_ANY_SOURCE: &[NotInAnySource] = &[NotInAnySource {
    fact: "GLIBC_2.39",
    why: "glibc 하한은 저장소가 아니라 **빌드 러너 이미지**가 정한다(Ubuntu 24.04). \
          저장소 전체에서 이 문자열은 가이드 밖에 0 건이라 대조할 원본이 없다 — \
          러너를 올리면 이 숫자가 낡지만 그것을 잡는 채널은 이 가드가 아니다.",
}];

#[test]
fn the_linux_binary_path_the_guide_states_is_the_packaging_destination() {
    let manifest = read("Cargo.toml");

    // ★ 두 패키지 형식이 **같은 사실을 다른 문법으로** 적는다. 하나만 보면 다른 하나가
    //   바뀌어도 조용하다.
    //   deb  — 배열형. 목적지가 **디렉토리**(`usr/bin/`, 앞 슬래시 없음)라 파일 이름은
    //          소스 쪽 `target/release/tasty` 에서 온다. 이어야 `/usr/bin/tasty` 가 된다.
    //   rpm  — 표형. 목적지가 전체 경로라 그대로 읽힌다.
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
    // 배포판마다 패키지 이름이 다르다 — deb 는 recommends, rpm 은 requires 키.
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

/// WiX 의 `Name=` 사슬을 이어 실행 파일의 설치 경로 꼬리를 만든다.
///
/// 이것이 **한 홉 변환**이다. 소스에는 `tasty` · `bin` · `tasty.exe` 가 따로 있고 가이드에는
/// 이어진 경로가 있다 — 대조하려면 잇는 규칙을 여기 적어 둬야 한다. 규칙을 안 적고 통짜
/// 문자열로 찾으면 소스에 없는 것을 없다고 보고하게 된다.
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
    // 실행 파일 이름은 그 아래 `File` 의 Name 이다. 사슬을 잇기 전에 그것이 실재하는지
    // 본다 — 없으면 아래 `format!` 이 소스에 없는 경로를 지어내고, 그 경로가 가이드와
    // 안 맞는다는 보고는 참이지만 이유가 거짓이 된다.
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
    // 등록된 사실은 가이드에는 있고 소스에는 없어야 한다. 소스에 생겼으면 등록을 지우고
    // 위처럼 대조로 올려라 — 안 그러면 잴 수 있게 된 것을 계속 못 잰다고 적어 두게 된다.
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
    // 위 단정들은 전부 `guide().contains(...)` 다. 파일이 비면 전부 빨개지긴 하지만,
    // 반대로 **모수 쪽**(Cargo.toml · main.wxs)이 비면 조용해질 수 있는 자리가 남는다.
    // 세 파일이 다 실물인지 여기서 한 번 본다.
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
