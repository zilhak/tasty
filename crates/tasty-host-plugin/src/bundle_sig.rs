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
/// 두 키는 build.rs 가 `OUT_DIR` 로 staging 한 raw 32 byte 를 가리킨다 (소스
/// 트리 `keys/` 직접 참조 아님). `dev-pubkey.bin` 은 개발자별 로컬 키라 추적되지
/// 않으므로, 부재 시 build.rs 가 all-zero placeholder 로 슬롯을 채워 컴파일이
/// 깨지지 않게 한다. 자세한 배경은 `build.rs` 모듈 주석 참고. zeroed placeholder
/// 는 `VerifyingKey::from_bytes` 가 정상 파싱하더라도 어떤 서명도 통과시키지 못한다.
pub const TRUSTED_PUBKEYS: &[[u8; 32]] = &[
    *include_bytes!(concat!(env!("OUT_DIR"), "/release-pubkey.bin")),
    *include_bytes!(concat!(env!("OUT_DIR"), "/dev-pubkey.bin")),
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

/// 외부 publisher 가 동봉한 `tasty-plugin.toml.pub` sidecar 를 읽어
/// raw 32 byte ed25519 pubkey 를 반환. 파일이 없거나 길이가 다르면 None.
///
/// 본 함수는 [`verify_bundle_signature`] 의 *core 검증* 에는 참여하지 않는다.
/// `TrustDecision::Untrusted { UnknownKey, .. }` 인 plugin 을 사용자가 trust
/// 하기로 결정했을 때, [`crate::known_plugins::KnownPluginEntry::pubkey`] 에
/// 저장할 raw pubkey 를 얻기 위한 *trust 저장 시점 helper*. signature 자체는
/// pubkey 를 노출하지 않으므로 publisher 가 sidecar 로 제공해야 trust 가 가능.
pub fn read_pubkey_sidecar(dir: &Path) -> Option<[u8; 32]> {
    let path = dir.join("tasty-plugin.toml.pub");
    let bytes = std::fs::read(&path).ok()?;
    if bytes.len() != 32 {
        return None;
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Some(out)
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
    fn pubkey_sidecar_reads_32_byte_file() {
        let tmp = TempDir::new().unwrap();
        let pk = [0x55u8; 32];
        std::fs::write(tmp.path().join("tasty-plugin.toml.pub"), pk).unwrap();
        assert_eq!(read_pubkey_sidecar(tmp.path()), Some(pk));
    }

    #[test]
    fn pubkey_sidecar_rejects_wrong_length() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("tasty-plugin.toml.pub"), [0u8; 16]).unwrap();
        assert_eq!(read_pubkey_sidecar(tmp.path()), None);
    }

    #[test]
    fn pubkey_sidecar_missing_returns_none() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(read_pubkey_sidecar(tmp.path()), None);
    }

    /// 시나리오: 외부 publisher 가 자체 key 로 서명 → 임베드 키 mismatch.
    /// `verify_bundle_signature` 는 `Untrusted` 반환해야 함 (Add UI 가 빨간 경고).
    /// 또한 `.pub` sidecar 가 있으면 trust 저장에 필요한 raw pubkey 가 동봉됨.
    #[test]
    fn untrusted_external_signer_with_pubkey_sidecar() {
        let sk = SigningKey::from_bytes(&[99u8; 32]);
        let vk = sk.verifying_key();
        let tmp = TempDir::new().unwrap();
        let manifest = b"id = \"com.example.extp\"\nversion = \"0.1.0\"\n";
        write_manifest(tmp.path(), std::str::from_utf8(manifest).unwrap());
        write_sig(tmp.path(), &sign_digest(&sk, manifest));
        std::fs::write(
            tmp.path().join("tasty-plugin.toml.pub"),
            vk.to_bytes().as_slice(),
        )
        .unwrap();

        // verify_bundle_signature 는 Untrusted (UnknownKey) — 단, placeholder
        // 키 환경에서는 NoValidTrustedKeys 도 가능 (테스트 환경 의존).
        match verify_bundle_signature(tmp.path()) {
            Ok(TrustDecision::Untrusted { reason, .. }) => {
                assert_eq!(reason, UntrustedReason::UnknownKey);
            }
            Err(SigVerifyError::NoValidTrustedKeys) => {} // placeholder key 환경
            other => panic!("expected Untrusted/UnknownKey, got {other:?}"),
        }

        // `.pub` sidecar 로 publisher pubkey 를 추출 가능 → Add UI 가
        // TrustAndInstall 액션을 enqueue 할 수 있음.
        assert_eq!(read_pubkey_sidecar(tmp.path()), Some(vk.to_bytes()));
    }

    #[test]
    fn untrusted_external_signer_without_pubkey_sidecar() {
        let sk = SigningKey::from_bytes(&[88u8; 32]);
        let tmp = TempDir::new().unwrap();
        let manifest = b"id = \"com.example.nopub\"\nversion = \"0.1.0\"\n";
        write_manifest(tmp.path(), std::str::from_utf8(manifest).unwrap());
        write_sig(tmp.path(), &sign_digest(&sk, manifest));
        // sidecar 없음 → Add UI 가 UntrustedNoPubkey 분기, install 차단.
        assert!(read_pubkey_sidecar(tmp.path()).is_none());
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

/// `KnownPlugins` 와의 e2e 분기 — HOME 환경변수를 임시 디렉토리로 돌려 trust DB
/// 를 격리한 뒤 `verify_bundle_signature` 의 multi-stage 동작을 검증.
///
/// Unix 한정: `directories::BaseDirs` 가 Unix 에서는 HOME 환경변수로 결정되지만
/// Windows 에서는 `SHGetKnownFolderPath` 를 통해 결정되므로 env override 가 통하지
/// 않는다. Windows 용 e2e 는 별도 `KnownPlugins::load_from(path)` API 도입 후 0.7+.
#[cfg(all(test, unix))]
mod integration_tests {
    use super::*;
    use crate::known_plugins::KnownPluginEntry;
    use ed25519_dalek::{Signer, SigningKey};
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// HOME override race 방지용 글로벌 mutex — cargo test 의 병렬 실행에서
    /// 두 테스트가 동시에 HOME 을 바꾸면 한 쪽이 다른 쪽의 디렉토리를 보게 된다.
    static HOME_LOCK: Mutex<()> = Mutex::new(());

    struct HomeOverride {
        original: Option<std::ffi::OsString>,
        // MutexGuard 이 살아있는 동안 다른 테스트가 HOME 을 못 만짐.
        _guard: std::sync::MutexGuard<'static, ()>,
    }

    impl HomeOverride {
        fn new(home: &Path) -> Self {
            let guard = HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let original = std::env::var_os("HOME");
            // SAFETY: 단일 스레드만이 HOME_LOCK 을 보유 — 다른 테스트의 동시 access 차단.
            unsafe {
                std::env::set_var("HOME", home);
            }
            Self {
                original,
                _guard: guard,
            }
        }
    }

    impl Drop for HomeOverride {
        fn drop(&mut self) {
            match &self.original {
                // SAFETY: lock 해제 이전, 단일 스레드.
                Some(v) => unsafe { std::env::set_var("HOME", v) },
                // SAFETY: lock 해제 이전, 단일 스레드.
                None => unsafe { std::env::remove_var("HOME") },
            }
        }
    }

    /// 매니페스트 작성 + ed25519 signing — 매니페스트 digest 에 sk 서명.
    fn make_bundle(
        bundle_dir: &Path,
        sk: &SigningKey,
        plugin_id: &str,
        perms: &[&str],
    ) -> [u8; 32] {
        let perms_toml = perms
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let manifest =
            format!("id = \"{plugin_id}\"\nversion = \"1.0.0\"\npermissions = [{perms_toml}]\n");
        std::fs::write(bundle_dir.join("tasty-plugin.toml"), &manifest).unwrap();
        let digest = Sha256::digest(manifest.as_bytes());
        let sig = sk.sign(digest.as_slice()).to_bytes().to_vec();
        std::fs::write(bundle_dir.join("tasty-plugin.toml.sig"), &sig).unwrap();
        sk.verifying_key().to_bytes()
    }

    /// `~/.tasty/known-plugins.toml` 에 trust 항목 1 개 작성.
    fn write_known_db(home: &Path, plugin_id: &str, pk: &[u8; 32], perms: &[&str]) {
        let tasty_dir = home.join(".tasty");
        std::fs::create_dir_all(&tasty_dir).unwrap();
        let perms_lines = perms
            .iter()
            .map(|p| format!("\"{p}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let contents = format!(
            "[plugins.\"{plugin_id}\"]\npubkey = \"{}\"\npermissions = [{perms_lines}]\ntrusted_at = \"2026-06-09T00:00:00Z\"\npublisher_fingerprint = \"\"\n",
            KnownPluginEntry::encode_pubkey(pk),
        );
        std::fs::write(tasty_dir.join("known-plugins.toml"), contents).unwrap();
    }

    /// B-1: 임베드 키로는 검증 실패하지만 known_plugins.toml 의 trust 항목이
    /// 일치 → `Trusted` 반환.
    #[test]
    fn known_plugins_trust_grants_trusted() {
        let home_tmp = TempDir::new().unwrap();
        let _g = HomeOverride::new(home_tmp.path());

        let bundle_tmp = TempDir::new().unwrap();
        let sk = SigningKey::from_bytes(&[99u8; 32]);
        let perms = ["filesystem.read"];
        let pk = make_bundle(bundle_tmp.path(), &sk, "com.example.bnk", &perms);
        write_known_db(home_tmp.path(), "com.example.bnk", &pk, &perms);

        let result = verify_bundle_signature(bundle_tmp.path()).unwrap();
        assert!(
            matches!(result, TrustDecision::Trusted),
            "expected Trusted, got {result:?}"
        );
    }

    /// B-2: known_plugins.toml 에는 권한 `["filesystem.read"]` 만 trust 했는데
    /// 매니페스트는 `["filesystem.read", "network.outbound"]` 를 요구 → 권한 변경
    /// 감지로 `Untrusted { PermissionsChanged }`.
    #[test]
    fn known_plugins_permission_change_yields_untrusted() {
        let home_tmp = TempDir::new().unwrap();
        let _g = HomeOverride::new(home_tmp.path());

        let bundle_tmp = TempDir::new().unwrap();
        let sk = SigningKey::from_bytes(&[88u8; 32]);
        let manifest_perms = ["filesystem.read", "network.outbound"];
        let old_perms = ["filesystem.read"];
        let pk = make_bundle(bundle_tmp.path(), &sk, "com.example.pchg", &manifest_perms);
        write_known_db(home_tmp.path(), "com.example.pchg", &pk, &old_perms);

        let result = verify_bundle_signature(bundle_tmp.path()).unwrap();
        match result {
            TrustDecision::Untrusted {
                reason,
                plugin_id,
                manifest_permissions,
                ..
            } => {
                assert_eq!(reason, UntrustedReason::PermissionsChanged);
                assert_eq!(plugin_id, "com.example.pchg");
                assert_eq!(manifest_permissions.len(), 2);
            }
            other => panic!("expected PermissionsChanged, got {other:?}"),
        }
    }

    /// B-3: placeholder embed pubkey + 빈 trust DB → 절대 `Trusted` 가 나오면 안 됨.
    /// 현재 동작: `Untrusted { UnknownKey }` 또는 `Err(NoValidTrustedKeys)` 중 하나
    /// (ed25519-dalek 버전에 따라 zero pubkey 의 `from_bytes` 결과가 다름).
    ///
    /// TODO(0.7+): release-pubkey.bin 이 zero 일 때 *build.rs* 가 cargo:warning 으로
    /// 명시적 misconfig 안내. 본 테스트는 placeholder 인 동안 *invariant 만* 검증.
    #[test]
    fn placeholder_embed_with_empty_db_never_trusts() {
        let home_tmp = TempDir::new().unwrap();
        let _g = HomeOverride::new(home_tmp.path());

        let bundle_tmp = TempDir::new().unwrap();
        let sk = SigningKey::from_bytes(&[17u8; 32]);
        // pk 반환값 무시 — placeholder 시나리오는 trust DB 가 비어 있어 어떤
        // pubkey 로도 매칭이 일어나지 않는다.
        let _pk = make_bundle(
            bundle_tmp.path(),
            &sk,
            "com.example.ph",
            &["filesystem.read"],
        );

        let result = verify_bundle_signature(bundle_tmp.path());
        match result {
            Ok(TrustDecision::Trusted) => panic!("placeholder must never grant Trusted"),
            Ok(TrustDecision::Untrusted {
                reason: UntrustedReason::UnknownKey,
                ..
            }) => {}
            Err(SigVerifyError::NoValidTrustedKeys) => {}
            other => panic!("unexpected result: {other:?}"),
        }
    }
}
