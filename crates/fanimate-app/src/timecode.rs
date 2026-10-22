//! Frame numbers as time: "seconds:frames" for the status bar, transport and ruler.

/// Whole frames in one second at `fps`. Anything below 1 (or not a number) counts as 1.
fn per_second(fps: f32) -> i32 {
    if fps.is_finite() && fps >= 1.0 {
        (fps.round() as i32).max(1)
    } else {
        1
    }
}

/// `S:FF`: whole seconds, then the frame within that second, two digits. Frame 41 at 24 fps is "1:17".
pub fn clock(frame: i32, fps: f32) -> String {
    let n = per_second(fps);
    let frame = frame.max(0);
    format!("{}:{:02}", frame / n, frame % n)
}

/// True on the first frame of each second (frame 0 included).
pub fn is_second_start(frame: i32, fps: f32) -> bool {
    frame.rem_euclid(per_second(fps)) == 0
}

/// "2s" for the ruler: the whole seconds before `frame`.
pub fn seconds_label(frame: i32, fps: f32) -> String {
    format!("{}s", frame.max(0) / per_second(fps))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_splits_seconds_and_frames() {
        assert_eq!(clock(0, 24.0), "0:00");
        assert_eq!(clock(41, 24.0), "1:17");
        assert_eq!(clock(24, 24.0), "1:00");
        assert_eq!(clock(120, 24.0), "5:00");
    }

    #[test]
    fn odd_input_does_not_panic() {
        assert_eq!(clock(-5, 24.0), "0:00");
        assert_eq!(clock(7, 0.0), "7:00");
        assert_eq!(clock(7, f32::NAN), "7:00");
        assert_eq!(clock(7, f32::INFINITY), "7:00");
        assert!(!clock(i32::MAX, 24.0).is_empty());
        assert!(is_second_start(3, 0.0));
        assert_eq!(seconds_label(-3, f32::NAN), "0s");
    }

    #[test]
    fn fractional_fps_rounds() {
        assert_eq!(clock(30, 29.97), "1:00");
        assert!(is_second_start(30, 29.97));
        assert!(!is_second_start(31, 29.97));
    }

    #[test]
    fn second_starts_and_labels() {
        assert!(is_second_start(0, 24.0));
        assert!(is_second_start(48, 24.0));
        assert!(!is_second_start(47, 24.0));
        assert_eq!(seconds_label(48, 24.0), "2s");
        assert_eq!(seconds_label(0, 24.0), "0s");
    }
}
