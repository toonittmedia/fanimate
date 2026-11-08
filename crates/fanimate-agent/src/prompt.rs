//! What a model is told about Fanimate: the rules of the project's space and
//! time that the tool descriptions cannot carry, and where the user is.
//!
//! The same guide goes to every route: the in-app agent sends it as its
//! system prompt, and the MCP server hands it to other agents as the server's
//! `instructions`.

/// How Fanimate works, for a model that drives it through the tools.
pub const GUIDE: &str = "\
You are the assistant inside Fanimate, a 2D animation app. You change the open project only by calling the tools, and the tools are the only way to look at it.

How the project works:
- Space: the canvas is in pixels, x to the right, y DOWN, origin at the top left. `document` gives its width, height and frame rate.
- Time: frames are whole numbers from `start` (1 in a new project) to `end`. Frame 0 is the rest pose, the value a property has when nothing is keyed.
- Units: angles are in degrees, clockwise on screen. Opacity is 0 to 1. Colours are like #RRGGBB.
- Keying rule: a change on frame 0 (the default when no frame is given) sets the value itself for the whole animation. A change on a later frame sets a key on that frame, and seeds a key on frame 0 with the old value if there was none, so nothing jumps. To animate, set the starting value, then set the values you want on later frames. Use set_key_interpolation to ease a move.
- Pivot: a layer turns and scales about its own origin. Draw a moving shape around the layer's origin (x and y of minus half its width and height) and move the layer with set_layer_transform, so it turns about its middle.
- Drawing: new layers are empty. Add shapes with add_shape (rectangle or ellipse). For anything else, write an SVG and use import_svg. Give layers clear names.
- Bones: add_bone_layer, add_bone and set_bone_angle make a rig; bake_spring adds secondary motion.

How to work:
- If you do not know the project, call `document` and `layers` first, and use the layer ids they give. Never guess an id.
- Work in small steps and read every answer. An error says what to fix; never repeat a failing call unchanged.
- The animation cannot be edited while it plays (the error code is `playing`). Say so, and do not retry.
- Every call is one undo step, everything you add is labelled as AI-made in the file, and the user can undo it. Say only what the tools confirmed.
- Stop when the request is done and say in one or two plain sentences what changed.";

/// Where the user is in the project, sent with each message.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    pub project: String,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub start: i32,
    pub end: i32,
    pub layers: usize,
    /// The playhead.
    pub frame: i32,
    /// The selected layers, as (id, name).
    pub selection: Vec<(u32, String)>,
}

impl Context {
    /// The line under the chat box: what a message is about.
    pub fn chip(&self) -> String {
        let what = match self.selection.as_slice() {
            [] => "whole project".to_owned(),
            [(_, name)] => name.clone(),
            [(_, name), rest @ ..] => format!("{name} and {} more", rest.len()),
        };
        format!("Context: {what}, frame {}", self.frame)
    }

    /// The same, for the model.
    pub fn to_text(&self) -> String {
        let mut text = format!(
            "Project \"{}\": {}x{} pixels at {} frames a second, frames {} to {}, {} layers. The playhead is on frame {}.",
            self.project, self.width, self.height, self.fps, self.start, self.end, self.layers, self.frame
        );
        match self.selection.as_slice() {
            [] => text.push_str(" No layer is selected."),
            list => {
                let names: Vec<String> = list.iter().map(|(id, n)| format!("{id} \"{n}\"")).collect();
                text.push_str(&format!(" Selected layers: {}.", names.join(", ")));
            }
        }
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> Context {
        Context {
            project: "Scene".into(),
            width: 1920,
            height: 1080,
            fps: 24.0,
            start: 1,
            end: 120,
            layers: 3,
            frame: 12,
            selection: vec![],
        }
    }

    #[test]
    fn the_chip_says_what_a_message_is_about() {
        let mut c = context();
        assert_eq!(c.chip(), "Context: whole project, frame 12");
        c.selection = vec![(4, "Title".into())];
        assert_eq!(c.chip(), "Context: Title, frame 12");
        c.selection.push((5, "Ball".into()));
        assert_eq!(c.chip(), "Context: Title and 1 more, frame 12");
    }

    #[test]
    fn the_model_is_told_ids_and_names() {
        let mut c = context();
        assert!(c.to_text().contains("No layer is selected"));
        c.selection = vec![(4, "Title".into())];
        let text = c.to_text();
        assert!(text.contains("1920x1080") && text.contains("frames 1 to 120"));
        assert!(text.contains("4 \"Title\""));
    }

    #[test]
    fn the_guide_names_only_tools_that_exist() {
        for tool in [
            "set_layer_transform",
            "set_key_interpolation",
            "add_shape",
            "import_svg",
            "add_bone_layer",
            "add_bone",
            "set_bone_angle",
            "bake_spring",
            "document",
            "layers",
        ] {
            assert!(crate::tools::is_tool_name(tool), "{tool} is not a tool any more");
            assert!(GUIDE.contains(tool), "{tool}");
        }
        assert!(!GUIDE.contains('\u{2014}'), "no em dashes");
    }
}
