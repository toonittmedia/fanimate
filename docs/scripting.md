# Scripting Fanimate

*New in the development version (API `0.1.0-unstable`); not in a release yet.*

Fanimate can be driven by a program with no window: `fanimate-cli script` opens a project (or an empty one) and answers [JSON-RPC 2.0](https://www.jsonrpc.org/specification) requests, one per line, on standard input and output. A script in any language starts it as a child process, so nothing of the engine has to be linked into the script, and the same calls are what plugins and the AI tools will use. Every call is one undo step, all or nothing, and is refused while the animation plays.

```
fanimate-cli script scene.fnm            # open a project; "save" writes it back
fanimate-cli script scene.fnm --save out.fnm
fanimate-cli script                      # an empty 1920 by 1080 project
```

## Over a local socket

`fanimate-cli serve [project.fnm] [--save out.fnm] [--port N]` offers the same calls on a socket, for a tool or plugin that connects to a Fanimate that is already running. It listens on `127.0.0.1` only (this computer) and prints one line of JSON: `{"host": "127.0.0.1", "port": 50123, "token": "...", "api_version": "..."}`. The token is 128 random bits made for this run. **The first request on a connection must be `auth`**: `{"jsonrpc": "2.0", "id": 0, "method": "auth", "params": {"token": "..."}}`; anything else, or a wrong token, is answered with the error code `unauthorized` and the connection is closed. After that the calls are the ones below. One connection is served at a time and all of them work on the one project, so what a client did is there for the next one; `quit` ends your connection and `shutdown` stops the host (save first). A line longer than 8 MB ends the connection with an error.

From Python: `host = fanimate.Host.connect(port, token)`.

## The protocol

A request is a JSON object on one line: `{"jsonrpc": "2.0", "id": 1, "method": "add_text_layer", "params": {"content": "Hello", "x": 100, "y": 80}}`. The answer is one line with the same `id`:

- success: `{"jsonrpc": "2.0", "id": 1, "result": 3}`
- failure: `{"jsonrpc": "2.0", "id": 1, "error": {"code": -32602, "message": "...", "data": {"code": "invalid_argument"}}}`

A request without an `id` is a notification: it runs and nothing is answered. A line that is not a valid request is answered with an error (`-32700` not JSON, `-32600` not a request, `-32601` no such method, `-32602` bad parameters) and the session goes on. `"jsonrpc"` may be left out of a request.

**Match on `error.data.code`**, not on the number or the message. These strings are part of the API: `not_found` (nothing has that id, or no such method), `invalid_argument`, `playing` (the animation is playing), `io_error` (a file could not be read or written), `unauthorized` (a socket connection that has not shown the token).

## Methods

`api.methods` returns the list below (name, parameters, whether it edits, a sentence), so a client can discover what a build offers.

| Method | Parameters | Result |
|---|---|---|
| `api.version` | | The API version, such as `"0.1.0-unstable"` |
| `api.methods` | | The method list |
| `document` | | Name, size, rate, frame range, layer count, `.fnm` format version |
| `layers` | | Every layer, depth first: `id`, `name`, `kind`, `parent`, `visible`, `locked`, `children`, `provenance` |
| `layer` | `id` | One layer |
| `bones` | `layer` | The bones of a bone layer, by id |
| `text` | `layer` | A text layer's words, look, counts, animators and whether it is on a path |
| `rename_layer` | `id`, `name` | |
| `rename_bone` | `layer`, `bone`, `name` | |
| `set_layer_visible` | `id`, `visible` | |
| `set_layer_provenance` | `id`, `provenance` (object or `null`) | |
| `add_text_layer` | `content`, `x`, `y` | The new layer's id |
| `set_text_content` | `layer`, `content` | |
| `set_text_style` | `layer`, `style` (`family`, `size`, `weight`, `tracking`, `line_height`, `width`, `wrap`) | |
| `add_text_animator` | `layer`, `preset` (`fade_in`, `typewriter`, `rise_in`, `pop_in`, `wave`) | The animator's index |
| `set_text_animator_value` | `layer`, `index`, `property`, `value`, optional `frame` | Sets one value of an animator by the keying rule. Properties: `range_start`, `range_end`, `range_offset`, `position`, `anchor`, `rotation` (degrees), `scale`, `opacity`, `tracking`, `stroke_width` (numbers, or `[x, y]` for `position`, `anchor` and `scale`), `fill` and `stroke_color` (`"#RRGGBB"`) |
| `set_text_path` | `layer`, `shape_layer` | |
| `clear_text_path` | `layer` | |
| `set_text_path_offset` | `layer`, `percent` | |
| `add_layer` | `kind` (`vector`, `group`, `switch`), `name`, optional `parent` | The new layer's id |
| `delete_layer` | `id` | |
| `set_document` | any of `name`, `width`, `height`, `fps`, `start`, `end` | |
| `set_layer_transform` | `id`, optional `frame` (0, the rest pose, by default), `x`, `y`, `rotation` (degrees), `scale_x`, `scale_y`, `opacity` | On frame 0 the value changes; on a later frame a key is set and one is seeded on frame 0 |
| `add_shape` | `layer`, `shape` (`kind` `rectangle` or `ellipse`, `x`, `y`, `width`, `height`, `radius`, `fill`, `stroke`, `stroke_width`; colours like `#e63946`) | The shape's index |
| `actions` | `layer` | The bone layer's actions: `index`, `name`, `kind` (`action`, `pose`, `smart_bone`), `length` |
| `save_pose` | `layer`, optional `name`, `frame` | The pose's index: the whole rig on that frame (0 is the rest pose), kept as a one-frame action |
| `mirror_pose` | `layer`, `index` | The index of a new pose, the left-for-right mirror of the pose by the bones' names (`arm L` swaps with `arm R`) |
| `insert_action` | `layer`, `index`, `frame` | Puts an action or pose into the timeline from that frame |
| `import_svg` | `svg` (the file's text), `name` | The new layer's id: each top-level group becomes a vector layer and each `<text>` a text layer |
| `controllers` | `layer`, optional `frame` | The bone layer's master controllers: `index`, `name`, `kind` (`slider`, `pad`), `x`, `y` (0 to 1 on that frame), `links` |
| `add_controller` | `layer`, `name`, `kind` (`slider` or `pad`) | The controller's index |
| `rename_controller` | `layer`, `index`, `name` | |
| `curves` | `layer` | The layer's keyed curves, what the graph editor draws: `channel` (`position`, `rotation`, `scale`, `opacity`, `order`, `bone:<id>`), `component` (0, or 0 and 1 for x and y), and `keys` of `frame`, `value` (degrees for angles, percent for opacity) and `interp` (as the file writes it) |
| `move_key` | `layer`, `channel`, `component`, `frame`, `to_frame`, `value` | Moves the key on `frame` to `to_frame` and sets that curve's value there; the key stays between its neighbours; returns the frame it landed on |
| `set_key_interpolation` | `layer`, `channel`, `frame`, `interp` | `"smooth"`, `"linear"`, `"step"`, `{"ease": "bounce"}` or `{"bezier": [x1, y1, x2, y2]}` in thousandths; returns whether it changed |
| `remove_controller` | `layer`, `index` | |
| `link_controller` | `layer`, `controller`, `action`, `axis` (`x` or `y`), `from`, `to` | The controller plays the action as its value goes from `from` (action on its first frame) to `to` (its last); linking the same action and axis again replaces the link; Smart Bone actions cannot be linked |
| `unlink_controller` | `layer`, `controller`, `action` | Whether it had a link to take away |
| `set_controller_value` | `layer`, `controller`, `axis`, `value` (0 to 1), optional `frame` | By the keying rule: frame 0 is the rest pose |
| `add_bone_layer` | `name` | The new bone layer's id |
| `add_bone` | `layer`, `name`, optional `parent` (a bone id), `x`, `y`, `length` | The bone's id (ids never change when bones are moved or removed) |
| `set_bone_angle` | `layer`, `bone`, `degrees`, optional `frame` | Keyed by the keying rule: frame 0 is the rest pose |
| `bake_spring` | `layer`, `bones` (ids), optional `stiffness`, `damping`, `inertia`, `gravity`, `wind`, `start`, `end` | How many keys were written: the bones follow their keys on a spring, thrown by the bones above, and the swing replaces their keys on every frame of the range |
| `import_captions` | `content` (SRT or WebVTT text), optional `name` | `{layer, captions, skipped, cut_off}`: a switch layer with one text layer per caption, shown at its time |
| `export_captions` | `layer`, `format` (`"srt"` or `"vtt"`) | The captions as text |
| `undo`, `redo` | | Whether there was a step to undo or redo |
| `save` | optional `path` | Writes the project (this host only) |
| `quit` | | Ends the session (this host only) |

Layers, bones and other things are named by their ids, which never change when something is moved or reordered. Frames count from 1 and frame 0 is the rest pose, as everywhere else.

## An example

From Python there is a small client in [`clients/python`](../clients/python/README.md) (standard library only): `with fanimate.Host() as host: host.add_text_layer(content="Hi", x=10, y=10)`. Without it, the protocol is plain JSON lines, so a script can speak it directly. A Python script (the `python` on your path, nothing to install):

```python
import json, subprocess

host = subprocess.Popen(["fanimate-cli", "script", "--save", "title.fnm"],
                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
n = 0
def call(method, **params):
    global n
    n += 1
    host.stdin.write(json.dumps({"jsonrpc": "2.0", "id": n, "method": method, "params": params}) + "\n")
    host.stdin.flush()
    answer = json.loads(host.stdout.readline())
    if "error" in answer:
        raise RuntimeError(answer["error"]["data"]["code"] + ": " + answer["error"]["message"])
    return answer["result"]

title = call("add_text_layer", content="Made by a script", x=120, y=90)
call("set_text_style", layer=title, style={"size": 96, "weight": 700})
call("add_text_animator", layer=title, preset="typewriter")
call("save")
call("quit")
```

Then `fanimate-cli render title.fnm -o frame.png --frame 30` or `fanimate-cli export title.fnm -o title.mp4`.

## Stability

Until Fanimate 1.0 the API is **unstable**: the version ends in `-unstable`, and `crates/fanimate-api/api-surface.txt` lists the Rust surface so that a change is a visible diff. Method and parameter names follow the Rust calls. After the 1.0 freeze, changes follow semantic versioning and a deprecated call keeps working for two major releases. This is a plan, and the freeze is a Gate 1 criterion, not a promise made today.

Not built yet: a local socket, permissions for plugins, the Python package, the TypeScript and WASM hosts, and calls for freehand drawing, bones, Smart Bones, actions, keys on the timeline and rendering (the API grows with each milestone; `fanimate-cli render` and `export` render a saved file).
