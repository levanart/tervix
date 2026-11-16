//! Small native chrome adapter. No native handle is retained beyond its borrowed callback.

/// Starts the Windows system sizing loop on a borrowed live window.
/// `edge` is clockwise: left=0, right=1, top=2, bottom=3,
/// top-left=4, top-right=5, bottom-left=6, bottom-right=7.
/// Returns false on unsupported platforms, handle types or edge values.
/// Must be called on the window event thread, in response to a left-button press.
pub fn begin_resize(handle: raw_window_handle::WindowHandle<'_>, edge: u8) -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::UI::{
            Input::KeyboardAndMouse::ReleaseCapture, WindowsAndMessaging::*,
        };
        let raw_window_handle::RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return false;
        };
        let hit = match edge {
            0 => HTLEFT,
            1 => HTRIGHT,
            2 => HTTOP,
            3 => HTBOTTOM,
            4 => HTTOPLEFT,
            5 => HTTOPRIGHT,
            6 => HTBOTTOMLEFT,
            7 => HTBOTTOMRIGHT,
            _ => return false,
        };
        // SAFETY: the toolkit lends a live HWND on its owning event thread for
        // this callback. No pointer is retained. ReleaseCapture and the system
        // sizing message implement the same modal operation as a native frame.
        unsafe {
            let mut position = windows_sys::Win32::Foundation::POINT { x: 0, y: 0 };
            if GetCursorPos(&mut position) == 0 {
                return false;
            }
            ReleaseCapture();
            PostMessageW(
                handle.hwnd.get() as _,
                WM_NCLBUTTONDOWN,
                hit as usize,
                ((position.x as u16 as u32) | ((position.y as u16 as u32) << 16)) as isize,
            ) != 0
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (handle, edge);
        false
    }
}
