//! 공개키 두 슬롯을 OUT_DIR에 준비해 bundle_sig가 빌드에 포함할 수 있게 한다.
//! 파일이 없거나 길이가 맞지 않으면 32바이트 zero 값으로 채워 컴파일은 계속한다.
//! release 계열 빌드에서 두 슬롯 모두 zero로 준비되면 경고한다.
//! 키 준비: docs/dev-guide/plugin-packaging.md.

use std::path::{Path, PathBuf};

const KEY_LEN: usize = 32;

fn main() {
    let release_key = Path::new("keys/release-pubkey.bin");
    let dev_key = Path::new("keys/dev-pubkey.bin");

    // 공개키 파일이 없을 수 있으므로 존재하는 keys 디렉터리를 감시한다.
    // 없는 파일 경로를 감시하면 내용 변경이 없어도 빌드 스크립트가 반복 실행된다.
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

    // 32바이트 nonzero 파일이 어느 한쪽에라도 있으면 경고하지 않는다.
    let release_is_zero = read_key(release_key)
        .map(|b| b.iter().all(|x| *x == 0))
        .unwrap_or(true);
    let dev_is_zero = read_key(dev_key)
        .map(|b| b.iter().all(|x| *x == 0))
        .unwrap_or(true);

    if release_is_zero && dev_is_zero {
        println!(
            "cargo:warning=tasty-host-plugin: both release-pubkey.bin and dev-pubkey.bin are \
             placeholders or absent. Prepare the public key matching the bundle signing key. Run \
             scripts/ensure-sign-key.sh (or scripts/gen-dev-key.sh) before building."
        );
    }
}

/// 32바이트 공개키를 복사한다. 파일이 없거나 길이가 다르면 zero 값으로 채운다.
/// 이 대체값은 빌드를 위한 것이며 서명할 개인키에 대응하는 공개키를 대신하지 않는다.
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
