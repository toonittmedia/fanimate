//! Progress on this window's taskbar button, for renders and updates.
//!
//! A render goes on in its own window and an update downloads in the
//! background, often with Fanimate behind other programs; the taskbar button
//! shows how far along they are. Windows only (elsewhere it does nothing).

use fanimate_oscodec::{TaskbarProgress, set_taskbar_progress};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// The Win32 window handle of this window, if it has one.
fn hwnd(frame: &eframe::Frame) -> Option<isize> {
    match frame.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
        _ => None,
    }
}

/// What was last shown, so Windows is asked only when something changed.
#[derive(Default)]
pub struct Taskbar {
    last: Option<(isize, TaskbarProgress)>,
}

/// A fraction in whole percents: the bar moves in steps the eye can see, and
/// Windows is not asked for every sliver of a frame.
fn rounded(p: TaskbarProgress) -> TaskbarProgress {
    let percent = |f: f32| (f.clamp(0.0, 1.0) * 100.0).round() / 100.0;
    match p {
        TaskbarProgress::Fraction(f) => TaskbarProgress::Fraction(percent(f)),
        TaskbarProgress::Failed(f) => TaskbarProgress::Failed(percent(f)),
        other => other,
    }
}

impl Taskbar {
    /// Show `progress` on this window's button.
    pub fn show(&mut self, frame: &eframe::Frame, progress: TaskbarProgress) {
        let Some(window) = hwnd(frame) else { return };
        let progress = rounded(progress);
        if self.last == Some((window, progress)) {
            return;
        }
        set_taskbar_progress(window, progress);
        self.last = Some((window, progress));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_moves_in_whole_percents() {
        assert_eq!(
            rounded(TaskbarProgress::Fraction(0.1234)),
            TaskbarProgress::Fraction(0.12)
        );
        assert_eq!(
            rounded(TaskbarProgress::Fraction(0.1241)),
            rounded(TaskbarProgress::Fraction(0.1249))
        );
        assert_eq!(
            rounded(TaskbarProgress::Failed(2.0)),
            TaskbarProgress::Failed(1.0)
        );
        assert_eq!(rounded(TaskbarProgress::Busy), TaskbarProgress::Busy);
        assert_eq!(rounded(TaskbarProgress::Off), TaskbarProgress::Off);
    }
}
