//! 이 프로세스의 인스턴스 식별자.
//!
//! `system.info`의 `instance_id`로 노출한다. 호스트명이나 LAN IP로 SSH 터널을 거쳐 연결한 상대가
//! 이 프로세스 자신인지는 포트만으로 알 수 없으므로 상대가 보고한 이 값과 비교한다.
//! 프로세스가 시작될 때마다 새로 만들며 저장하지 않는다.

use std::sync::OnceLock;

/// 16바이트 OS random을 hex 32자로 만든 값. 같은 프로세스 안에서는 항상 같다.
pub fn instance_id() -> &'static str {
    static ID: OnceLock<String> = OnceLock::new();
    ID.get_or_init(|| {
        use rand::RngCore;
        let mut bytes = [0u8; 16];
        rand::rng().fill_bytes(&mut bytes);
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_is_stable_within_the_process_and_hex() {
        let id = instance_id();
        assert_eq!(id, instance_id());
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
    }
}
