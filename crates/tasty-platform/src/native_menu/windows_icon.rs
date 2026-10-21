//! Win32 메뉴가 빌려 쓰는 alpha DIB. 메뉴 파괴 뒤 bitmap을 해제한다.

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, COLOR_GRAYTEXT, COLOR_MENUTEXT, CreateDIBSection,
    DIB_RGB_COLORS, DeleteObject, GetSysColor, HBITMAP,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    HMENU, MENUITEMINFOW, MIIM_BITMAP, SetMenuItemInfoW,
};

pub(super) struct MenuBitmap(pub HBITMAP);

impl MenuBitmap {
    pub fn new(icon: super::super::MenuIcon, hwnd: HWND, enabled: bool) -> anyhow::Result<Self> {
        // Win32 logical pixel은 96 DPI 기준이다. 이는 디자인 치수가 아닌 OS 단위 변환이다.
        // SAFETY: 호출자가 메뉴를 띄우는 동안 살아 있는 winit HWND를 전달한다.
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        anyhow::ensure!(dpi != 0, "cannot read menu window DPI");
        let raster = icon.rasterize(dpi as f32 / 96.0)?;
        let side = raster.physical.value() as i32;
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: side,
                // 음수 높이는 top-down DIB. SVG raster의 첫 행과 방향을 맞춘다.
                biHeight: -side,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        // SAFETY: info는 유효한 32-bit DIB 선언이며 OS가 bits에 소유 메모리를 반환한다.
        let bitmap =
            Self(unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0)? });
        anyhow::ensure!(
            !bits.is_null(),
            "CreateDIBSection returned no pixel storage"
        );
        // SAFETY: GetSysColor는 전역 native 팔레트를 읽고 외부 포인터를 받지 않는다.
        let color = unsafe {
            GetSysColor(if enabled {
                COLOR_MENUTEXT
            } else {
                COLOR_GRAYTEXT
            })
        };
        let bgra = raster.bgra([color as u8, (color >> 8) as u8, (color >> 16) as u8]);
        // SAFETY: 성공한 DIB는 side²×4 바이트를 소유하며 raster도 같은 크기이다.
        unsafe {
            std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast(), bgra.len());
        }
        Ok(bitmap)
    }
    pub fn attach(self, menu: HMENU, id: u32) -> anyhow::Result<Self> {
        let info = MENUITEMINFOW {
            cbSize: std::mem::size_of::<MENUITEMINFOW>() as u32,
            fMask: MIIM_BITMAP,
            hbmpItem: self.0,
            ..Default::default()
        };
        // SAFETY: 호출자가 소유한 메뉴 항목에 이 객체가 살아 있는 동안 bitmap을 빌려 준다.
        unsafe {
            SetMenuItemInfoW(menu, id, false, &info)?;
        }
        Ok(self)
    }
}

impl Drop for MenuBitmap {
    fn drop(&mut self) {
        // SAFETY: 이 RAII 객체가 만든 bitmap만 소유하며 어떤 DC에도 선택하지 않았다.
        if !unsafe { DeleteObject(self.0.into()) }.as_bool() {
            tracing::warn!(
                "DeleteObject menu bitmap failed: {}",
                std::io::Error::last_os_error()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_type_appearance::theme::SIZING;
    use windows::Win32::Graphics::Gdi::{DIBSECTION, GetObjectW};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WINDOW_STYLE,
    };
    use windows::core::w;

    struct TestWindow(HWND);
    impl Drop for TestWindow {
        fn drop(&mut self) {
            // SAFETY: 이 시험 스레드에서 만든 숨긴 윈도우만 파괴한다.
            unsafe {
                DestroyWindow(self.0).unwrap();
            }
        }
    }

    #[test]
    fn dib_has_backing_pixels_and_is_released() {
        // SAFETY: 내장 STATIC 클래스로 보이지 않는 독립 윈도우를 만든다. 입력/포커스를 요청하지 않는다.
        let window = TestWindow(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!("menu icon test"),
                WINDOW_STYLE::default(),
                0,
                0,
                1,
                1,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        });
        for enabled in [true, false] {
            let icon = crate::native_menu::MenuIcon::from_svg(
                tasty_icons::FOLDER_OPEN.svg,
                SIZING.icon_glyph_size_md,
            );
            let bitmap = MenuBitmap::new(icon, window.0, enabled).unwrap();
            let handle = bitmap.0;
            let mut dib = DIBSECTION::default();
            // SAFETY: dib는 구조체 크기만큼 쓰기 가능하며 bitmap은 살아 있다.
            let read = unsafe {
                GetObjectW(
                    handle.into(),
                    std::mem::size_of::<DIBSECTION>() as i32,
                    Some((&mut dib as *mut DIBSECTION).cast()),
                )
            };
            assert_eq!(read as usize, std::mem::size_of::<DIBSECTION>());
            assert_eq!(dib.dsBm.bmBitsPixel, 32);
            // SAFETY: 같은 스레드에서 소유한 숨긴 윈도우의 DPI를 읽는다.
            let scale = unsafe { GetDpiForWindow(window.0) } as f32 / 96.0;
            assert_eq!(
                dib.dsBm.bmWidth,
                icon.size.to_physical(scale).value().ceil() as i32
            );
            assert_eq!(dib.dsBm.bmHeight, dib.dsBm.bmWidth);
            let len = (dib.dsBm.bmWidthBytes * dib.dsBm.bmHeight) as usize;
            // SAFETY: GetObjectW로 받은 DIB의 소유 메모리를 해제 전에 해당 크기만큼 읽는다.
            let pixels = unsafe { std::slice::from_raw_parts(dib.dsBm.bmBits.cast::<u8>(), len) };
            assert!(pixels.chunks_exact(4).any(|p| p[3] > 0));
            assert!(
                pixels
                    .chunks_exact(4)
                    .all(|p| p[..3].iter().all(|c| *c <= p[3]))
            );
            drop(bitmap);
            // SAFETY: 출력 버퍼를 지정해 실제 조회한다. null 버퍼는 핸들과 무관하게 타입 크기를 돌려줄 수 있다.
            assert_eq!(
                // SAFETY: 위에서 설명한 실제 출력 버퍼 조회이며 dib는 쓰기 가능한 구조체다.
                unsafe {
                    GetObjectW(
                        handle.into(),
                        std::mem::size_of::<DIBSECTION>() as i32,
                        Some((&mut dib as *mut DIBSECTION).cast()),
                    )
                },
                0,
                "bitmap must be deleted"
            );
        }
    }
}
