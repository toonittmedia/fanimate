//! The personal pose library as the text of a file: the user's poses and
//! actions kept outside any project, so a walk or a wave made once is there
//! for every rig. The data and its rules are `fanimate_core::poses`; this
//! only reads and writes the JSON, so the app can keep the file where it
//! keeps the user's other settings and the logic is tested without a window.

use fanimate_core::poses::{LIBRARY_VERSION, MAX_LIBRARY_BYTES, PoseLibrary};

/// Read a library file's text. Anything wrong is an error that says what; the
/// caller keeps the library it had. A library from a newer Fanimate, a file
/// larger than 16 MB and one with too many poses are refused.
pub fn parse(text: &str) -> Result<PoseLibrary, String> {
    if text.len() > MAX_LIBRARY_BYTES {
        return Err("the pose library file is larger than 16 MB".to_owned());
    }
    let library: PoseLibrary =
        serde_json::from_str(text).map_err(|e| format!("the pose library is damaged: {e}"))?;
    library.check()?;
    Ok(library)
}

/// The library as file text.
pub fn to_text(library: &PoseLibrary) -> String {
    let mut library = library.clone();
    library.version = LIBRARY_VERSION;
    // a library always serialises: it holds plain numbers, names and lists
    serde_json::to_string_pretty(&library).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fanimate_core::actions::RigAction;
    use fanimate_core::poses::{self, LibraryPose};
    use fanimate_core::{Interpolation, Skeleton, Vec2};

    fn kept() -> LibraryPose {
        let mut rig = Skeleton::default();
        let spine = rig.add("spine", None, Vec2::ZERO, 10.0, 0.0);
        rig.add("arm L", Some(spine), Vec2::ZERO, 10.0, 0.0);
        rig.bones[1].angle.set_key(1, 0.5, Interpolation::Linear);
        let mut pose: RigAction = poses::snapshot(&rig, "wave", 1.0);
        pose.name = "wave".to_owned();
        rig.actions.push(pose);
        LibraryPose::keep(&rig, 0).unwrap()
    }

    #[test]
    fn the_library_round_trips_through_its_file_text() {
        let mut library = PoseLibrary::default();
        library.add(kept()).unwrap();
        library.add(kept()).unwrap();
        let text = to_text(&library);
        let back = parse(&text).unwrap();
        assert_eq!(back.poses, library.poses);
        assert_eq!(back.version, LIBRARY_VERSION);
        assert_eq!(back.poses[1].name(), "wave 2");
    }

    #[test]
    fn a_file_that_is_wrong_is_refused_with_a_reason() {
        assert!(parse("not json").unwrap_err().contains("damaged"));
        assert!(parse(r#"{"version": 99}"#).unwrap_err().contains("newer"));
        assert!(
            parse(&" ".repeat(MAX_LIBRARY_BYTES + 1))
                .unwrap_err()
                .contains("16 MB")
        );
        // an empty file, or one from before versions, is an empty library
        assert!(parse("{}").unwrap().poses.is_empty());
        // a pose whose numbers are not numbers is damage, not a panic
        assert!(parse(r#"{"poses": [{"rig": 3, "action": "x"}]}"#).is_err());
    }
}
