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
//! 순간 컴파일이 깨진다. build.rs 가 "있으면 복사, 없으면 all-zero placeholder
//! 생성" 으로 `OUT_DIR` 슬롯을 항상 채우므로 어떤 빌드 경로(cargo / 스크립트 /
//! CI)에서도 안전하다.
//!
//! `dev-pubkey.bin` 은 `scripts/gen-dev-key.sh`(release/dist 빌드에서는
//! `scripts/ensure-sign-key.sh` 가 호출)가 매 빌드 재도출한다. `release-pubkey.bin`
//! 은 영구 신뢰 루트가 아니다 — 이 슬롯은 항상 placeholder 로 남고, 실제 서명
//! 검증은 dev 슬롯이 담당한다(자세한 배경은 `docs/dev-guide/plugin-packaging.md`,
//! `docs/adr/0051-ephemeral-release-signing-key.md` 참고).
//!
//! ## 2. misconfig 경고
//!
//! release / dist 빌드에서 **두 임베드 슬롯이 전부** zero placeholder 인 채로
//! 산출물이 만들어지지 않도록 `cargo:warning` 으로 안내한다 — dev 슬롯 하나만
//! 유효해도 검증은 정상 동작하므로, release 슬롯 단독 placeholder 는 경고 대상이
//! 아니다(이제 항상 그런 상태이므로). 두 슬롯 다 zero 면 `verify_bundle_signature`
//! 가 `NoValidTrustedKeys` 로 떨어져 builtin plugin 서명 검증이 전부 실패한다.
//! release 빌드에서만 경고 — debug 빌드는 dev workspace bundle 이 unsigned 라
//! placeholder 도 정상.
//!
//! `PROFILE` 환경변수는 cargo 가 build.rs 에 주입하는 것으로, dev 파생 프로필
//! ("dev") 은 "debug", release 파생 ("release", "dist") 은 "release" 가 된다.

use std::path::{Path, PathBuf};

const KEY_LEN: usize = 32;

fn main() {
    let release_key = Path::new("keys/release-pubkey.bin");
    let dev_key = Path::new("keys/dev-pubkey.bin");

    // 파일이 아니라 **디렉토리**를 건다. `keys/release-pubkey.bin` 은 **없는 것이 정상
    // 상태**이고(ADR-0051 — 영구 신뢰 루트를 두지 않는다), 없는 경로에 걸린
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

    // release-pubkey.bin 슬롯은 항상 placeholder 로 남는 게 정상 상태다(영구
    // 신뢰 루트를 두지 않기로 한 정책). 실질적인 검증은 dev 슬롯이 담당하므로,
    // dev 슬롯이 zero 인 경우에만 경고한다 — 이 경우 두 슬롯이 전부 zero 가 되어
    // verify_bundle_signature 가 NoValidTrustedKeys 로 떨어진다.
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
