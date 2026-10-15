//! Keeps the custom title bar's undecorated window truly borderless on Windows (#1758).
//!
//! winit 0.30 gives an undecorated window the `WS_CAPTION | WS_BORDER` styles and hides the
//! non-client area by spreading the client rect over it (`WM_NCCALCSIZE`). On some systems the
//! desktop compositor still reserves that frame for the window surface: the content is drawn
//! offset by the caption and border while the pointer keeps the un-offset client coordinates, so
//! it no longer points at what is drawn under it (rust-windowing/winit#2419; PhotoCraft fixed the
//! same in storytold/photocraft#2401). [`service`] strips the two styles and has Windows
//! recalculate the frame; winit puts them back whenever it changes the window flags (maximize,
//! restore, …), so the returned guard runs every frame.
//!
//! Maximizing needs a second fix: while maximized, winit clamps the client rect to the taskbar's
//! work area (`WM_NCCALCSIZE`), but Windows itself maximizes the caption-less window to the whole
//! monitor plus its resize-border overhang. The band between the two is non-client area nothing
//! paints into, where a caption winit gave back gets drawn and then sticks as a stale composition
//! — at the top of the screen when the taskbar is there, above the app bar. The guard puts the
//! window rect where the client already is while maximized, so there is no band to draw in.

/// Run once per frame; a no-op once the window is borderless.
pub type WindowFrameFn = Box<dyn FnMut()>;

pub use platform::service;

#[cfg(all(windows, feature = "wgpu"))]
mod platform {
    use eframe::wgpu::rwh::{HasWindowHandle, RawWindowHandle};
    use winsafe::{HWND, HwndPlace, POINT, SIZE, co};

    /// A guard that keeps the window borderless each frame, or `None` with the system title bar
    /// (the window is decorated) or when its handle can't be found.
    pub fn service(cc: &eframe::CreationContext<'_>, custom_titlebar: bool) -> Option<super::WindowFrameFn> {
        if !custom_titlebar {
            return None;
        }
        let target = match cc.window_handle().ok()?.as_raw() {
            RawWindowHandle::Win32(h) => h.hwnd.get(),
            _ => return None,
        };
        let hwnd = find_window(target)?;
        strip_and_align(&hwnd);
        Some(Box::new(move || strip_and_align(&hwnd)))
    }

    /// The top-level window with the given raw handle: `EnumWindows` hands out real `HWND`s, so
    /// this needs no `unsafe` to wrap a raw one.
    fn find_window(target: isize) -> Option<HWND> {
        let mut found = None;
        let _ = winsafe::EnumWindows(|hwnd| {
            if hwnd.ptr() as isize == target {
                found = Some(hwnd);
                false
            } else {
                true
            }
        });
        found
    }

    /// Strips `WS_CAPTION | WS_BORDER` and, while maximized, puts the window rect where the
    /// client already is (the work area). Both in one atomic `SetWindowPos` so winit can't
    /// re-add the caption between the two operations.
    fn strip_and_align(hwnd: &HWND) {
        let style = hwnd.style();
        let without = style & !(co::WS::CAPTION | co::WS::BORDER);
        let style_changed = without.raw() != style.raw();

        // While maximized, the outer rect should match the work area; otherwise keep the current rect.
        let (pos, size, needs_move) = if hwnd.IsZoomed() {
            let Ok(work) = hwnd.MonitorFromWindow(co::MONITOR::DEFAULTTONEAREST).GetMonitorInfo().map(|m| m.rcWork) else {
                return;
            };
            let pos = POINT::with(work.left, work.top);
            let size = SIZE::with(work.right - work.left, work.bottom - work.top);
            let Ok(outer) = hwnd.GetWindowRect() else { return };
            (pos, size, outer != work)
        } else {
            (POINT::new(), SIZE::new(), false)
        };

        if !style_changed && !needs_move {
            return;
        }

        let mut flags = co::SWP::NOZORDER | co::SWP::NOACTIVATE;
        if !needs_move {
            flags |= co::SWP::NOMOVE | co::SWP::NOSIZE;
        }
        if style_changed {
            flags |= co::SWP::FRAMECHANGED;
        }

        hwnd.set_style(without);
        let _ = hwnd.SetWindowPos(HwndPlace::None, pos, size, flags);
    }
}

#[cfg(not(all(windows, feature = "wgpu")))]
mod platform {
    pub fn service(_cc: &eframe::CreationContext<'_>, _custom_titlebar: bool) -> Option<super::WindowFrameFn> {
        None
    }
}
