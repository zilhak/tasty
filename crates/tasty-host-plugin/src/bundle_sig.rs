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
//!
//! ## Trust 단계 (정책 5=B+C)
//!
//! 1. **임베드 키** (`TRUSTED_PUBKEYS`): release-pubkey + dev-pubkey 자동 trust.
//!    이 키로 서명 통과 시 → [`TrustDecision::Trusted`].
//! 2. **사용자 trust DB** ([`crate::known_plugins::KnownPlugins`],
//!    `~/.tasty/known-plugins.toml`): 외부 plugin 이라도 사용자가 한 번 trust
//!    했으면 해당 plugin_id 의 pubkey + 권한 스냅샷을 저장. 매칭 시 → `Trusted`.
//!    단, 매니페스트 권한이 trust 시점과 달라졌으면 → `Untrusted` 로 재승인 요구.
//! 3. **모두 실패** → [`TrustDecision::Untrusted`]. 호출처 (호스트 UI) 가 사용자
//!    승인 모달을 띄우고, 승인 시 `KnownPlugins::add` 로 trust DB 에 기록.

use std::path::Path;

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};

use crate::known_plugins::KnownPlugins;

/// 임베드 trust 키 목록. release-pubkey + dev-pubkey 가 자동 trust.
///
/// 두 키 파일 모두 `crates/tasty-host-plugin/keys/` 하위에 raw 32 byte 로 위치.
/// 길이 mismatch 는 `include_bytes!` 가 컴파일타임에 잡는다. zeroed placeholder 는
/// `VerifyingKey::from_bytes` 가 정상 파싱하더라도 어떤 서명도 통과시키지 못한다.
pub const TRUSTED_PUBKEYS: &[[u8; 32]] = &[
    *include_bytes!("../keys/release-pubkey.bin"),
    *include_bytes!("../keys/dev-pubkey.bin"),
];

/// 하위 호환 alias — 외부 crate 가 본 상수를 참조할 가능성에 대비.
/// `TRUSTED_PUBKEYS` 의 첫 항목 (release-pubkey) 를 가리킨다.
pub const TASTY_BUNDLE_PUBKEY: [u8; 32] = TRUSTED_PUBKEYS[0];

/// `verify_bundle_signature` 의 결과 — 신뢰 단계 분기.
#[derive(Debug, Clone)]
pub enum TrustDecision {
    /// 임베드 키 또는 known_plugins.toml 의 trust 키로 검증 통과.
    Trusted,
    /// 알 수 없는 키 — 사용자 모달 결정 필요.
    ///
    /// 호출처는 본 variant 의 `plugin_id` / `fingerprint` /
    /// `manifest_permissions` 를 사용자에게 노출하고, 승인 시
    /// `KnownPlugins::add` 로 trust DB 에 기록해야 한다.
    Untrusted {
        plugin_id: String,
        /// 서명 키의 SHA-256 hex (사용자 비교용).
        fingerprint: String,
        /// 매니페스트가 요구하는 권한 목록 (UI 노출 + trust DB 저장용).
        manifest_permissions: Vec<String>,
        /// 사유 — UI 분기 (예: 권한 변경 vs 첫 설치).
        reason: UntrustedReason,
    },
}

/// `Untrusted` 의 사유 — UI 가 다른 메시지 / 다른 모달 톤을 보여줄 수 있도록 분리.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UntrustedReason {
    /// known_plugins.toml 에도 없고 임베드 키와도 불일치.
    UnknownKey,
    /// known_plugins.toml 에는 있지만 매니페스트 권한이 trust 시점과 다름.
    PermissionsChanged,
}

#[derive(Debug)]
pub enum SigVerifyError {
    SidecarMissing,
    SidecarReadError(std::io::Error),
    ManifestReadError(std::io::Error),
    InvalidSignatureLength,
    /// 임베드 trust key 가 전부 invalid (예: zeroed placeholder 에서 from_bytes 실패).
    NoValidTrustedKeys,
}

impl std::fmt::Display for SigVerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SidecarMissing => write!(f, "tasty-plugin.toml.sig sidecar missing"),
            Self::SidecarReadError(e) => write!(f, "sidecar read error: {e}"),
            Self::ManifestReadError(e) => write!(f, "manifest read error: {e}"),
            Self::InvalidSignatureLength => write!(f, "signature length != 64 bytes"),
            Self::NoValidTrustedKeys => {
                write!(
                    f,
                    "no valid trusted public keys embedded (all placeholders?)"
                )
            }
        }
    }
}

impl std::error::Error for SigVerifyError {}

/// 매니페스트 sha256 digest 와 sig 를 읽어 `(digest, sig)` 로 반환.
fn read_digest_and_sig(dir: &Path) -> Result<([u8; 32], [u8; 64]), SigVerifyError> {
    let manifest_path = dir.join("tasty-plugin.toml");
    let sig_path = dir.join("tasty-plugin.toml.sig");
    if !sig_path.exists() {
        return Err(SigVerifyError::SidecarMissing);
    }
    let manifest_bytes =
        std::fs::read(&manifest_path).map_err(SigVerifyError::ManifestReadError)?;
    let sig_bytes = std::fs::read(&sig_path).map_err(SigVerifyError::SidecarReadError)?;
    let sig_array: [u8; 64] = sig_bytes
        .as_slice()
        .try_into()
        .map_err(|_| SigVerifyError::InvalidSignatureLength)?;
    let digest: [u8; 32] = Sha256::digest(&manifest_bytes).into();
    Ok((digest, sig_array))
}

/// 매니페스트에서 plugin id 와 요구 권한 목록을 *최선 노력 (best-effort)* 추출.
///
/// 매니페스트 파싱이 실패해도 trust 판단 자체는 진행 가능해야 하므로 panic 하지
/// 않고 fallback 값을 반환한다. plugin_id 가 비면 디렉토리 basename 으로.
fn parse_manifest_basics(dir: &Path) -> (String, Vec<String>) {
    let manifest_path = dir.join("tasty-plugin.toml");
    let text = std::fs::read_to_string(&manifest_path).unwrap_or_default();
    let value: toml::Value = match toml::from_str(&text) {
        Ok(v) => v,
        Err(_) => toml::Value::Table(Default::default()),
    };

    let plugin_id = value
        .get("id")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            dir.file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("<unknown>")
                .to_string()
        });

    // permissions 는 매니페스트 스키마에 따라 [permissions] table 또는
    // [[permissions]] array 가능. 둘 다 지원하지 않더라도 빈 vec 으로.
    let permissions = value
        .get("permissions")
        .and_then(|v| match v {
            toml::Value::Array(arr) => Some(
                arr.iter()
                    .filter_map(|e| e.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>(),
            ),
            toml::Value::Table(tbl) => Some(
                tbl.keys()
                    .filter(|k| matches!(tbl.get(k.as_str()), Some(toml::Value::Boolean(true))))
                    .cloned()
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        })
        .unwrap_or_default();

    (plugin_id, permissions)
}

/// 32 byte ed25519 pubkey 의 SHA-256 fingerprint (hex, 콜론 구분).
pub fn pubkey_fingerprint(pk: &[u8; 32]) -> String {
    let digest = Sha256::digest(pk);
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    hex.as_bytes()
        .chunks(2)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(":")
}

/// 서명 검증 + multi-stage trust 판단.
///
/// 검증 순서:
/// 1. sidecar (`tasty-plugin.toml.sig`) 존재 / 길이 / digest 계산 — 실패 시 Err.
/// 2. [`TRUSTED_PUBKEYS`] 순회 — 매칭 키 발견 시 `Trusted` (early return).
/// 3. 매니페스트에서 plugin_id 추출 → [`KnownPlugins`] lookup.
///    - 매칭 + 서명 검증 통과 + 권한 동일 → `Trusted`.
///    - 매칭 + 권한 다름 → `Untrusted { PermissionsChanged }`.
///    - 매칭 + 서명 검증 실패 → 무시하고 다음 단계로 (DB 의 키와 sig 가 안 맞음).
/// 4. 위 모두 실패 → `Untrusted { UnknownKey }`.
pub fn verify_bundle_signature(dir: &Path) -> Result<TrustDecision, SigVerifyError> {
    let (digest, sig_array) = read_digest_and_sig(dir)?;
    let sig = Signature::from_bytes(&sig_array);

    // Step 1: 임베드 키 시도. invalid 키 (zeroed placeholder 등) 는 skip.
    let mut had_valid_embedded = false;
    for pk in TRUSTED_PUBKEYS {
        let Ok(vk) = VerifyingKey::from_bytes(pk) else {
            continue;
        };
        had_valid_embedded = true;
        if vk.verify(&digest, &sig).is_ok() {
            return Ok(TrustDecision::Trusted);
        }
    }

    // Step 2: known_plugins.toml lookup. load 실패는 비치명 (DB 없을 수 있음).
    let (plugin_id, manifest_permissions) = parse_manifest_basics(dir);
    let known = KnownPlugins::load().unwrap_or_default();

    if let Some(entry) = known.lookup(&plugin_id)
        && let Some(db_pk) = entry.pubkey_bytes()
        && let Ok(vk) = VerifyingKey::from_bytes(&db_pk)
        && vk.verify(&digest, &sig).is_ok()
    {
        if known.permissions_changed(&plugin_id, &manifest_permissions) {
            return Ok(TrustDecision::Untrusted {
                plugin_id: plugin_id.clone(),
                fingerprint: pubkey_fingerprint(&db_pk),
                manifest_permissions,
                reason: UntrustedReason::PermissionsChanged,
            });
        }
        return Ok(TrustDecision::Trusted);
    }

    // Step 3: 알 수 없는 키. 서명의 R 값은 안전한 식별자가 아니므로, 매니페스트
    // 의 id + sig 의 첫 32 byte (signature R) 를 fingerprint 화. 사용자는 publisher
    // 가 제시한 키 fingerprint 와 trust DB 의 fingerprint 를 수동 비교해야 한다.
    // 단, 여기서는 *임베드 키 후보군 자체가 없음* (placeholder) 면 별도 에러로
    // 노출하여 release 빌드 misconfiguration 을 표면화.
    if !had_valid_embedded {
        return Err(SigVerifyError::NoValidTrustedKeys);
    }

    // sig R component (앞 32 byte) — pubkey 자체는 알 수 없으므로 sig 의 R 을 키
    // 후보 fingerprint 로 대체 표현. 사용자가 publisher 의 공식 키 fingerprint 와
    // 비교하려면 publisher 가 raw pubkey 를 제공해야 한다 (또는 manifest 에 pubkey
    // 가 embed 되어 있어야 함). 매니페스트 embed pubkey 정책은 0.7+.
    let r_component: [u8; 32] = sig_array[..32].try_into().unwrap_or([0u8; 32]);
    Ok(TrustDecision::Untrusted {
        plugin_id,
        fingerprint: pubkey_fingerprint(&r_component),
        manifest_permissions,
        reason: UntrustedReason::UnknownKey,
    })
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

    /// 본 헬퍼는 release 시점 전까지 embed pubkey 가 placeholder 라서, multi-key
    /// loop 의 *임시 키 통과 경로* 를 단독으로 점검하기 위해 직접 검증.
    /// 사용자 정의 키로 서명만 검증 (multi-key trust store 우회). placeholder
    /// 시기 동안 single-key 흐름을 보존하기 위한 테스트 헬퍼.
    fn verify_with_custom_key(dir: &Path, vk: &VerifyingKey) -> bool {
        let (digest, sig_array) = match read_digest_and_sig(dir) {
            Ok(p) => p,
            Err(_) => return false,
        };
        let sig = Signature::from_bytes(&sig_array);
        vk.verify(&digest, &sig).is_ok()
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
        assert!(verify_with_custom_key(tmp.path(), &vk));
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
        assert!(!verify_with_custom_key(tmp.path(), &vk));
    }

    #[test]
    fn verify_rejects_missing_sidecar() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path(), "id = \"com.example.foo\"\n");
        let err = verify_bundle_signature(tmp.path()).unwrap_err();
        assert!(matches!(err, SigVerifyError::SidecarMissing));
    }

    #[test]
    fn verify_rejects_invalid_signature_length() {
        let tmp = TempDir::new().unwrap();
        write_manifest(tmp.path(), "id = \"com.example.foo\"\n");
        write_sig(tmp.path(), &[0u8; 32]);
        let err = verify_bundle_signature(tmp.path()).unwrap_err();
        assert!(matches!(err, SigVerifyError::InvalidSignatureLength));
    }

    /// placeholder 시기 (embed pubkey 가 zero) — 임의 sig 를 주면
    /// `NoValidTrustedKeys` 또는 `Untrusted { UnknownKey }` 중 하나. zero pubkey
    /// 의 `VerifyingKey::from_bytes` 동작은 ed25519-dalek 버전에 따라 다르므로
    /// 두 분기 모두 허용.
    #[test]
    fn placeholder_pubkeys_never_grant_trust() {
        let sk = SigningKey::from_bytes(&[42u8; 32]);
        let tmp = TempDir::new().unwrap();
        let manifest = b"id = \"com.example.bar\"\n";
        write_manifest(tmp.path(), std::str::from_utf8(manifest).unwrap());
        write_sig(tmp.path(), &sign_digest(&sk, manifest));
        let result = verify_bundle_signature(tmp.path());
        match result {
            Ok(TrustDecision::Trusted) => panic!("placeholder must not Trust"),
            Ok(TrustDecision::Untrusted { .. }) => {}
            Err(SigVerifyError::NoValidTrustedKeys) => {}
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[test]
    fn fingerprint_format_is_colon_separated_hex() {
        let pk = [0x12u8; 32];
        let fp = pubkey_fingerprint(&pk);
        // 32 byte digest → 64 hex chars → 32 두-자리 그룹 → 31 colon
        assert_eq!(fp.matches(':').count(), 31);
        assert_eq!(fp.len(), 64 + 31);
    }
}
