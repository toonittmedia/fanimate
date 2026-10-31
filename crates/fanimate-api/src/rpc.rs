//! The API as JSON calls: what the script hosts, the headless mode and
//! out-of-process plugins speak.
//!
//! A call names a method and gives its parameters as a JSON object; the
//! answer is a JSON value or an error with one of the API's stable codes.
//! Method and parameter names are part of the API (the same names as the
//! Rust calls), and [`METHODS`] lists them so a host can say what it offers
//! without a second source of truth. A call that is not valid for its method
//! is `invalid_argument`, never a panic.

use serde::Deserialize;
use serde_json::{Value, json};

use crate::{Api, ApiError, ErrorCode, TextStyle};

/// One method a host can call: its name, what it takes and what it does.
pub struct Method {
    pub name: &'static str,
    /// Parameter names with their JSON types, in order.
    pub params: &'static str,
    /// Whether it changes the document (and so is refused while playing).
    pub edits: bool,
    pub about: &'static str,
}

const fn query(name: &'static str, params: &'static str, about: &'static str) -> Method {
    Method {
        name,
        params,
        edits: false,
        about,
    }
}

const fn edit(name: &'static str, params: &'static str, about: &'static str) -> Method {
    Method {
        name,
        params,
        edits: true,
        about,
    }
}

/// Every method [`call`] knows, queries first.
pub const METHODS: &[Method] = &[
    query("api.version", "", "The API version"),
    query("api.methods", "", "This list"),
    query("document", "", "The project size, rate, range and format version"),
    query("layers", "", "Every layer, depth first"),
    query("layer", "id: number", "One layer"),
    query("bones", "layer: number", "The bones of a bone layer"),
    query("text", "layer: number", "A text layer with its words and look"),
    edit("rename_layer", "id: number, name: string", "Rename a layer"),
    edit(
        "rename_bone",
        "layer: number, bone: number, name: string",
        "Rename a bone",
    ),
    edit(
        "set_layer_visible",
        "id: number, visible: boolean",
        "Show or hide a layer",
    ),
    edit(
        "set_layer_provenance",
        "id: number, provenance: object or null",
        "Record what made a layer's content",
    ),
    edit(
        "add_text_layer",
        "content: string, x: number, y: number",
        "Add a text layer, returns its id",
    ),
    edit(
        "set_text_content",
        "layer: number, content: string",
        "Replace the words",
    ),
    edit(
        "set_text_style",
        "layer: number, style: object",
        "Change a text layer's look",
    ),
    edit(
        "add_text_animator",
        "layer: number, preset: string",
        "Add a ready-made animator, returns its index",
    ),
    query(
        "actions",
        "layer: number",
        "The actions and poses of a bone layer",
    ),
    edit(
        "save_pose",
        "layer: number, name: string (optional), frame: number (optional)",
        "Save the whole rig on a frame as a pose, returns its index",
    ),
    edit(
        "mirror_pose",
        "layer: number, index: number",
        "Make the left-for-right mirror of a pose, returns its index",
    ),
    edit(
        "insert_action",
        "layer: number, index: number, frame: number",
        "Put an action or pose into the timeline from a frame",
    ),
    edit(
        "import_svg",
        "svg: string, name: string",
        "Import SVG text as layers; text elements become text layers",
    ),
    query(
        "controllers",
        "layer: number, frame: number (optional)",
        "The master controllers of a bone layer with their values",
    ),
    edit(
        "add_controller",
        "layer: number, name: string, kind: \"slider\" or \"pad\"",
        "Add a master controller, returns its index",
    ),
    query(
        "curves",
        "layer: number",
        "The layer's keyed curves: channel (position, rotation, scale, opacity, order, bone:<id>), component, and keys with frame, value (degrees for angles, percent for opacity) and interp",
    ),
    edit(
        "move_key",
        "layer: number, channel: string, component: number, frame: number, to_frame: number, value: number",
        "Move a key in time and set one curve's value there; returns the frame it landed on",
    ),
    edit(
        "set_key_interpolation",
        "layer: number, channel: string, frame: number, interp: \"smooth\" | \"linear\" | \"step\" | {\"ease\": name} | {\"bezier\": [x1, y1, x2, y2]}",
        "Change how the curve leaves a key; returns whether it changed",
    ),
    edit(
        "rename_controller",
        "layer: number, index: number, name: string",
        "Rename a master controller",
    ),
    edit(
        "remove_controller",
        "layer: number, index: number",
        "Remove a master controller",
    ),
    edit(
        "link_controller",
        "layer: number, controller: number, action: number, axis: \"x\" or \"y\", from: number, to: number",
        "Make a controller play an action as its value goes from one number to another",
    ),
    edit(
        "unlink_controller",
        "layer: number, controller: number, action: number",
        "Take away a controller's hold on an action, returns whether it had one",
    ),
    edit(
        "set_controller_value",
        "layer: number, controller: number, axis: \"x\" or \"y\", value: number (0 to 1), frame: number (optional)",
        "Set a controller value, keyed after frame 0",
    ),
    edit(
        "add_bone_layer",
        "name: string",
        "Add an empty bone layer, returns its id",
    ),
    edit(
        "add_bone",
        "layer: number, name: string, parent: number (optional), x, y, length: number",
        "Add a bone to a bone layer, returns its id",
    ),
    edit(
        "set_bone_angle",
        "layer: number, bone: number, degrees: number, frame: number (optional)",
        "Turn a bone relative to its parent, keyed after frame 0",
    ),
    edit(
        "bake_spring",
        "layer: number, bones: [number], stiffness, damping, inertia, gravity, wind (each optional), start, end (optional)",
        "Make bones follow their keys on a spring and bake the swing to keys",
    ),
    edit(
        "set_text_animator_value",
        "layer: number, index: number, property: string, value: number, [x, y] or \"#RRGGBB\", frame: number (optional)",
        "Set one value of a text animator (tracking, outline, position, ...), keyed after frame 0",
    ),
    edit(
        "set_text_path",
        "layer: number, shape_layer: number",
        "Run the text along another layer's first shape",
    ),
    edit("clear_text_path", "layer: number", "Set the text straight again"),
    edit(
        "set_text_path_offset",
        "layer: number, percent: number",
        "Where along its path the text starts",
    ),
    edit(
        "import_captions",
        "content: string, name: string (optional)",
        "Read SRT or WebVTT text into a layer of captions",
    ),
    query(
        "export_captions",
        "layer: number, format: \"srt\" or \"vtt\"",
        "A caption layer's captions as SRT or WebVTT text",
    ),
    edit(
        "add_layer",
        "kind: \"vector\", \"group\" or \"switch\", name: string, parent: number (optional)",
        "Add an empty layer, returns its id",
    ),
    edit(
        "delete_layer",
        "id: number",
        "Delete a layer and what is inside it",
    ),
    edit(
        "set_document",
        "name, width, height, fps, start, end (each optional)",
        "Change the project settings",
    ),
    edit(
        "set_layer_transform",
        "id: number, frame, x, y, rotation (degrees), scale_x, scale_y, opacity (each optional)",
        "Move, turn, scale or fade a layer, keyed after frame 0",
    ),
    edit(
        "add_shape",
        "layer: number, shape: object (kind, x, y, width, height, radius, fill, stroke, stroke_width)",
        "Draw a rectangle or an ellipse, returns its index",
    ),
    edit("undo", "", "Undo the last step; returns whether anything changed"),
    edit(
        "redo",
        "",
        "Redo the last undone step; returns whether anything changed",
    ),
];

fn bad(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::InvalidArgument, message)
}

/// Pull parameter `key` out of the params object as a `T`.
fn param<T: for<'de> Deserialize<'de>>(params: &Value, key: &str) -> Result<T, ApiError> {
    let value = params
        .get(key)
        .ok_or_else(|| bad(format!("missing parameter \"{key}\"")))?;
    serde_json::from_value(value.clone()).map_err(|e| bad(format!("parameter \"{key}\": {e}")))
}

fn json_of<T: serde::Serialize>(value: T) -> Result<Value, ApiError> {
    serde_json::to_value(value).map_err(|e| bad(format!("the answer could not be written: {e}")))
}

/// Run one call. `params` is a JSON object (or `null` for none).
pub fn call(api: &mut Api<'_>, method: &str, params: &Value) -> Result<Value, ApiError> {
    if !(params.is_object() || params.is_null()) {
        return Err(bad("params must be an object"));
    }
    match method {
        "api.version" => Ok(json!(crate::API_VERSION)),
        "api.methods" => Ok(Value::Array(
            METHODS
                .iter()
                .map(|m| json!({"name": m.name, "params": m.params, "edits": m.edits, "about": m.about}))
                .collect(),
        )),
        "document" => json_of(api.document()),
        "layers" => json_of(api.layers()),
        "layer" => json_of(api.layer(param(params, "id")?)?),
        "bones" => json_of(api.bones(param(params, "layer")?)?),
        "text" => json_of(api.text(param(params, "layer")?)?),
        "rename_layer" => {
            api.rename_layer(param(params, "id")?, &param::<String>(params, "name")?)?;
            Ok(Value::Null)
        }
        "rename_bone" => {
            api.rename_bone(
                param(params, "layer")?,
                param(params, "bone")?,
                &param::<String>(params, "name")?,
            )?;
            Ok(Value::Null)
        }
        "set_layer_visible" => {
            api.set_layer_visible(param(params, "id")?, param(params, "visible")?)?;
            Ok(Value::Null)
        }
        "set_layer_provenance" => {
            api.set_layer_provenance(param(params, "id")?, param(params, "provenance")?)?;
            Ok(Value::Null)
        }
        "add_text_layer" => json_of(api.add_text_layer(
            &param::<String>(params, "content")?,
            param(params, "x")?,
            param(params, "y")?,
        )?),
        "set_text_content" => {
            api.set_text_content(param(params, "layer")?, &param::<String>(params, "content")?)?;
            Ok(Value::Null)
        }
        "set_text_style" => {
            api.set_text_style(param(params, "layer")?, param::<TextStyle>(params, "style")?)?;
            Ok(Value::Null)
        }
        "add_text_animator" => {
            json_of(api.add_text_animator(param(params, "layer")?, &param::<String>(params, "preset")?)?)
        }
        "actions" => json_of(api.actions(param(params, "layer")?)?),
        "save_pose" => {
            let name: Option<String> = params
                .get("name")
                .filter(|n| !n.is_null())
                .map(|_| param(params, "name"))
                .transpose()?;
            let frame = params
                .get("frame")
                .filter(|f| !f.is_null())
                .map(|_| param(params, "frame"))
                .transpose()?;
            json_of(api.save_pose(param(params, "layer")?, name.as_deref(), frame)?)
        }
        "mirror_pose" => json_of(api.mirror_pose(param(params, "layer")?, param(params, "index")?)?),
        "insert_action" => {
            api.insert_action(
                param(params, "layer")?,
                param(params, "index")?,
                param(params, "frame")?,
            )?;
            Ok(Value::Null)
        }
        "import_svg" => json_of(api.import_svg(
            &param::<String>(params, "svg")?,
            &param::<String>(params, "name")?,
        )?),
        "controllers" => {
            let frame = params
                .get("frame")
                .filter(|f| !f.is_null())
                .map(|_| param(params, "frame"))
                .transpose()?;
            json_of(api.controllers(param(params, "layer")?, frame)?)
        }
        "add_controller" => json_of(api.add_controller(
            param(params, "layer")?,
            &param::<String>(params, "name")?,
            &param::<String>(params, "kind")?,
        )?),
        "curves" => json_of(api.curves(param(params, "layer")?)?),
        "move_key" => json_of(api.move_key(
            param(params, "layer")?,
            &param::<String>(params, "channel")?,
            param(params, "component")?,
            param(params, "frame")?,
            param(params, "to_frame")?,
            param(params, "value")?,
        )?),
        "set_key_interpolation" => json_of(api.set_key_interpolation(
            param(params, "layer")?,
            &param::<String>(params, "channel")?,
            param(params, "frame")?,
            param(params, "interp")?,
        )?),
        "rename_controller" => {
            api.rename_controller(
                param(params, "layer")?,
                param(params, "index")?,
                &param::<String>(params, "name")?,
            )?;
            Ok(Value::Null)
        }
        "remove_controller" => {
            api.remove_controller(param(params, "layer")?, param(params, "index")?)?;
            Ok(Value::Null)
        }
        "link_controller" => {
            api.link_controller(
                param(params, "layer")?,
                param(params, "controller")?,
                param(params, "action")?,
                &param::<String>(params, "axis")?,
                param(params, "from")?,
                param(params, "to")?,
            )?;
            Ok(Value::Null)
        }
        "unlink_controller" => json_of(api.unlink_controller(
            param(params, "layer")?,
            param(params, "controller")?,
            param(params, "action")?,
        )?),
        "set_controller_value" => {
            let frame = params
                .get("frame")
                .filter(|f| !f.is_null())
                .map(|_| param(params, "frame"))
                .transpose()?;
            api.set_controller_value(
                param(params, "layer")?,
                param(params, "controller")?,
                &param::<String>(params, "axis")?,
                param(params, "value")?,
                frame,
            )?;
            Ok(Value::Null)
        }
        "add_bone_layer" => json_of(api.add_bone_layer(&param::<String>(params, "name")?)?),
        "add_bone" => {
            let parent = params
                .get("parent")
                .filter(|p| !p.is_null())
                .map(|_| param(params, "parent"))
                .transpose()?;
            json_of(api.add_bone(
                param(params, "layer")?,
                &param::<String>(params, "name")?,
                parent,
                param(params, "x")?,
                param(params, "y")?,
                param(params, "length")?,
            )?)
        }
        "set_bone_angle" => {
            let frame = params
                .get("frame")
                .filter(|f| !f.is_null())
                .map(|_| param(params, "frame"))
                .transpose()?;
            api.set_bone_angle(
                param(params, "layer")?,
                param(params, "bone")?,
                param(params, "degrees")?,
                frame,
            )?;
            Ok(Value::Null)
        }
        "bake_spring" => {
            let optional = |key: &str| -> Result<Option<i32>, ApiError> {
                params
                    .get(key)
                    .filter(|v| !v.is_null())
                    .map(|_| param(params, key))
                    .transpose()
            };
            let bones: Vec<u32> = param(params, "bones")?;
            let settings: crate::SpringSettings =
                serde_json::from_value(params.clone()).map_err(|e| bad(format!("params: {e}")))?;
            json_of(api.bake_spring(
                param(params, "layer")?,
                &bones,
                settings,
                optional("start")?,
                optional("end")?,
            )?)
        }
        "set_text_animator_value" => {
            let frame = params
                .get("frame")
                .filter(|f| !f.is_null())
                .map(|_| param(params, "frame"))
                .transpose()?;
            api.set_text_animator_value(
                param(params, "layer")?,
                param(params, "index")?,
                &param::<String>(params, "property")?,
                param(params, "value")?,
                frame,
            )?;
            Ok(Value::Null)
        }
        "set_text_path" => {
            api.set_text_path(param(params, "layer")?, param(params, "shape_layer")?)?;
            Ok(Value::Null)
        }
        "clear_text_path" => {
            api.clear_text_path(param(params, "layer")?)?;
            Ok(Value::Null)
        }
        "set_text_path_offset" => {
            api.set_text_path_offset(param(params, "layer")?, param(params, "percent")?)?;
            Ok(Value::Null)
        }
        "import_captions" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or("");
            json_of(api.import_captions(&param::<String>(params, "content")?, name)?)
        }
        "export_captions" => {
            json_of(api.export_captions(param(params, "layer")?, &param::<String>(params, "format")?)?)
        }
        "add_layer" => json_of(
            api.add_layer(
                &param::<String>(params, "kind")?,
                &param::<String>(params, "name")?,
                params
                    .get("parent")
                    .filter(|p| !p.is_null())
                    .map(|_| param(params, "parent"))
                    .transpose()?,
            )?,
        ),
        "delete_layer" => {
            api.delete_layer(param(params, "id")?)?;
            Ok(Value::Null)
        }
        "set_document" => {
            api.set_document(
                serde_json::from_value(params.clone()).map_err(|e| bad(format!("params: {e}")))?,
            )?;
            Ok(Value::Null)
        }
        "set_layer_transform" => {
            let id = param(params, "id")?;
            let change = serde_json::from_value(params.clone()).map_err(|e| bad(format!("params: {e}")))?;
            api.set_layer_transform(id, change)?;
            Ok(Value::Null)
        }
        "add_shape" => json_of(api.add_shape(param(params, "layer")?, param(params, "shape")?)?),
        "undo" => json_of(api.undo()?),
        "redo" => json_of(api.redo()?),
        other => Err(ApiError::new(
            ErrorCode::NotFound,
            format!("there is no method \"{other}\""),
        )),
    }
}

/// JSON-RPC 2.0 error codes the protocol uses (the spec's own).
const PARSE_ERROR: i64 = -32700;
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;
/// An error of the API's own that has no JSON-RPC code of its own (`not_found`, `playing`).
const SERVER_ERROR: i64 = -32000;

/// One line of the protocol, which is JSON-RPC 2.0 with one request or one
/// notification per line: `{"jsonrpc": "2.0", "id": 1, "method": "layers",
/// "params": {}}` in, `{"jsonrpc": "2.0", "id": 1, "result": ...}` or
/// `{"jsonrpc": "2.0", "id": 1, "error": {"code": -32602, "message": ...,
/// "data": {"code": "invalid_argument"}}}` out. The API's own stable code is
/// always in `error.data.code`. A request without an `id` is a notification:
/// it is run and nothing is answered (`None`). A line that is not a request
/// is answered with an error and a `null` id, and the host carries on.
/// (`"jsonrpc"` may be left out of a request; every answer has it.)
pub fn handle_line(api: &mut Api<'_>, line: &str) -> Option<String> {
    let request: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(error(
                Value::Null,
                PARSE_ERROR,
                ErrorCode::InvalidArgument,
                &format!("not JSON: {e}"),
            ));
        }
    };
    let notification = request.is_object() && request.get("id").is_none();
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = request.get("method").and_then(Value::as_str) else {
        return Some(error(
            id,
            INVALID_REQUEST,
            ErrorCode::InvalidArgument,
            "a request is an object with a \"method\" string",
        ));
    };
    let params = request.get("params").cloned().unwrap_or(Value::Null);
    let answer = if METHODS.iter().any(|m| m.name == method) {
        match call(api, method, &params) {
            Ok(result) => reply(id, result),
            Err(e) => failure(id, e),
        }
    } else {
        error(
            id,
            METHOD_NOT_FOUND,
            ErrorCode::NotFound,
            &format!("there is no method \"{method}\""),
        )
    };
    (!notification).then_some(answer)
}

/// A successful answer.
pub fn reply(id: Value, result: Value) -> String {
    json!({"jsonrpc": "2.0", "id": id, "result": result}).to_string()
}

/// A failed answer with JSON-RPC code `rpc_code` and the API's own code.
pub fn error(id: Value, rpc_code: i64, code: ErrorCode, message: &str) -> String {
    json!({"jsonrpc": "2.0", "id": id, "error": {
        "code": rpc_code, "message": message, "data": {"code": code.as_str()}}})
    .to_string()
}

/// A failed answer for an [`ApiError`].
pub fn failure(id: Value, e: ApiError) -> String {
    let rpc_code = match e.code {
        ErrorCode::InvalidArgument => INVALID_PARAMS,
        _ => SERVER_ERROR,
    };
    error(id, rpc_code, e.code, &e.message)
}
