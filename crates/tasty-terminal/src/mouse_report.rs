//! 마우스 리포팅 시퀀스 인코딩 (xterm DECSET 1000/1002/1003, SGR 1006 / legacy X10).
//!
//! 좌표 변환(픽셀→셀)·버튼/modifier 조립은 호출측(view 레이어)이 하고, 본 모듈은
//! 조립된 `cb`(코드 바이트)와 1-based 셀 좌표를 받아 바이트열로만 인코딩한다.
//! 휠·버튼 press/release·드래그 motion·debug 주입이 모두 이 함수를 공유한다.

/// 단일 마우스 이벤트를 리포팅 시퀀스로 인코딩한다.
///
/// `cb` 는 호출측이 조립한 코드 바이트:
/// - 버튼: 0=left / 1=middle / 2=right, 휠 64=up / 65=down
/// - 드래그(motion): `| 32`
/// - modifier: shift `| 4` / alt(meta) `| 8` / ctrl `| 16`
///
/// `col`/`row` 는 1-based 셀 좌표. `release` 가 true 면 버튼 떼기.
///
/// `sgr`(mode 1006) true: `ESC [ < cb ; col ; row (M|m)` — release 면 `m`, 좌표 무제한.
/// false (legacy X10): `ESC [ M (32+cb') (32+col) (32+row)` — release 는 버튼을 3 으로
/// 표기(버튼 구분 불가), 각 바이트 255 clamp (좌표 223 초과는 인코딩 한계로 포화).
pub fn encode_mouse_report(sgr: bool, cb: u8, col: usize, row: usize, release: bool) -> Vec<u8> {
    if sgr {
        let suffix = if release { 'm' } else { 'M' };
        format!("\x1b[<{cb};{col};{row}{suffix}").into_bytes()
    } else {
        // X10: release 는 버튼 코드 3(low 2 bits = 11). motion/휠/modifier 비트는 보존.
        let cb_x10 = if release { cb | 0b11 } else { cb };
        let off = |v: usize| (32 + v.min(223)) as u8; // 32 offset, 223 cap (255 포화)
        vec![
            0x1b,
            b'[',
            b'M',
            (32u16 + cb_x10 as u16).min(255) as u8,
            off(col),
            off(row),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::encode_mouse_report;

    #[test]
    fn sgr_press_release_motion() {
        // press left @ (3,5)
        assert_eq!(encode_mouse_report(true, 0, 3, 5, false), b"\x1b[<0;3;5M");
        // release → 'm', 버튼 코드 유지
        assert_eq!(encode_mouse_report(true, 0, 3, 5, true), b"\x1b[<0;3;5m");
        // drag(motion) = cb | 32
        assert_eq!(
            encode_mouse_report(true, 32, 10, 2, false),
            b"\x1b[<32;10;2M"
        );
    }

    #[test]
    fn sgr_modifier_and_wheel() {
        // shift(|4) + left
        assert_eq!(encode_mouse_report(true, 4, 1, 1, false), b"\x1b[<4;1;1M");
        // wheel up (64)
        assert_eq!(encode_mouse_report(true, 64, 3, 5, false), b"\x1b[<64;3;5M");
    }

    #[test]
    fn x10_press_and_offset() {
        // press left @ (1,1): cb=0 → 32, col/row → 33
        assert_eq!(
            encode_mouse_report(false, 0, 1, 1, false),
            vec![0x1b, b'[', b'M', 32, 33, 33]
        );
        // wheel up: cb=64 → 96
        assert_eq!(
            encode_mouse_report(false, 64, 1, 1, false),
            vec![0x1b, b'[', b'M', 96, 33, 33]
        );
    }

    #[test]
    fn x10_release_sets_button_three() {
        // release: 버튼 코드 3 → 32+3 = 35
        assert_eq!(
            encode_mouse_report(false, 0, 1, 1, true),
            vec![0x1b, b'[', b'M', 35, 33, 33]
        );
    }

    /// 1003 의 버튼 없는 hover: 버튼 코드 3(no button) + motion 32 = 35.
    #[test]
    fn hover_motion_uses_button_three() {
        assert_eq!(
            encode_mouse_report(true, 35, 12, 4, false),
            b"\x1b[<35;12;4M"
        );
        // X10 폴백: 32+35 = 67 로 255 포화 없이 인코딩된다.
        assert_eq!(
            encode_mouse_report(false, 35, 1, 1, false),
            vec![0x1b, b'[', b'M', 67, 33, 33]
        );
    }

    #[test]
    fn x10_coordinate_saturates() {
        // col=300 → 32+223 = 255 포화
        let out = encode_mouse_report(false, 64, 300, 1, false);
        assert_eq!(out[4], 255);
    }
}
