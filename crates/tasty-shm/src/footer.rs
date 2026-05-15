//! SharedBuffer atomic generation footer.
//!
//! 두 프로세스가 공유 메모리 영역을 read/write할 때 tear(half-painted frame)를
//! 막기 위해 영역의 시작 8바이트를 `AtomicU64 generation`으로 reserve한다.
//!
//! # 메모리 레이아웃
//!
//! ```text
//! [ AtomicU64 generation (8B) | user data (size - 8) ... ]
//! ```
//!
//! footer를 영역 *끝*이 아니라 *시작*에 둔 이유: mmap된 페이지의 시작 주소는
//! 항상 페이지 크기로 정렬되므로 (4KB align ⊃ 8B align), 시작 8바이트의 `AtomicU64`
//! 정렬이 자명하게 보장된다. 끝에 두면 영역 전체 크기에 따라 unaligned 가능.
//!
//! # 동기화 규약
//!
//! - **Writer**(생산자): user data 모두 쓴 뒤 `fetch_add(1, Release)`.
//! - **Reader**(소비자): `gen_before = load(Acquire)` → user data 읽기 →
//!   `gen_after = load(Acquire)` → 두 값이 같으면 일관된 frame, 다르면 다음
//!   frame까지 skip(이전 결과 유지).
//!
//! # 사용자 영역
//!
//! 본 모듈은 footer 8바이트를 user에게 숨기는 슬라이스 분할(`user_slice` /
//! `user_slice_mut`)과 footer atomic 접근(`load` / `fetch_add`)을 제공한다.

use std::sync::atomic::{AtomicU64, Ordering};

/// footer 영역의 바이트 크기.
pub const SIZE: usize = 8;

/// 사용자 영역이 시작되는 offset (= footer 크기).
pub const USER_OFFSET: usize = SIZE;

/// user 데이터에 사용 가능한 길이를 전체 영역 길이로부터 계산.
///
/// `total < SIZE`이면 0을 반환한다(영역이 footer도 못 담는 비정상 상태).
pub fn user_len(total: usize) -> usize {
    total.saturating_sub(SIZE)
}

/// 전체 영역 raw slice에서 user data 부분을 잘라낸다.
///
/// 영역 길이가 `SIZE` 미만이면 빈 슬라이스 반환.
pub fn user_slice(raw: &[u8]) -> &[u8] {
    if raw.len() < SIZE {
        &[]
    } else {
        &raw[SIZE..]
    }
}

/// 전체 영역 raw mutable slice에서 user data 부분을 잘라낸다.
pub fn user_slice_mut(raw: &mut [u8]) -> &mut [u8] {
    if raw.len() < SIZE {
        &mut []
    } else {
        &mut raw[SIZE..]
    }
}

/// 전체 영역 raw slice의 시작 8바이트를 `AtomicU64`로 해석한다.
///
/// # Safety
///
/// 호출자는 다음을 보장해야 한다:
/// - `raw.as_ptr()`이 8바이트 정렬되어 있다 (mmap 페이지 시작 = 항상 4KB 정렬 ⊃ 8).
/// - `raw.len() >= SIZE`.
/// - 영역의 시작 8바이트가 다른 용도로 사용되지 않는다 (footer 합의를 따르는 양쪽
///   프로세스 사이에서만 호출).
pub unsafe fn footer_atomic(raw: &[u8]) -> &AtomicU64 {
    debug_assert!(raw.len() >= SIZE, "raw too small for footer");
    debug_assert_eq!(
        (raw.as_ptr() as usize) % std::mem::align_of::<AtomicU64>(),
        0,
        "raw start must be 8-aligned"
    );
    // SAFETY: 호출자가 정렬과 길이를 보장. AtomicU64는 8바이트 layout, raw[0..8]을
    // 재해석한다. atomic 접근은 외부 프로세스의 동시 atomic 접근과 합쳐도 Rust
    // 메모리 모델 위반이 아니다(byte-level race가 아닌 atomic op끼리의 race).
    unsafe { &*(raw.as_ptr() as *const AtomicU64) }
}

/// footer를 통해 generation 값을 atomic load.
///
/// # Safety
///
/// `footer_atomic`의 안전 조건과 동일.
pub unsafe fn load(raw: &[u8], ordering: Ordering) -> u64 {
    // SAFETY: 호출자가 footer_atomic 조건을 보장.
    unsafe { footer_atomic(raw).load(ordering) }
}

/// footer의 generation을 1 증가시키고 이전 값을 반환.
///
/// # Safety
///
/// `footer_atomic`의 안전 조건과 동일.
pub unsafe fn fetch_add(raw: &[u8], val: u64, ordering: Ordering) -> u64 {
    // SAFETY: 호출자가 footer_atomic 조건을 보장.
    unsafe { footer_atomic(raw).fetch_add(val, ordering) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn user_len_subtracts_footer() {
        assert_eq!(user_len(4096), 4096 - SIZE);
        assert_eq!(user_len(SIZE), 0);
        assert_eq!(user_len(0), 0);
        assert_eq!(user_len(SIZE - 1), 0);
    }

    #[test]
    fn user_slice_skips_footer() {
        let raw = [0u8; 16];
        let user = user_slice(&raw);
        assert_eq!(user.len(), 16 - SIZE);
    }

    #[test]
    fn user_slice_mut_skips_footer() {
        let mut raw = [0u8; 16];
        let user = user_slice_mut(&mut raw);
        assert_eq!(user.len(), 16 - SIZE);
        user[0] = 0xAB;
        assert_eq!(raw[SIZE], 0xAB);
        assert!(raw[..SIZE].iter().all(|&b| b == 0), "footer untouched");
    }

    #[test]
    fn footer_load_and_increment() {
        // 8-aligned 영역을 만들기 위해 Box<[u64]> 사용.
        let backing: Box<[u64]> = vec![0; 4].into_boxed_slice();
        let ptr = backing.as_ptr() as *const u8;
        // SAFETY: backing은 32바이트(4 × u64) 살아있고, u64-aligned이므로 ptr/len 유효.
        let raw = unsafe { std::slice::from_raw_parts(ptr, 32) };

        // SAFETY: raw는 8-aligned이고 길이 32 ≥ SIZE. footer 모듈의 안전 조건 충족.
        let g0 = unsafe { load(raw, Ordering::Acquire) };
        assert_eq!(g0, 0);

        // SAFETY: 위와 동일.
        let prev = unsafe { fetch_add(raw, 1, Ordering::Release) };
        assert_eq!(prev, 0);
        // SAFETY: 위와 동일.
        let g1 = unsafe { load(raw, Ordering::Acquire) };
        assert_eq!(g1, 1);

        // SAFETY: 위와 동일.
        unsafe { fetch_add(raw, 1, Ordering::Release) };
        // SAFETY: 위와 동일.
        let g2 = unsafe { load(raw, Ordering::Acquire) };
        assert_eq!(g2, 2);

        drop(backing);
    }
}
