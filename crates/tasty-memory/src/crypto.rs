//! Secret 영역 AES-256-GCM 암호화.
//!
//! 단일 master key (32 byte) 를 OS keyring 에 보관, 첫 실행 시 자동 생성.
//! 모든 secret value 는 `nonce(12) || ciphertext || tag(16)` 으로 저장된다.
//! AAD 는 `SHA256(owner || ':' || scope || ':' || key)` — entry 가 다른
//! (owner, scope, key) 위치로 옮겨졌을 때 decryption 이 실패하도록 묶는다.
//!
//! Keyring 미가용 환경(Linux + dbus 없음 등)에서는
//! `MemoryConfig::allow_plaintext_secret = true` 시 [`SecretCipher::Plaintext`]
//! 로 폴백한다 (호스트가 시작 시 warning 로그). 그 외에는 [`SecretCipher::Unavailable`]
//! 로 두고 secret op 마다 `SecretUnavailable` 를 응답한다.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use rand::RngCore;
use sha2::{Digest, Sha256};

const KEYRING_SERVICE: &str = "tasty";
const KEYRING_USER: &str = "memory.secret.master.v1";
const KEY_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

#[derive(Clone)]
pub struct MasterKey([u8; KEY_LEN]);

impl MasterKey {
    #[cfg(test)]
    pub fn from_bytes(bytes: [u8; KEY_LEN]) -> Self {
        Self(bytes)
    }

    fn cipher(&self) -> Aes256Gcm {
        Aes256Gcm::new_from_slice(&self.0).expect("32-byte key")
    }
}

impl std::fmt::Debug for MasterKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MasterKey(<redacted>)")
    }
}

/// Secret 영역의 동작 모드. `MemoryStore` 는 시작 시 한 번 결정.
pub enum SecretCipher {
    /// 정상 — keyring 에서 가져온 (또는 새로 만들어 저장한) 키로 암복호화.
    Encrypted(MasterKey),
    /// Keyring 없음 + `allow_plaintext_secret=true` — 평문 저장 (경고 후).
    Plaintext,
    /// Keyring 없음 + `allow_plaintext_secret=false` — 모든 secret op 거절.
    Unavailable,
}

impl std::fmt::Debug for SecretCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Encrypted(_) => f.write_str("SecretCipher::Encrypted(..)"),
            Self::Plaintext => f.write_str("SecretCipher::Plaintext"),
            Self::Unavailable => f.write_str("SecretCipher::Unavailable"),
        }
    }
}

impl SecretCipher {
    /// 일반 부팅 시: keyring 시도 → 실패 + plaintext 허용 → Plaintext,
    /// 실패 + 거부 → Unavailable. 성공이면 항상 Encrypted.
    pub fn from_keyring(allow_plaintext: bool) -> Self {
        match load_or_create_master_key() {
            Ok(key) => Self::Encrypted(key),
            Err(e) => {
                if allow_plaintext {
                    tracing::warn!(
                        "memory secret keyring unavailable ({e}); falling back to plaintext per allow_plaintext_secret=true"
                    );
                    Self::Plaintext
                } else {
                    tracing::warn!(
                        "memory secret keyring unavailable ({e}); secret area disabled"
                    );
                    Self::Unavailable
                }
            }
        }
    }

    /// 테스트 / 통합 부팅 점진 마이그레이션용. caller 가 직접 키를 주입.
    #[cfg(test)]
    pub fn with_key(key: MasterKey) -> Self {
        Self::Encrypted(key)
    }
}

/// `nonce(12) || ct || tag(16)` 직렬화. AAD 는 (owner, scope, key) SHA256.
pub fn encrypt(
    key: &MasterKey,
    owner: &str,
    scope: &str,
    entry_key: &str,
    plaintext: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    let mut nonce = [0u8; NONCE_LEN];
    rand::thread_rng().fill_bytes(&mut nonce);
    let aad = aad_hash(owner, scope, entry_key);

    let ct_and_tag = key
        .cipher()
        .encrypt(
            Nonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: &aad,
            },
        )
        .map_err(|_| CryptoError::EncryptFailed)?;

    let mut out = Vec::with_capacity(NONCE_LEN + ct_and_tag.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct_and_tag);
    Ok(out)
}

pub fn decrypt(
    key: &MasterKey,
    owner: &str,
    scope: &str,
    entry_key: &str,
    blob: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    if blob.len() < NONCE_LEN + TAG_LEN {
        return Err(CryptoError::TruncatedBlob);
    }
    let (nonce_bytes, ct_and_tag) = blob.split_at(NONCE_LEN);
    let aad = aad_hash(owner, scope, entry_key);
    key.cipher()
        .decrypt(
            Nonce::from_slice(nonce_bytes),
            Payload {
                msg: ct_and_tag,
                aad: &aad,
            },
        )
        .map_err(|_| CryptoError::DecryptFailed)
}

fn aad_hash(owner: &str, scope: &str, key: &str) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(owner.as_bytes());
    h.update(b":");
    h.update(scope.as_bytes());
    h.update(b":");
    h.update(key.as_bytes());
    h.finalize().into()
}

fn load_or_create_master_key() -> Result<MasterKey, String> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, KEYRING_USER)
        .map_err(|e| format!("keyring entry: {e}"))?;
    match entry.get_password() {
        Ok(b64) => {
            let raw = BASE64
                .decode(b64.as_bytes())
                .map_err(|e| format!("stored key not base64: {e}"))?;
            if raw.len() != KEY_LEN {
                return Err(format!("stored key wrong size: {}", raw.len()));
            }
            let mut arr = [0u8; KEY_LEN];
            arr.copy_from_slice(&raw);
            Ok(MasterKey(arr))
        }
        Err(keyring::Error::NoEntry) => {
            let mut bytes = [0u8; KEY_LEN];
            rand::thread_rng().fill_bytes(&mut bytes);
            let b64 = BASE64.encode(bytes);
            entry
                .set_password(&b64)
                .map_err(|e| format!("keyring set: {e}"))?;
            tracing::info!("generated new memory.secret master key in OS keyring");
            Ok(MasterKey(bytes))
        }
        Err(e) => Err(format!("keyring get: {e}")),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CryptoError {
    #[error("encrypt failed")]
    EncryptFailed,
    #[error("decrypt failed (wrong key, corrupted ciphertext, or AAD mismatch)")]
    DecryptFailed,
    #[error("ciphertext blob too short")]
    TruncatedBlob,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> MasterKey {
        MasterKey::from_bytes([7u8; KEY_LEN])
    }

    #[test]
    fn roundtrip() {
        let k = key();
        let blob = encrypt(&k, "plugin.a", "global", "tok", b"hello world").unwrap();
        assert!(blob.len() >= NONCE_LEN + TAG_LEN);
        let plain = decrypt(&k, "plugin.a", "global", "tok", &blob).unwrap();
        assert_eq!(plain, b"hello world");
    }

    #[test]
    fn wrong_aad_fails() {
        let k = key();
        let blob = encrypt(&k, "plugin.a", "global", "tok", b"x").unwrap();
        let err = decrypt(&k, "plugin.b", "global", "tok", &blob).unwrap_err();
        assert!(matches!(err, CryptoError::DecryptFailed));
        let err = decrypt(&k, "plugin.a", "surface.1", "tok", &blob).unwrap_err();
        assert!(matches!(err, CryptoError::DecryptFailed));
        let err = decrypt(&k, "plugin.a", "global", "other", &blob).unwrap_err();
        assert!(matches!(err, CryptoError::DecryptFailed));
    }

    #[test]
    fn wrong_key_fails() {
        let blob = encrypt(&key(), "o", "s", "k", b"x").unwrap();
        let other = MasterKey::from_bytes([9u8; KEY_LEN]);
        let err = decrypt(&other, "o", "s", "k", &blob).unwrap_err();
        assert!(matches!(err, CryptoError::DecryptFailed));
    }

    #[test]
    fn truncated_blob_rejected() {
        let err = decrypt(&key(), "o", "s", "k", &[0u8; 8]).unwrap_err();
        assert!(matches!(err, CryptoError::TruncatedBlob));
    }
}
