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
| `fonts` | | `bundled` (shipped, the same everywhere) and `installed` (on this computer) font families a text can name; a project that names an installed font is set in the default font on a computer without it |
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
| `retarget_action` | `from`, `action`, `to` | Copies an action of bone layer `from` onto bone layer `to` by bone names (case, spaces, dots and underscores do not matter), keeping each bone's turn from its own rest angle; bones with no match and shapes' point keys are left out; returns the new action's index in `to` |
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
| `bake_expression` | `layer`, `channel`, `component`, `expression`, `first`, `last` | Works the [expression](#expressions) out on every frame from `first` to `last` and writes the answers as one key a frame on that curve (`channel` as `curves` names it: `position`, `rotation`, `scale`, `opacity`, `order`, `bone:<id>`; `component` 0 is the number or x, 1 is y). `value` in the expression is what the curve had, in the units `curves` reports. Returns how many keys. Nothing changes if any frame has no answer |
| `import_ora` | `data` (the `.ora` file, base64), `name` | Imports an OpenRaster file (Krita, MyPaint, GIMP) as a group of image layers, each with its place, opacity and visibility; blend modes other than normal are shown as normal. Returns the group's id |
| `restyle_captions` | `layer` (one caption's text layer) | How many other captions in its set changed to look like it (look, animators and place, not the words) |
| `undo`, `redo` | | Whether there was a step to undo or redo |
| `save` | optional `path` | Writes the project (this host only; needs the `save` permission) |
| `quit` | | Ends the session (this host only) |

Layers, bones and other things are named by their ids, which never change when something is moved or reordered. Frames count from 1 and frame 0 is the rest pose, as everywhere else.

## Permissions and plugins

A host can limit what its client may do. There are three permissions, from least to most:

| Permission | Lets the client |
|---|---|
| `read` | call every query (`layers`, `text`, `curves`, `fonts`, ...) |
| `edit` | call everything that changes the project, and `undo` and `redo` (also `shutdown`, which stops a shared host for everyone) |
| `save` | have the host write the project to disk (`save`) |

`quit` (ending your own session) needs none. `api.methods` lists the permission each method needs. A call the client may not make is answered with the error code `permission_denied` and does nothing; asking for a method that does not exist is still `not_found`.

`fanimate-cli script` and `serve` give their client everything unless told otherwise, because whoever starts the host is the author of the script. `--permissions read,edit` gives it only those, and `--plugin plugin.json` gives it what a plugin's manifest asks for (use one or the other). From Python: `fanimate.Host(permissions=["read"])` or `fanimate.Host(plugin="plugin.json")`.

A plugin describes itself in `plugin.json` (at most 64 KB; fields this Fanimate does not know are ignored):

```json
{
  "name": "Title Maker",
  "version": "1.2.0",
  "api": "0.1",
  "description": "Makes lower thirds from a list of names",
  "entry": "main.py",
  "permissions": ["read", "edit"]
}
```

`name` (1 to 64 letters, digits, spaces, `-`, `_`, `.`), `version`, `api` (the API's major and minor version the plugin was written for: while the API is unstable a different minor version is refused) and `permissions` (a list, possibly empty) are required. `entry` is the file or command that starts the plugin, relative to its folder: it may not be absolute, hold a drive, or contain `..`. `fanimate-cli plugin check plugin.json` reads a manifest and says what it asks for, or what is wrong with it.

What is built is the manifest and the permission check in the script and socket hosts. The plugin loader that starts `entry` and the user's prompt that grants a plugin its permissions arrive with the app's plugin host.

## Expressions

`bake_expression` works a small expression out on every frame. There are no loops, no variables to set and no recursion, so it always finishes; every function is a pure function of its arguments, so the same frame gives the same number on every machine and in any order. A result that is not a finite number (a division by zero, a square root of a negative) is an error that names the frame, and nothing is written.

```
value + 12 * sin(frame / 4)         a wobble on top of what is keyed
100 - blink(frame, 7, 96, 4) * 100  opacity drops to 0 for 4 frames, once somewhere in each 96
clamp(noise(time * 2, 3), 0, 1)     a drifting number between 0 and 1
if(frame < 24, 0, 100)              a step
```

Everything is a number (a comparison gives 1 or 0, and anything but 0 is true). Operators, tightest first: `^` (to the right), unary `-` and `!`, `* / %` (a remainder is never negative), `+ -`, `< > <= >= == !=`, `&&`, `||`. An expression is at most 1,000 characters and 200 parts, and nests at most 24 deep. A bake covers at most 20,000 frames.

| Names | Meaning |
|---|---|
| `frame`, `time`, `fps` | the frame being worked out, its time in seconds from the first frame, and the frame rate |
| `start`, `end` | the animation's first and last frame |
| `first`, `last`, `progress` | the range being baked, and how far through it the frame is (0 to 1) |
| `value` | what the property has on that frame now, as the graph shows it (degrees, percent) |
| `pi`, `e` | constants |
| `sin cos tan abs sqrt floor ceil round sign fract` | one number in |
| `min max pow mod` | two numbers in |
| `clamp(x, lo, hi)`, `lerp(a, b, t)`, `smoothstep(a, b, x)`, `if(c, a, b)` | three in (`if` only works out the branch it takes) |
| `noise(x, seed)` | smooth random between -1 and 1 that drifts as `x` changes |
| `random(seed, n)` | a random number from 0 up to (not including) 1 for each whole `n` |
| `blink(t, seed, period, length)` | 1 for `length` frames at a random place in every `period` frames, else 0 |

The keys it writes are ordinary keys (straight lines between frames), so nothing here is needed to play the file. The panel for typing an expression in the app is not built yet; use the API.

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
