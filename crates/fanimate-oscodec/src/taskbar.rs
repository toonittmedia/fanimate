//! Progress on the program's taskbar button (Windows' `ITaskbarList3`), so a
//! render or an update shows how far along it is while the window is hidden
//! behind others.
//!
//! The one call into Windows is wrapped here; asking is harmless when the
//! window or the taskbar is not there, and on other systems it does nothing.

/// What the taskbar button shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TaskbarProgress {
    /// No progress: the plain button.
    Off,
    /// Working, with no way to say how far along.
    Busy,
    /// This far along, 0 to 1.
    Fraction(f32),
    /// Stopped by a problem, at this fraction (the bar turns red).
    Failed(f32),
}

/// A fraction as the (done, of) pair Windows takes: out of 1000, so the bar
/// moves in small steps, and never outside the bar.
pub fn scale(fraction: f32) -> (u64, u64) {
    let f = if fraction.is_finite() {
        fraction.clamp(0.0, 1.0)
    } else {
        0.0
    };
    ((f * 1000.0).round() as u64, 1000)
}

/// Show `progress` on the taskbar button of the window `window` (a Win32
/// `HWND`). Does nothing off Windows, or if the window is not valid.
pub fn set_taskbar_progress(window: isize, progress: TaskbarProgress) {
    #[cfg(windows)]
    imp::set(window, progress);
    #[cfg(not(windows))]
    let _ = (window, progress);
}

#[cfg(windows)]
mod imp {
    use std::cell::RefCell;

    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance};
    use windows::Win32::UI::Shell::{
        ITaskbarList3, TBPF_ERROR, TBPF_INDETERMINATE, TBPF_NOPROGRESS, TBPF_NORMAL, TaskbarList,
    };

    use super::{TaskbarProgress, scale};

    thread_local! {
        /// The taskbar object belongs to the thread that made it (the
        /// window's), so each thread keeps its own.
        static LIST: RefCell<Option<ITaskbarList3>> = const { RefCell::new(None) };
    }

    pub fn set(window: isize, progress: TaskbarProgress) {
        if window == 0 {
            return;
        }
        LIST.with(|cell| {
            let mut slot = cell.borrow_mut();
            if slot.is_none() {
                // SAFETY: creates a COM object from a class Windows ships;
                // winit has started COM on the window's thread
                *slot =
                    unsafe { CoCreateInstance::<_, ITaskbarList3>(&TaskbarList, None, CLSCTX_INPROC_SERVER) }
                        .ok();
                if let Some(list) = slot.as_ref()
                    // SAFETY: HrInit has no arguments but the object itself
                    && unsafe { list.HrInit() }.is_err()
                {
                    *slot = None;
                }
            }
            let Some(list) = slot.as_ref() else { return };
            let hwnd = HWND(window as *mut std::ffi::c_void);
            // errors (a window that has gone) are not worth reporting
            let _ = match progress {
                TaskbarProgress::Off => unsafe { list.SetProgressState(hwnd, TBPF_NOPROGRESS) },
                TaskbarProgress::Busy => unsafe { list.SetProgressState(hwnd, TBPF_INDETERMINATE) },
                TaskbarProgress::Fraction(f) => {
                    let (done, of) = scale(f);
                    // SAFETY: plain calls on a live COM object with a window handle
                    unsafe { list.SetProgressState(hwnd, TBPF_NORMAL) }
                        .and_then(|()| unsafe { list.SetProgressValue(hwnd, done, of) })
                }
                TaskbarProgress::Failed(f) => {
                    let (done, of) = scale(f);
                    // SAFETY: as above
                    unsafe { list.SetProgressValue(hwnd, done, of) }
                        .and_then(|()| unsafe { list.SetProgressState(hwnd, TBPF_ERROR) })
                }
            };
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fraction_becomes_a_step_of_a_thousand_inside_the_bar() {
        assert_eq!(scale(0.0), (0, 1000));
        assert_eq!(scale(0.5), (500, 1000));
        assert_eq!(scale(1.0), (1000, 1000));
        assert_eq!(scale(-3.0), (0, 1000));
        assert_eq!(scale(7.0), (1000, 1000));
        assert_eq!(scale(f32::NAN), (0, 1000));
        assert_eq!(scale(f32::INFINITY), (0, 1000));
    }

    #[test]
    fn asking_about_no_window_does_nothing() {
        // a window that does not exist is not an error
        for p in [
            TaskbarProgress::Off,
            TaskbarProgress::Busy,
            TaskbarProgress::Fraction(0.5),
            TaskbarProgress::Failed(0.2),
        ] {
            set_taskbar_progress(0, p);
        }
    }
}
