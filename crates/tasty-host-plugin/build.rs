//! Build-time pubkey staging + misconfig 표면화.
//!
//! ## 1. pubkey staging (`stage_key`)
//!
//! `bundle_sig.rs` 의 `TRUSTED_PUBKEYS` 는 `include_bytes!` 로 두 pubkey 를
//! 컴파일타임 임베드한다. 단 소스 트리의 `keys/` 가 아니라 **`OUT_DIR`** 를
//! 참조한다 — 본 build.rs 가 빌드 직전 `OUT_DIR` 로 키를 staging 하기 때문이다.
//!
//! 이렇게 분리하는 이유: `dev-pubkey.bin` / `release-pubkey.bin` 둘 다 로컬
//! 전용 키라 추적하지 않는다(`.gitignore`). 추적되지 않으니 새 클론·CI 에는
//! 파일이 없을 수 있는데, `include_bytes!` 가 소스 경로를 직접 가리키면 그
//! 순간 컴파일이 깨진다. build.rs가 "길이가 맞으면 복사, 없거나 길이가 틀리면 placeholder
//! 생성" 으로 `OUT_DIR` 슬롯을 항상 채우므로 어떤 빌드 경로(cargo / 스크립트 /
//! CI)에서도 안전하다.
//!
//! 서명 스크립트는 선택한 개인키에서 공개키를 도출한다. 개발용 개인키는 기존 파일이
//! 있으면 재사용한다. release 슬롯도 파일이 있고 길이가 맞으면 그대로 포함한다.
//! 키 선택과 준비 절차는 `docs/dev-guide/plugin-packaging.md` 및
//! `docs/dev-guide/release.md#배포-범위와-번들-서명`을 따른다.
//!
//! ## 2. 키 준비 경고
//!
//! release / dist 빌드에서 두 슬롯이 모두 없거나, 길이가 틀리거나, 전부 zero이면
//! `cargo:warning`을 낸다. 두 슬롯 중 하나라도 32바이트의 nonzero 값을 가지면
//! 이 경고는 내지 않는다. 경고는 빌드를 중단하지 않는다.
//! 두 슬롯이 모두 placeholder이면 `verify_bundle_signature`가
//! `NoValidTrustedKeys`를 반환해 번들 서명을 검증하지 못한다.
//! debug 빌드는 서명 없는 개발용 번들을 사용할 수 있어 이 경고를 내지 않는다.
//!
//! `PROFILE` 환경변수는 cargo 가 build.rs 에 주입하는 것으로, dev 파생 프로필
//! ("dev") 은 "debug", release 파생 ("release", "dist") 은 "release" 가 된다.

use std::path::{Path, PathBuf};

const KEY_LEN: usize = 32;

fn main() {
    let release_key = Path::new("keys/release-pubkey.bin");
    let dev_key = Path::new("keys/dev-pubkey.bin");

    // 파일이 아니라 디렉토리를 감시한다. 공개키 파일은 로컬에서 준비하므로 없을 수 있다.
    // 없는 경로에 걸린
    // `rerun-if-changed` 는 cargo 가 이 build script 를 **언제나 stale 로** 본다.
    // 그러면 무변화 cargo 호출마다 host-plugin → cli → tasty 가 다시 컴파일되고 전 타깃이
    // relink 된다. 디렉토리는 실재하고 cargo 가 그 아래를 훑으므로, 두 키의 **내용 변경도
    // 새 키의 등장도** 그대로 잡힌다 — 감시 범위를 줄이지 않고 stale 만 없앤다.
    println!("cargo:rerun-if-changed=keys");
    println!("cargo:rerun-if-env-changed=PROFILE");

    // OUT_DIR 로 두 pubkey 를 staging. include_bytes! 가 이 경로를 참조한다.
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set by cargo"));
    stage_key(release_key, &out_dir.join("release-pubkey.bin"));
    stage_key(dev_key, &out_dir.join("dev-pubkey.bin"));

    let profile = std::env::var("PROFILE").unwrap_or_default();
    let is_release_like = profile == "release";

    if !is_release_like {
        return;
    }

    // 두 슬롯 모두 검증에 쓸 키가 없는 경우만 경고한다. release 또는 dev 어느 슬롯이든
    // 길이가 맞고 zero가 아닌 키가 있으면 아래 조건은 false다.
    let release_is_zero = read_key(release_key)
        .map(|b| b.iter().all(|x| *x == 0))
        .unwrap_or(true);
    let dev_is_zero = read_key(dev_key)
        .map(|b| b.iter().all(|x| *x == 0))
        .unwrap_or(true);

    if release_is_zero && dev_is_zero {
        println!(
            "cargo:warning=tasty-host-plugin: both release-pubkey.bin and dev-pubkey.bin are \
             placeholders or absent. verify_bundle_signature will return NoValidTrustedKeys — \
             builtin plugin signature verification will fail entirely. Run \
             scripts/ensure-sign-key.sh (or scripts/gen-dev-key.sh) before building."
        );
    }
}

/// `src` 가 유효한 32 byte 키면 `dst` 로 복사하고, 없거나 길이가 어긋나면
/// all-zero placeholder 를 `dst` 에 쓴다. placeholder 는 `VerifyingKey` 가
/// 정상 파싱하더라도 어떤 서명도 통과시키지 못하므로 (release 빌드는 위
/// `cargo:warning` 으로 표면화) 안전한 기본값이다.
fn stage_key(src: &Path, dst: &Path) {
    let bytes = match std::fs::read(src) {
        Ok(b) if b.len() == KEY_LEN => b,
        _ => vec![0u8; KEY_LEN],
    };
    std::fs::write(dst, &bytes)
        .unwrap_or_else(|e| panic!("failed to stage pubkey into {}: {e}", dst.display()));
}

fn read_key(path: &Path) -> std::io::Result<Vec<u8>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() != KEY_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("expected {KEY_LEN} bytes, got {}", bytes.len()),
        ));
    }
    Ok(bytes)
}
