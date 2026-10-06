//! Windows 단일 실행 보조. 두 번째 프로세스가 실행 중인 Tasty의 MainView 창을 찾아 등록 창 메시지
//! "활성화"를 보내고, 실행 중인 Tasty는 이벤트 루프 메시지 훅으로 그 메시지를 받는다.
//!
//! MainView 창은 전용 클래스 이름 [`MAIN_VIEW_CLASS`]로 만들어 tray-icon의 숨김 창 등과 구별한다.
//! 메시지에는 증거가 실리지 않는다. 같은 데스크톱의 어떤 프로세스든 보낼 수 있지만, 같은 사용자의
//! 프로세스는 원래 Win32 `ShowWindow`·`SetForegroundWindow`로 같은 일을 할 수 있다(ADR-0059).

/// MainView 창의 Win32 클래스 이름.
pub const MAIN_VIEW_CLASS: &str = "TastyMainView";
/// `RegisterWindowMessageW`로 등록하는 활성화 메시지 이름.
pub const ACTIVATE_MESSAGE_NAME: &str = "Tasty.Activate";

/// 열거한 최상위 창 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TopLevelWindow {
    pub hwnd: isize,
    pub pid: u32,
    pub class_name: String,
}

/// `pid`의 MainView 창 하나를 고른다(숨김 포함). 어느 View를 올릴지는 받는 쪽 Tasty가 정한다.
pub fn pick_main_view(windows: &[TopLevelWindow], pid: u32) -> Option<isize> {
    windows
        .iter()
        .find(|w| w.pid == pid && w.class_name == MAIN_VIEW_CLASS)
        .map(|w| w.hwnd)
}

#[cfg(windows)]
pub use imp::*;

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, EnumWindows, GetClassNameW, GetWindowThreadProcessId, MSG,
        PostMessageW, RegisterWindowMessageW, SetForegroundWindow,
    };
    use windows::core::{BOOL, PCWSTR};

    use super::{ACTIVATE_MESSAGE_NAME, TopLevelWindow};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    /// 등록 메시지 번호. 같은 이름이면 프로세스가 달라도 같은 번호다. 실패하면 0.
    pub fn activate_message() -> u32 {
        let name = wide(ACTIVATE_MESSAGE_NAME);
        // SAFETY: NUL 종단 UTF-16 문자열이 호출 동안 살아 있다.
        unsafe { RegisterWindowMessageW(PCWSTR(name.as_ptr())) }
    }

    /// 이 프로세스의 포그라운드 권한을 `pid`에 넘긴다. 사용자가 실행한 프로세스면 성공한다.
    /// 포그라운드 잠금 시간이 지났거나 마지막 입력을 받은 프로세스여도 성공하므로 완전한 증거는 아니다.
    pub fn allow_set_foreground(pid: u32) -> Result<(), String> {
        // SAFETY: 인자는 PID 값뿐이다.
        unsafe { AllowSetForegroundWindow(pid) }
            .map_err(|e| format!("AllowSetForegroundWindow: {e}"))
    }

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
        // SAFETY: lparam 은 enumerate_top_level 이 넘긴 Vec 의 주소이고 열거 동안 살아 있다.
        let out = unsafe { &mut *(lparam.0 as *mut Vec<TopLevelWindow>) };
        let mut pid = 0u32;
        // SAFETY: hwnd 는 EnumWindows 가 넘긴 창이고 pid 는 지역 변수다.
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        let mut buffer = [0u16; 256];
        // SAFETY: buffer 는 256 칸의 쓰기 가능한 버퍼다.
        let len = unsafe { GetClassNameW(hwnd, &mut buffer) };
        let class_name = String::from_utf16_lossy(&buffer[..len.max(0) as usize]);
        out.push(TopLevelWindow {
            hwnd: hwnd.0 as isize,
            pid,
            class_name,
        });
        BOOL(1)
    }

    /// 숨김 창을 포함해 최상위 창을 모두 열거한다.
    pub fn enumerate_top_level() -> Result<Vec<TopLevelWindow>, String> {
        let mut out: Vec<TopLevelWindow> = Vec::new();
        // SAFETY: 콜백은 열거 동안만 out 을 쓴다.
        unsafe { EnumWindows(Some(collect), LPARAM(&mut out as *mut _ as isize)) }
            .map_err(|e| format!("EnumWindows: {e}"))?;
        Ok(out)
    }

    /// 등록 메시지 "활성화"를 창의 스레드 큐로 보낸다.
    pub fn post_activate(hwnd: isize) -> Result<(), String> {
        let message = activate_message();
        if message == 0 {
            return Err("RegisterWindowMessageW failed".to_string());
        }
        // SAFETY: 다른 프로세스의 창에도 메시지를 보낼 수 있으며 인자는 정수뿐이다.
        unsafe {
            PostMessageW(
                Some(HWND(hwnd as *mut std::ffi::c_void)),
                message,
                WPARAM(0),
                LPARAM(0),
            )
        }
        .map_err(|e| format!("PostMessageW: {e}"))
    }

    /// 다른 프로세스가 만든 창에 포그라운드를 요청한다. 거절되면 false.
    pub fn set_foreground(hwnd: u64) -> bool {
        // SAFETY: 창이 이미 사라졌으면 FALSE 를 돌려줄 뿐이다.
        unsafe { SetForegroundWindow(HWND(hwnd as usize as *mut std::ffi::c_void)) }.as_bool()
    }

    /// winit 메시지 훅이 받은 `MSG` 포인터가 활성화 메시지인지 본다.
    ///
    /// # Safety
    /// `msg`는 winit이 훅에 넘긴 유효한 `MSG` 포인터여야 한다.
    pub unsafe fn is_activate_message(msg: *const std::ffi::c_void, activate: u32) -> bool {
        if msg.is_null() || activate == 0 {
            return false;
        }
        // SAFETY: 호출자가 유효한 MSG 포인터를 보장한다.
        unsafe { (*(msg as *const MSG)).message == activate }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(hwnd: isize, pid: u32, class: &str) -> TopLevelWindow {
        TopLevelWindow {
            hwnd,
            pid,
            class_name: class.to_string(),
        }
    }

    #[test]
    fn only_a_main_view_window_of_that_process_is_picked() {
        let windows = [
            w(1, 7, "tray_icon_app"),
            w(2, 8, MAIN_VIEW_CLASS),
            w(3, 7, MAIN_VIEW_CLASS),
        ];
        assert_eq!(pick_main_view(&windows, 7), Some(3));
        assert_eq!(pick_main_view(&windows, 9), None);
        assert_eq!(pick_main_view(&[w(4, 7, "Window Class")], 7), None);
    }
}
