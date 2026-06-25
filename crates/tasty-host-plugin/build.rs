//! Build-time pubkey staging + misconfig 표면화.
//!
//! ## 1. pubkey staging (`stage_key`)
//!
//! `bundle_sig.rs` 의 `TRUSTED_PUBKEYS` 는 `include_bytes!` 로 두 pubkey 를
//! 컴파일타임 임베드한다. 단 소스 트리의 `keys/` 가 아니라 **`OUT_DIR`** 를
//! 참조한다 — 본 build.rs 가 빌드 직전 `OUT_DIR` 로 키를 staging 하기 때문이다.
//!
//! 이렇게 분리하는 이유: `dev-pubkey.bin` 은 개발자별 로컬 키라 추적하지
//! 않는다(`.gitignore`). 추적되지 않으니 새 클론·CI 에는 파일이 없을 수 있는데,
//! `include_bytes!` 가 소스 경로를 직접 가리키면 그 순간 컴파일이 깨진다.
//! build.rs 가 "있으면 복사, 없으면 all-zero placeholder 생성" 으로 `OUT_DIR`
//! 슬롯을 항상 채우므로 어떤 빌드 경로(cargo / 스크립트 / CI)에서도 안전하다.
//! `release-pubkey.bin` 은 신뢰 루트라 추적되며 항상 존재한다.
//!
//! ## 2. misconfig 경고
//!
//! release / dist 빌드에서 임베드 pubkey 가 zero placeholder 인 채로 산출물이
//! 만들어지지 않도록 `cargo:warning` 으로 안내한다. raw 32 byte 가 *모두 zero*
//! 면 publisher 가 실제 keypair 를 생성·커밋하지 않은 상태로, release 산출물의
//! builtin plugin 서명이 어떤 sig 도 통과시키지 못한다. release 빌드에서만 경고
//! — debug 빌드는 dev workspace bundle 이 unsigned 라 placeholder 도 정상.
//!
//! `PROFILE` 환경변수는 cargo 가 build.rs 에 주입하는 것으로, dev 파생 프로필
//! ("dev") 은 "debug", release 파생 ("release", "dist") 은 "release" 가 된다.

use std::path::{Path, PathBuf};

const KEY_LEN: usize = 32;

fn main() {
    let release_key = Path::new("keys/release-pubkey.bin");
    let dev_key = Path::new("keys/dev-pubkey.bin");

    println!("cargo:rerun-if-changed=keys/release-pubkey.bin");
    println!("cargo:rerun-if-changed=keys/dev-pubkey.bin");
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

    match read_key(release_key) {
        Ok(bytes) if bytes.iter().all(|b| *b == 0) => {
            println!(
                "cargo:warning=tasty-host-plugin: release-pubkey.bin is a placeholder (all zeros). \
                 The release build will NOT be able to verify any builtin plugin signature. \
                 Generate a real keypair (scripts/build/gen-dev-key.sh) and replace \
                 crates/tasty-host-plugin/keys/release-pubkey.bin before shipping."
            );
        }
        Ok(_) => {}
        Err(e) => {
            println!(
                "cargo:warning=tasty-host-plugin: failed to read release-pubkey.bin ({e}). \
                 Builtin plugin signature verification may misbehave."
            );
        }
    }

    // dev key 가 zero (또는 부재 → staging 이 zero placeholder) 인 경우는 release
    // 산출물에는 영향 없지만 (release 빌드는 임베드 키 슬롯 2개 중 release-pubkey
    // 만 trust 로 의도), 두 슬롯 다 zero 면 verify 루프 자체가 NoValidTrustedKeys
    // 로 떨어지므로 한 줄 더 경고.
    if read_key(dev_key)
        .map(|b| b.iter().all(|x| *x == 0))
        .unwrap_or(true)
    {
        println!(
            "cargo:warning=tasty-host-plugin: dev-pubkey.bin is a placeholder or absent. \
             Both embed slots may be zero — verify_bundle_signature will return \
             NoValidTrustedKeys for unknown plugins in release."
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
