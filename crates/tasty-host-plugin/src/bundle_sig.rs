//! bundle manifest 의 ed25519 detached signature 검증.
//!
//! 보호 대상: bundle 디렉토리 안의 `tasty-plugin.toml` 한 파일. 같은 디렉토리의
//! `tasty-plugin.toml.sig` (raw 64-byte signature) 사이드카로 확인한다. 매니페스트
//! 안에 권한·contributes 가 들어있으므로 *변조 시 confused-deputy 차단* 이 핵심
//! 목표다. 디렉토리 전체 hash 서명은 0.7 이후.
//!
//! release 빌드에서는 `upgrade_builtins` / `install_builtins_if_needed` 가 본
//! verify 를 호출해서 실패 시 해당 plugin 을 `Skipped { signature-invalid }` 로
//! 차단한다. debug 빌드는 dev workspace bundle 이 unsigned 라 warn 로깅 후 통과.

use std::path::Path;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

/// Tasty release bundle 의 공식 ed25519 공개키 (32 byte).
///
/// 현재는 placeholder (zeroed). release pipeline 의 `scripts/sign-bundle.sh`
/// (E.3.5, J.E 범위 밖) 가 완성되면 그 시점에 실키로 교체. 키 rotation /
/// trust store 는 0.7 이후.
pub const TASTY_BUNDLE_PUBKEY: [u8; 32] = [0u8; 32];

#[derive(Debug)]
pub enum SigVerifyError {
    SidecarMissing,
    SidecarReadError(std::io::Error),
    ManifestReadError(std::io::Error),
    InvalidSignatureLength,
    InvalidPublicKey,
    VerificationFailed,
}

impl std::fmt::Display for SigVerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SidecarMissing => write!(f, "tasty-plugin.toml.sig sidecar missing"),
            Self::SidecarReadError(e) => write!(f, "sidecar read error: {e}"),
            Self::ManifestReadError(e) => write!(f, "manifest read error: {e}"),
            Self::InvalidSignatureLength => write!(f, "signature length != 64 bytes"),
            Self::InvalidPublicKey => write!(f, "embedded TASTY_BUNDLE_PUBKEY is invalid"),
            Self::VerificationFailed => write!(f, "ed25519 verification failed"),
        }
    }
}

/// `<dir>/tasty-plugin.toml` 의 sha256 을 `<dir>/tasty-plugin.toml.sig` (raw
/// 64-byte ed25519 signature) 로 검증.
pub fn verify_bundle_signature(dir: &Path) -> Result<(), SigVerifyError> {
    let manifest_path = dir.join("tasty-plugin.toml");
    let sig_path = dir.join("tasty-plugin.toml.sig");
    if !sig_path.exists() {
        return Err(SigVerifyError::SidecarMissing);
    }
    let manifest_bytes =
        std::fs::read(&manifest_path).map_err(SigVerifyError::ManifestReadError)?;
    let sig_bytes = std::fs::read(&sig_path).map_err(SigVerifyError::SidecarReadError)?;
    if sig_bytes.len() != 64 {
        return Err(SigVerifyError::InvalidSignatureLength);
    }
    let digest = Sha256::digest(&manifest_bytes);

    let vk = VerifyingKey::from_bytes(&TASTY_BUNDLE_PUBKEY)
        .map_err(|_| SigVerifyError::InvalidPublicKey)?;
    let sig_array: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| SigVerifyError::InvalidSignatureLength)?;
    let sig = Signature::from_bytes(&sig_array);
    vk.verify(digest.as_slice(), &sig)
        .map_err(|_| SigVerifyError::VerificationFailed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use tempfile::TempDir;

    fn write_manifest(dir: &Path, content: &str) {
        std::fs::write(dir.join("tasty-plugin.toml"), content).unwrap();
    }

    fn write_sig(dir: &Path, bytes: &[u8]) {
        std::fs::write(dir.join("tasty-plugin.toml.sig"), bytes).unwrap();
    }

    fn sign_digest(sk: &SigningKey, manifest: &[u8]) -> Vec<u8> {
        let digest = Sha256::digest(manifest);
        sk.sign(digest.as_slice()).to_bytes().to_vec()
    }

    /// 임시 SigningKey 로 서명한 후, 그 keypair 의 pubkey 로 *직접 검증* 만 점검.
    /// `TASTY_BUNDLE_PUBKEY` 상수는 release 까지 placeholder 라 본 함수와 분리.
    fn verify_with_custom_key(dir: &Path, vk: &VerifyingKey) -> Result<(), SigVerifyError> {
        let manifest_path = dir.join("tasty-plugin.toml");
        let sig_path = dir.join("tasty-plugin.toml.sig");
        if !sig_path.exists() {
            return Err(SigVerifyError::SidecarMissing);
        }
        let manifest_bytes =
            std::fs::read(&manifest_path).map_err(SigVerifyError::ManifestReadError)?;
        let sig_bytes = std::fs::read(&sig_path).map_err(SigVerifyError::SidecarReadError)?;
        if sig_bytes.len() != 64 {
            return Err(SigVerifyError::InvalidSignatureLength);
        }
        let digest = Sha256::digest(&manifest_bytes);
        let sig_array: [u8; 64] = sig_bytes
            .as_slice()
            .try_into()
            .map_err(|_| SigVerifyError::InvalidSignatureLength)?;
        let sig = Signature::from_bytes(&sig_array);
        vk.verify(digest.as_slice(), &sig)
            .map_err(|_| SigVerifyError::VerificationFailed)
    }

    #[test]
    fn verify_with_valid_signature() {
        let seed = [7u8; 32];
        let sk = SigningKey::from_bytes(&seed);
        let vk = sk.verifying_key();
        let tmp = TempDir::new().unwrap();
        let manifest = b"id = \"com.example.foo\"\nversion = \"0.7.0\"\n";
        write_manifest(tmp.path(), std::str::from_utf8(manifest).unwrap());
        let sig = sign_digest(&sk, manifest);
        write_sig(tmp.path(), &sig);
        assert!(verify_with_custom_key(tmp.path(), &vk).is_ok());
    }

    #[test]
    fn verify_rejects_tampered_manifest() {
        let seed = [7u8; 32];
        let sk = SigningKey::from_bytes(&seed);
        let vk = sk.verifying_key();
        let tmp = TempDir::new().unwrap();
        let original = b"id = \"com.example.foo\"\nversion = \"0.7.0\"\n";
        let sig = sign_digest(&sk, original);
        write_manifest(
            tmp.path(),
            "id = \"com.example.foo\"\nversion = \"9.9.9\"\n",
        );
        write_sig(tmp.path(), &sig);
        let err = verify_with_custom_key(tmp.path(), &vk).unwrap_err();
        assert!(matches!(err, SigVerifyError::VerificationFailed));
    }

    #[test]
    fn verify_rejects_missing_sidecar() {
        let vk = SigningKey::from_bytes(&[7u8; 32]).verifying_key();
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path(), "id = \"com.example.foo\"\n");
        let err = verify_with_custom_key(tmp.path(), &vk).unwrap_err();
        assert!(matches!(err, SigVerifyError::SidecarMissing));
    }

    #[test]
    fn verify_rejects_wrong_pubkey() {
        let sk_signing = SigningKey::from_bytes(&[7u8; 32]);
        let sk_other = SigningKey::from_bytes(&[9u8; 32]);
        let wrong_vk = sk_other.verifying_key();
        let tmp = TempDir::new().unwrap();
        let manifest = b"id = \"com.example.foo\"\n";
        write_manifest(tmp.path(), std::str::from_utf8(manifest).unwrap());
        let sig = sign_digest(&sk_signing, manifest);
        write_sig(tmp.path(), &sig);
        let err = verify_with_custom_key(tmp.path(), &wrong_vk).unwrap_err();
        assert!(matches!(err, SigVerifyError::VerificationFailed));
    }

    #[test]
    fn verify_rejects_invalid_signature_length() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path(), "id = \"com.example.foo\"\n");
        write_sig(tmp.path(), &[0u8; 32]);
        let vk = SigningKey::from_bytes(&[7u8; 32]).verifying_key();
        let err = verify_with_custom_key(tmp.path(), &vk).unwrap_err();
        assert!(matches!(err, SigVerifyError::InvalidSignatureLength));
    }

    #[test]
    fn embedded_pubkey_is_placeholder_zeros() {
        assert_eq!(TASTY_BUNDLE_PUBKEY, [0u8; 32]);
    }
}
