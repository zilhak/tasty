//! 공유 영역의 변경 횟수를 나타내는 atomic generation.
//!
//! ```text
//! [ AtomicU64 generation (8B) | user data (size - 8) ... ]
//! ```
//!
//! 페이지 정렬을 이용하기 위해 이름과 달리 영역 시작에 둔다. 쓰기를 마친 뒤
//! `fetch_add(1, Release)`하고 읽는 쪽은 `load(Acquire)`로 변경 여부를 확인한다.
//! 읽기 전후 값이 같아도 다음 쓰기가 이미 시작됐을 수 있으므로, 이 비교만으로
//! 일관된 프레임이나 동시 접근의 안전성을 보장하지 않는다. 데이터 접근에는 별도
//! 동기화가 필요하다. `user_slice`와 `user_slice_mut`은 앞의 8바이트를 제외한다.

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
    if raw.len() < SIZE { &[] } else { &raw[SIZE..] }
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
/// 시작 주소가 `AtomicU64` 정렬을 만족하고 길이가 `SIZE` 이상이어야 한다.
/// 반환된 참조가 유효한 동안 앞의 8바이트는 generation 전용이며,
/// 다른 접근도 같은 크기의 atomic 연산을 사용해야 한다. 비atomic 접근과 섞으면 안 된다.
pub unsafe fn footer_atomic(raw: &[u8]) -> &AtomicU64 {
    debug_assert!(raw.len() >= SIZE, "raw too small for footer");
    debug_assert_eq!(
        (raw.as_ptr() as usize) % std::mem::align_of::<AtomicU64>(),
        0,
        "raw start must be 8-aligned"
    );
    // SAFETY: 호출자가 유효한 영역·정렬·길이와 atomic 전용 접근을 보장한다.
    // 이 변환은 payload의 동시 접근까지 안전하게 만들지는 않는다.
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

/// generation에 val을 더하고 이전 값을 반환한다.
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
