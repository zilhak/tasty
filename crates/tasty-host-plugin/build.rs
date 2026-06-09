//! Build-time misconfig 표면화 — release / dist 빌드에서 임베드 pubkey 가 zero
//! placeholder 인 채로 산출물이 만들어지지 않도록 cargo:warning 으로 안내한다.
//!
//! 본 검사는 `crates/tasty-host-plugin/keys/release-pubkey.bin` 와 `dev-pubkey.bin`
//! 두 파일을 직접 읽는다. raw 32 byte 가 *모두 zero* 면 publisher 가 실제
//! keypair 를 생성·커밋하지 않은 상태로, release 산출물의 builtin plugin 서명이
//! 어떤 sig 도 통과시키지 못한다. release 빌드에서만 경고 — debug 빌드는 dev
//! workspace bundle 이 unsigned 라 placeholder 도 정상.
//!
//! `PROFILE` 환경변수는 cargo 가 build.rs 에 주입하는 것으로, dev 파생 프로필
//! ("dev") 은 "debug", release 파생 ("release", "dist") 은 "release" 가 된다.
//!
//! 키 파일을 못 읽는 경우 (라이브 개발 환경에서 keys/ 경로 누락 등) 도 동일하게
//! 경고하여 build 자체는 막지 않되 산출물의 신뢰성 결함을 표면화한다.

use std::path::Path;

const KEY_LEN: usize = 32;

fn main() {
    let release_key = Path::new("keys/release-pubkey.bin");
    let dev_key = Path::new("keys/dev-pubkey.bin");

    println!("cargo:rerun-if-changed=keys/release-pubkey.bin");
    println!("cargo:rerun-if-changed=keys/dev-pubkey.bin");
    println!("cargo:rerun-if-env-changed=PROFILE");

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

    // dev key 가 zero 인 경우는 *release 산출물에는 영향 없음* 이지만 (release
    // 빌드는 임베드 키 슬롯 2개 중 release-pubkey 만 trust 로 의도), 두 슬롯 다
    // zero 면 verify 루프 자체가 NoValidTrustedKeys 로 떨어지므로 한 줄 더 경고.
    if let Ok(bytes) = read_key(dev_key)
        && bytes.iter().all(|b| *b == 0)
    {
        println!(
            "cargo:warning=tasty-host-plugin: dev-pubkey.bin is also a placeholder. \
             Both embed slots are zero — verify_bundle_signature will return NoValidTrustedKeys \
             for unknown plugins in release."
        );
    }
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
