# The `.fnm` project format

**Current version: 6. Status: draft.** The format may still change before Fanimate 1.0, but every change that older readers would misread bumps the version, and Fanimate always opens files written by earlier versions. See [Version history](#version-history).

The reference implementation is the [`fanimate-fnm`](../../crates/fanimate-fnm/) crate.

## Design goals

- **Open and documented.** Anyone can read and write `.fnm` without Fanimate's code.
- **Version-control friendly.** Plain JSON, split into one file per top-level layer, so Git diffs are readable and two artists rarely edit the same file.
- **Never lose work.** Saves are atomic; loads are validated and damage is repaired or reported, never silently kept.
- **Lossless.** Saving and loading a project gives back exactly the same project.

## Layout

A project is one file whose name ends in `.fnm`. The file is a ZIP archive, and the archive holds this folder layout:

```text
manifest.json
document.json
thumbnail.png        (optional)
layers/
├── 0002.json
└── 0005.json
media/
└── 9f3a…-20481.png
```

A **project folder** with the same layout (a folder whose name ends in `.fnm`) is the other form, and is equal to the file: both open, and the content of every file is the same. Fanimate saves a project that is a folder as a folder, so projects kept in Git keep their per-layer diffs; everything else is saved as a single file.

All files are UTF-8 JSON, apart from the media files. Whitespace is not significant; Fanimate writes them indented, with a final newline.

### The single file

- **Any ZIP reader opens it.** Entries are stored or deflated (methods 0 and 8), names are UTF-8 with `/` separators, and every entry has a CRC-32. ZIP64 is not used, so a project over 4 GiB cannot be saved as a single file (Fanimate says so rather than writing a damaged file).
- **The folder layout may sit at the root or one folder down**, so a project folder zipped by hand also opens.
- **Fanimate checks every entry's checksum** when it opens a single file. An entry that does not match is reported as damaged; a project with a damaged layer does not open silently with that layer missing.
- **Saving is atomic.** The new file is written beside the old one as `.<name>.saving`, read back and checked (it must open and have the same layers), flushed to disk, and only then renamed over the old file. A crash or a full disk leaves the old file as it was; a failed save leaves no stray files.
- **Saving the same project twice writes the same bytes** (entries in a fixed order, a fixed timestamp), so the file works with tools that compare or deduplicate files.
- **The file starts with `PK`** (a ZIP signature). The type is told by the `.fnm` extension and by the `manifest.json` inside. On Linux the shared MIME type is `application/x-fanimate-project` with the glob `*.fnm`; on macOS the type is `app.fanimate.project`.

## The preview picture

`thumbnail.png`, at the root of the project (version 6, optional), is a small picture of the project for file browsers and Fanimate's Home tab. It is a PNG of at most 262 144 bytes (256 KiB); Fanimate writes one of about 320 × 180 pixels. It is not part of the animation: nothing depends on it, a project without one is complete, and a reader that does not know it ignores the entry. A reader must ignore one that is not a PNG (it does not start with the PNG signature) or is larger than the limit, rather than load it. Like every other entry it is covered by the single file's checksums and written by the same atomic save. A save that makes no new preview keeps the one in the project, so it may show an earlier state of the animation.

## Media files

Pictures used by image layers live in the `media` folder, as PNG or JPEG files, and sounds used by audio layers as WAV files. Fanimate names each file after its content (a hash and the size in bytes), so the same picture is stored once and a file never changes once written. A file no layer uses is not saved. A missing file does not stop the project opening: a picture shows as a grey placeholder and a sound is silent.

## `manifest.json`

Identifies the project. A folder or archive without it is not a project.

```json
{
  "format": "fnm",
  "version": 6,
  "generator": "fanimate 0.0.6"
}
```

| Field | Type | Meaning |
|---|---|---|
| `format` | string | Always `"fnm"`. Anything without it is not a project. |
| `version` | integer | Format version. Readers must refuse versions newer than they support. |
| `generator` | string | Program that wrote the file. Informational only. |

## `document.json`

Canvas, timing, and the order of top-level layers.

```json
{
  "name": "Hello Fanimate",
  "width": 1280.0,
  "height": 720.0,
  "fps": 24.0,
  "start": 1,
  "end": 48,
  "background": "#1C0B3D",
  "next_layer_id": 3,
  "guides": [{ "vertical": true, "position": 640.0 }],
  "camera": { "position": { "base": [0, 0] }, "zoom": { "base": 1.0, "keys": [{ "frame": 1, "value": 1.0 }, { "frame": 25, "value": 2.0 }] }, "roll": { "base": 0.0 } },
  "layers": ["layers/0002.json"]
}
```

| Field | Type | Meaning |
|---|---|---|
| `name` | string | Project name |
| `width`, `height` | number | Canvas size in document units (pixels at 100% zoom). Positive. |
| `fps` | number | Frames per second. Positive. |
| `start`, `end` | integer | Inclusive frame range that plays and renders. `0 <= start <= end`. Fanimate starts new projects at 1, so the rest pose on frame 0 is outside the range. Before version 6 the first frame was 0; see [Frames](#frames). |
| `background` | [colour](#colours) | Canvas colour |
| `next_layer_id` | integer | Next unused layer id. Greater than every id in the project. |
| `guides` | array | Optional (version 5). Guides placed from the rulers: `vertical` (true for a vertical line at `position` on the x axis, false for a horizontal one on the y axis), `position` (document units) and optionally `locked` (true when it cannot be dragged). At most 1000. |
| `styles` | array | Optional (version 6), left out when empty. The brush and shape styles saved with the project, so another computer has them. Each is `name` (1 to 60 characters, unique), `line` (a [colour](#colours)), `width` (above 0), `fill` (a colour or `null`: no fill), `outline` (boolean), `smoothing` (0 to 1), `taper` (0 or more) and `pressure` (boolean: a pen's pressure sets a brush stroke's width). At most 64. |
| `camera` | object | Optional (version 5). The [camera](#the-camera). Left out when it looks at the whole canvas and has no keys. |
| `layers` | array of strings | Top-level layer files relative to the project folder, **bottom to top** (the last is drawn on top). |

## Layer files

Each file in `layers/` holds one top-level layer. Child layers of groups and bone layers are stored inline inside their parent, not in separate files. Fanimate names the file after the layer id (`layers/0002.json` for id 2), so renaming a layer does not rename its file.

### Common layer fields

| Field | Type | Meaning |
|---|---|---|
| `id` | integer | Unique within the project; stable for the layer's life |
| `name` | string | Display name |
| `label` | string | Optional (version 3). Label colour: `"red"`, `"orange"`, `"yellow"`, `"lime"`, `"green"`, `"teal"`, `"cyan"`, `"blue"`, `"violet"` or `"pink"`. Without it, Fanimate shows the colour at position `id` mod 10 in that list. Readers ignore names they do not know. |
| `visible` | boolean | Optional, default `true` |
| `opacity` | [channel](#channels) of number | 0 to 1. Multiplies with ancestors' opacity. |
| `locked` | boolean | Optional (version 5), default `false`. A locked layer cannot be selected on the canvas, moved, drawn on or edited by the tools. It still draws and plays. |
| `order` | [channel](#channels) of number | Optional (version 6), left out when it has no keys. See [Layer order](#layer-order). |
| `transform` | object | `position` (channel of [vector](#vectors)), `rotation` (channel of number, radians), `scale` (channel of vector), and optionally (version 4) `pivot` |
| `type` | string | `"vector"`, `"group"`, `"bone"`, `"image"`, `"frames"`, `"switch"` or `"audio"` (the last four version 3); decides the remaining fields |

A layer's transform applies scale, then rotation, then position, relative to its parent, about the layer's **pivot**.

`transform.pivot` (optional, version 4) is a [vector](#vectors): a point in the layer's own space that rotation and scale turn and stretch the layer about. It is not animated. A point `p` of the layer lands at `position + pivot + rotate(scale(p - pivot))` in its parent. The pivot stays put as the layer turns, and `position` moves the pivot with the layer. Without a pivot it is the layer's origin, `(0, 0)`, which is how every layer saved before version 4 turns; the field is left out when it is `(0, 0)`. A pivot that is not a finite point of at most 10 000 000 units from the origin is an error, repaired by removing it.

### Layer order

Layers in a list (the `children` of a group, bone or switch layer) are drawn in the order they are stored, first to last. `order` lets a layer change its place in that list over time, such as a hand that passes behind the body and then in front of it. Its keys are **slots**: whole numbers from 0 (drawn first, at the back) to the number of layers in the list minus 1 (drawn last, on top). Between keys the slot is held, as for a `"step"` key; a layer is never between two slots.

At each frame the layers in a list are sorted by their slot, lowest first. Layers with the same slot keep their order in the list (the sort is stable), and a layer with no `order` keys has its place in the list as its slot. A slot outside the list is held within it. The channel is not used for the layers in `document.json`; Fanimate writes it only for layers inside a group, bone or switch layer, and the list in the file is not reordered by it.

### `"type": "vector"`

| Field | Type | Meaning |
|---|---|---|
| `shapes` | array of [shapes](#shapes) | Drawn first to last |

### `"type": "group"`

| Field | Type | Meaning |
|---|---|---|
| `children` | array of layers | Drawn first to last |

### `"type": "frames"`

Frame-by-frame animation: a list of drawings, each shown from its frame until the next drawing's frame (its exposure). Version 3.

```json
{"type": "frames", "cels": [
  {"frame": 0, "line": [], "colour": []},
  {"frame": 4, "rough": [], "line": []}
]}
```

| Field | Type | Meaning |
|---|---|---|
| `cels` | array | Drawings, sorted by `frame`, at most one per frame, no negative frames. |
| `cels[].frame` | integer | The first frame the drawing is shown on |
| `cels[].rough`, `.colour`, `.line`, `.overlay` | arrays of [shapes](#shapes) | Optional. The drawing's sub-layers, drawn in that order (rough at the bottom). Renders leave `rough` out. |

Nothing is shown before the first drawing. Shapes in drawings may be bound to bones like any shape.

### `"type": "switch"`

Shows one child at a time, such as mouth shapes for lip sync, hands or eyes. Version 3.

```json
{"type": "switch", "children": [], "switch": [
  {"frame": 0, "value": "rest", "interp": "step"},
  {"frame": 6, "value": "AI", "interp": "step"}
]}
```

| Field | Type | Meaning |
|---|---|---|
| `children` | array of layers | The layers to choose from |
| `switch` | array of [channel keys](#channels) whose values are strings | Optional. From each key's frame on, the child whose `name` matches the key's value (ignoring case) is shown. Sorted, one per frame, no negative frames. `interp` is written as `"step"` and has no other meaning here. |

Before the first key, or when a key names no child, the last child (the top of the stack) is shown.

### `"type": "audio"`

Sound on the timeline. Version 3. It draws nothing.

| Field | Type | Meaning |
|---|---|---|
| `media` | string | The WAV file, under the same rules as an image's `media` |
| `start` | integer | Optional, default 0. The frame the sound starts on; negative starts it part-way through. |
| `volume` | number | Optional, default 1 (as recorded). 0 is silent. Not negative. |

A hidden audio layer is silent.

### `"type": "image"`

A picture, such as one layer of an imported Photoshop file. Version 3.

| Field | Type | Meaning |
|---|---|---|
| `media` | string | The picture file, relative to the project folder: `media/` and a file name of letters, digits, `-`, `_` and `.` only. Readers must not read any other path. |
| `width`, `height` | number | Size in document units. The picture spans (0, 0) to (width, height) in the layer's space. Positive. |
| `grid` | `[columns, rows]` | Optional, default `[1, 1]`. The mesh the picture bends on: this many cells each way, 1 to 64. |
| `bone` | integer | Optional. Bone it follows rigidly, as for shapes. |
| `skin` | array | Optional. [Smooth binding](#smooth-binding) weights, one entry per mesh vertex (`(columns + 1) × (rows + 1)`), row by row from the top left. Takes the place of `bone`. |

The picture is drawn on the mesh: its vertices, evenly spaced over the picture, move with their bones and the picture stretches between them.

### `"type": "bone"`

A skeleton. Shapes on vector layers anywhere inside `children` can bind to its bones.

| Field | Type | Meaning |
|---|---|---|
| `skeleton.bones` | array of [bones](#bones) | Parents always come before their children |
| `skeleton.actions` | array of [actions](#actions) | Optional (version 3) |
| `children` | array of layers | Drawn first to last |

## Shapes

```json
{
  "name": "middle arm",
  "path": [{"M": [542.0, 322.0]}, {"L": [684.0, 322.0]}, "Z"],
  "style": {"fill": "#FFFFFF", "stroke": {"color": "#5EEAD4", "width": 6.0}},
  "bone": 0
}
```

| Field | Type | Meaning |
|---|---|---|
| `name` | string | Display name |
| `path` | array of [path commands](#paths) | Outline, in the layer's space |
| `style.fill` | colour | Optional. Non-zero fill rule. |
| `style.stroke` | object | Optional. `color` and `width` (document units). |
| `bone` | integer | Optional. Index of the bone this shape follows, in the nearest enclosing bone layer. |
| `widths` | array of numbers | Optional (version 1). The stroke's [width profile](#width-profiles). |
| `path_keys` | array of keys | Optional (version 2). [Point keyframes](#point-keyframes): the path at chosen frames. |
| `skin` | array | Optional (version 3). [Smooth binding](#smooth-binding): per on-curve point, the bones it follows. Takes the place of `bone`. |
| `visible` | boolean | Optional (version 5), default `true`. A hidden shape is not drawn, hit or edited, and keeps its place in the layer. |
| `trim` | `[start, end]` | Optional (version 5), default `[0, 1]`. [Stroke exposure](#stroke-exposure): the part of the stroke that is drawn. |
| `trim_keys` | array of keys | Optional (version 5). Keys for `trim`, with `value` as `[start, end]`, like a channel's. |
| `hidden_edge_flags` | array of booleans | Optional (version 6), left out when no edge is hidden. One flag for every command of `path`: `true` leaves that segment's stroke undrawn. See [Hidden edges](#hidden-edges). |
| `fill_keys` | array of keys | Optional (version 5). [Colour keys](#colour-keys) for `style.fill`. |
| `stroke_keys` | array of keys | Optional (version 5). Colour keys for `style.stroke.color`. |
| `rect` | object | Optional (version 4). Says the shape is a rectangle: `x`, `y`, `width`, `height` and `radius` (the corner radius, 0 to half the shorter side), in the layer's space, and optionally (version 5) `radii`, four radii for the corners. See [Rectangles](#rectangles). |

A bound shape's points are stored in the **rest pose**. At each frame the shape moves rigidly with its bone: from the bone's rest transform to its current transform. Weighted deformation (one point following several bones) will be added in a later version.

### Rectangles

`rect` records how a rectangle was made, so that its corner radius can still be changed after it is drawn. **The `path` is the shape**: a reader that ignores `rect` draws and edits the path as it always did. A reader that uses `rect` must use it only while the path is still the one the rectangle makes: four straight sides (`M`, `L`, `L`, `L`, `Z`, starting at `(x, y)`) when `radius` is 0, and with a radius above 0 four straight sides and four cubic corners (`M`, `L`, `C`, `L`, `C`, `L`, `C`, `L`, `C`, `Z`, starting at `(x + radius, y)`), both going clockwise on screen, within 0.01 of each point. If the path differs (a point was edited), or the shape has a width profile, point keys or a smooth binding, the shape is an ordinary path and `rect` means nothing. Fanimate does not need the field removed when it stops applying; the check is made when it is used.

With `radii` (version 5, `[top left, top right, bottom right, bottom left]`, each 0 to half the shorter side) the corners have their own radii and `radius` only remembers the last one set for all four. The path goes clockwise from the top left: it starts at `(x + radii[0], y)`, then for the top right, bottom right, bottom left and top left corners in turn draws a line to where that corner's arc begins and, when the radius is above 0, the cubic arc. A corner with no radius contributes only the line, and the line that would return to the start is left out when the first corner is square. This is what a reader needs to check that the path still is the rectangle; it gives the same path as before for a rectangle whose corners are alike.

Values that are not finite, a width or height of 0 or less, or a radius outside 0 to half the shorter side, are a warning, repaired by removing `rect`.

### Stroke exposure

`trim` limits the stroke to the part from `trim[0]` to `trim[1]`, each a fraction (0 to 1) of the length of every subpath, measured along the path (curves by their arc length). The fill is not trimmed. A `[0, 1]` trim, the default, draws the whole stroke, and a trimmed stroke is drawn as an open path with its width profile cut to match. Equal ends draw nothing. `trim_keys` animate it like a channel (sorted, one per frame, no negative frames, `interp` as for channels); `trim` is the value when there are none.

### Hidden edges

`hidden_edge_flags` has one boolean for every command of `path`, in order. A `true` on a `LineTo` or `CubicTo` leaves the stroke of that segment undrawn, and on a `Close` leaves the closing edge undrawn. The points, the fill and the other segments are unchanged; the shape is still hit, selected and edited as before. A `MoveTo` draws nothing, so its flag must be `false`. A shape's point keys have the same commands as its path, so the flags apply on every frame.

The stroke is drawn from the segments that are not hidden, as open pieces (a closed shape with every edge drawn keeps its corners joined); with a width profile the profile is cut to match. Stroke exposure applies to what is left. A list with the wrong length for the path is ignored and every edge is drawn, so a tool that changes the commands without updating the flags shows the edges again rather than hiding the wrong ones.

### Width profiles

Brush strokes vary in width along their length, from pen pressure and tapered ends. A shape records this as `widths`: one number per **on-curve point** of its path, each a multiple of `style.stroke.width`.

```json
{
  "name": "Stroke",
  "path": [{"M": [10.0, 40.0]}, {"C": [[20.0, 30.0], [40.0, 30.0], [50.0, 40.0]]}, {"L": [90.0, 40.0]}],
  "style": {"stroke": {"color": "#1C0B3D", "width": 8.0}},
  "widths": [0.05, 1.0, 0.3]
}
```

- On-curve points are the points of `M`, `L` and the end point of `C`, in order; `Z` has none. The example has three, so `widths` has three values, and the stroke is 0.4, 8 and 2.4 units wide at them.
- Between two points the width changes smoothly along the segment.
- Values are finite and not negative. A profile whose length does not match the path is ignored: the stroke is drawn at `style.stroke.width` everywhere.
- A stroke with a profile has round caps and round joins. Without one, caps are flat (butt) and joins are mitred.
- `widths` has no effect on fills, or on a shape without a stroke.

### Smooth binding

A shape can follow several bones, each point blending their movements, so limbs bend smoothly at joints:

```json
"skin": [[[0, 1.0]], [[0, 0.6], [1, 0.4]], [[1, 1.0]]]
```

- One entry per **on-curve point**, in order, as for `widths`. Each entry is a list of `[bone, weight]` pairs; `bone` indexes the nearest enclosing bone layer's skeleton.
- Weights are finite and not negative. They are normalised when drawn, so they need not add up to 1; a point whose weights add up to 0 does not move.
- At each frame a point moves by the weighted average of its bones' movements from their rest transforms to their current ones. A curve handle moves with the on-curve point it leaves from (the first control point of a `C`) or arrives at (the second).
- Points (and point keys) are stored in the rest pose. A `skin` whose length does not match the path's on-curve points is ignored.

### Point keyframes

A drawing can change shape over time. `path_keys` holds the shape's path at chosen frames, as [channel](#channels) keys whose `value` is a [path](#paths):

```json
{
  "name": "Blob",
  "path": [{"M": [0.0, 0.0]}, {"L": [10.0, 0.0]}, {"L": [5.0, 8.0]}, "Z"],
  "style": {"fill": "#5EEAD4"},
  "path_keys": [
    {"frame": 0, "value": [{"M": [0.0, 0.0]}, {"L": [10.0, 0.0]}, {"L": [5.0, 8.0]}, "Z"]},
    {"frame": 12, "value": [{"M": [0.0, 0.0]}, {"L": [10.0, 0.0]}, {"L": [5.0, 14.0]}, "Z"], "interp": "linear"}
  ]
}
```

- Keys follow the channel rules: sorted by `frame`, at most one per frame, no negative frames, `interp` saying how the path moves on to the next key.
- **Every key has the same commands as `path`**, in the same order; only the points differ. The path is then interpolated point by point, control points included. A key whose commands differ is invalid.
- `path` is the shape's path when it has no keys, and the one point edits change while it has none. With keys, `path` is kept but the keys decide what is drawn, as a channel's `base` does.
- `widths` applies to every key alike. A shape bound to a bone stores its keys in the rest pose, like its path.

## Paths

A path is a list of commands, named after SVG's path letters. Points are `[x, y]`.

| Command | JSON | Meaning |
|---|---|---|
| Move | `{"M": [x, y]}` | Start a new subpath |
| Line | `{"L": [x, y]}` | Straight line to the point |
| Cubic | `{"C": [[x1, y1], [x2, y2], [x, y]]}` | Cubic Bézier: two control points, then the end point |
| Close | `"Z"` | Close the subpath |

Every subpath starts with `M`. Drawing commands with no open subpath (before the first `M`, or after a `Z` and before the next `M`) are ignored.

## Bones

```json
{
  "name": "arm",
  "parent": 0,
  "origin": [570.0, 350.0],
  "length": 110.0,
  "angle": {"base": 0.0, "keys": [{"frame": 12, "value": -0.35}]}
}
```

| Field | Type | Meaning |
|---|---|---|
| `name` | string | Display name |
| `parent` | integer | Optional. Index of the parent bone; must be lower than this bone's index. |
| `origin` | vector | Joint position in the parent bone's space (x along the parent bone), or in the bone layer's space for a root bone |
| `length` | number | Bone length |
| `angle` | channel of number | Rotation relative to the parent, radians. `base` is the **rest pose**. |
| `limits` | `[min, max]` | Optional (version 3). Lowest and highest angle, radians; the angle is kept within them when drawn and posed. |
| `pinned` | boolean | Optional (version 3), default `false`. Inverse kinematics does not move this bone or past it. |
| `strength` | number | Optional (version 3). How far the bone reaches when weights are made automatically; default half its length. |

Each bone's local space has its origin at the joint and +x pointing along the bone. Its transform at a frame is `parent_world × translate(origin) × rotate(angle)`.

## Actions

An action is a short animation kept on a bone layer, outside the main timeline.

```json
{
  "name": "head",
  "driver": 0,
  "length": 25,
  "bones": [{"bone": 0, "angle": {"base": 0.0, "keys": [{"frame": 1, "value": 0.0}, {"frame": 25, "value": 1.57}]}}],
  "shapes": [{"layer": 4, "shape": 0, "path_keys": [{"frame": 1, "value": []}, {"frame": 25, "value": []}]}]
}
```

(Paths shortened.)

| Field | Type | Meaning |
|---|---|---|
| `name` | string | Display name |
| `driver` | integer | Optional. Makes this a **Smart Bone action**: the index of the bone whose angle plays it. |
| `length` | integer | Last frame of the action, which starts at frame 1 (frame 0 before version 6, see [Frames](#frames)). Positive. |
| `bones` | array | Optional. Bone angle [channels](#channels) the action animates: `bone` (index) and `angle`. |
| `shapes` | array | Optional. [Point keys](#point-keyframes) of shapes in vector layers inside the bone layer (not inside nested bone layers): `layer` (id), `shape` (index in it) and `path_keys`. |

**Regular actions** (no `driver`) are clips the user copies into the timeline; they do not affect drawing by themselves.

**Smart Bone actions** play by themselves:

1. The driver's angle, as the timeline poses it, is looked up on the action's own keys for the driver, giving a frame of the action. Between two keys the frame is found by solving the channel for the angle; before the first or after the last key, the nearest end is used. An action whose driver has fewer than two keys does nothing.
2. Every other bone in `bones` turns by its channel's value at that frame minus its value at the action's first frame.
3. Every shape in `shapes` moves point by point by its keys' path at that frame minus their path at the action's first frame. Keys whose commands differ from the shape's path are ignored.
4. Several Smart Bone actions add up. Smart Bone actions do not move their own drivers or drive each other.

## Channels

Any animatable value is a channel:

```json
{"base": 0.0, "keys": [{"frame": 0, "value": 0.0}, {"frame": 12, "value": 1.0, "interp": "linear"}]}
```

| Field | Type | Meaning |
|---|---|---|
| `base` | value | The value when there are no keys. For bone angles, also the rest pose. |
| `keys` | array | Optional, default empty. Sorted by `frame`, at most one key per frame, no negative frames. |
| `keys[].frame` | integer | Frame number. Frame 0, the rest pose, is allowed. |
| `keys[].value` | value | Value at that frame |
| `keys[].interp` | string | How the value moves **from this key to the next**: `"smooth"` (default, ease in and out), `"linear"` or `"step"` (hold). Optional. |
| `loop_mode` | string | Optional (version 6), default `"off"`, left out when off. `"absolute"` or `"relative"`: what the value does after the last key. See [Looping](#looping). |

Sampling: before the first key, the first key's value; after the last key, the last key's value (unless the channel loops); between keys, interpolated as set by the earlier key. Frames can be sampled fractionally.

### Looping

With `loop_mode` set, a channel with at least two keys does not hold its last value. After the last key, time wraps back into the span from the first key to the last, and the value repeats the keys' motion for as long as the animation runs:

- `"absolute"`: each repeat takes the same values as the first, so the value jumps from the last key's value back to the first key's value at the start of every cycle.
- `"relative"`: each repeat starts where the last one ended. The net change of one cycle (last key's value minus the first key's) is added once for every cycle that has completed, so a walking character keeps moving forward and a wheel keeps turning. For a colour, the change is added to each of red, green, blue and alpha.

`loop_mode` on a channel with fewer than two keys means nothing; see [Validation](#validation). Channels with keys of their own kind, such as `path_keys`, `fill_keys`, `trim_keys` and switch keys, do not loop.

### Frames

Frames are integers. **From version 6 the first frame of an animation is 1**, and `start` is 1 for a new project (it was 0 before). **Frame 0 is the rest pose**: the project as it is built, before any animation. It is outside the playing range of a new project, but is a frame like any other for keys: a key on frame 0 is valid, and a channel with keys sampled on frame 0 follows the sampling rule above. When Fanimate keys a value on a later frame it also keys the value on frame 0 first, so that nothing jumps when the playhead goes back to the rest pose. Bone `angle` channels keep their rest pose in `base`, as before.

Files written before version 6 count from 0. Reading one shifts it: **every frame number in the project goes up by 1** (`start` and `end`, the `frame` of every key of every channel, point, colour, exposure and switch key, the `frame` of every drawing, the keys and `length` of every action, and an audio layer's `start`), so the project plays exactly as it did and frame 0 is left for the rest pose. No key is made on frame 0 for an old file. Guide `position`s are places on the canvas, not frames, and do not change.

### Colour keys

A shape's fill and stroke colour are animated with `fill_keys` and `stroke_keys`: keys like a channel's (`frame`, `value` as a [colour](#colours), optional `interp`), sorted, one per frame, none on negative frames. The colour in `style` is the value when there are no keys. Between keys the colour is interpolated in sRGB, each of red, green, blue and alpha as set by the earlier key. Keys without a fill (or stroke) to colour are removed on load.

### The camera

`camera` in `document.json` is the project's camera, not a layer. It has three channels: `position` (a vector, where the camera looks, in document units from the middle of the canvas, 0, 0 sees the canvas as it is), `zoom` (a number, 1 sees all of the canvas, 2 half of it twice as big; held within 0.05 to 50) and `roll` (radians, positive is clockwise). Renders and exports see the canvas through the camera; the editor's canvas shows everything unless it is set to look through the camera. A project with no `camera` has a camera that sees all of the canvas.

## Values

### Vectors

`[x, y]` as two numbers.

### Colours

sRGB, straight (not premultiplied) alpha. Written as a hex string, `"#RRGGBB"` or `"#RRGGBBAA"`, when 8 bits per channel hold the colour exactly. Otherwise written as `[r, g, b, a]` with each channel from 0 to 1, so saving never changes a colour. Readers must accept both, and `"#RGB"`.

### Coordinates and units

- Document space has **y pointing down**, origin at the top-left corner of the canvas.
- Angles are in **radians**; positive turns clockwise on screen.
- Lengths are in document units.
- All numbers must be finite.

## Saving

To make saving safe against crashes and full disks, a writer should:

1. Write the complete project to a temporary sibling folder (Fanimate uses `.scene.fnm.saving`).
2. Rename the existing project out of the way (`.scene.fnm.old`).
3. Rename the temporary folder into place.
4. Delete the old copy.

If step 3 fails, put the old copy back. At no point is the only copy of the project half-written.

## Crash recovery copies

While a project is open, Fanimate keeps a copy of it in its data folder, updated after every action, so a crash loses at most the action being written. The copy is an ordinary `.fnm` project folder (`project.fnm`) next to a `session.json` saying where the project is saved and whether it had unsaved changes, and a `history` folder with the undo history: an `index.json` listing each state's layers, and one file per distinct top-level layer state in `history/layers/`, so a layer unchanged between steps is stored once. Each file in the copy is replaced atomically. The copy is deleted when Fanimate closes normally. The history format is internal to Fanimate and may change between versions; a history that cannot be read is left out and the project recovered without it. So is the history of a copy written before version 6: its project is converted like any older project (see [Frames](#frames)), and a history holding the old frame numbers would no longer agree with it. Nothing else in this specification depends on it.

## Validation

Readers should validate on load. Fanimate's validator reports, and on request repairs, the following:

| Problem | Severity | Repair |
|---|---|---|
| Keyframe on a negative frame (frame 0 is valid) | Error | Remove the key |
| Keys out of order | Error | Sort by frame |
| Two keys on the same frame | Error | Keep the later one in the file |
| Non-finite value or key | Error | Reset the value; remove the key |
| Duplicate layer id | Error | Renumber the later layer |
| `next_layer_id` not above every id | Error | Set to highest id + 1 |
| Bone parent not before the bone | Error | Make the bone a root |
| Non-positive `fps`, `width` or `height`; bad frame range | Error | Reset to defaults |
| `start` below 0 | Error | Set to 1 |
| `end` before `start` | Error | Set to `start` |
| `loop_mode` other than `"off"` on a channel with fewer than two keys | Warning | Turn the loop off |
| `order` with keys that are not finite, or on negative frames, out of order or duplicated | Error | As for channel keys |
| `order` slot outside 0 to the number of layers in the list minus 1 | Warning | None; held within the list when drawn |
| Width profile whose length does not match the path's on-curve points | Error | Remove the profile |
| Point keyframe whose commands differ from the shape's path | Error | Remove the key |
| Point keyframes on negative frames, out of order, duplicated or not finite | Error | As for channel keys |
| Negative or non-finite value in a width profile | Error | Non-finite becomes 1, negative becomes 0 |
| Path with points that are not numbers | Error | Each such point moves to the origin, so the project can be saved |
| `hidden_edge_flags` whose length is not the number of path commands | Warning | Remove it: every edge is drawn |
| `hidden_edge_flags` that hides a `MoveTo` | Warning | Show it |
| `styles` entry that is unusable (a number that is not finite, a width of 0 or less, smoothing outside 0 to 1), has no name or repeats a name, or more than 64 styles | Warning | Remove it, and the extra ones; names are trimmed |
| Shape bound to a bone that does not exist, or outside any bone layer | Warning | Unbind the shape |
| `skin` whose length does not match the path's on-curve points | Error | Remove the skin |
| `skin` weight naming a missing bone, or not a finite non-negative number | Error | Remove that weight |
| `skin` outside any bone layer | Warning | Remove the skin |
| `transform.pivot` not finite, or over 10 000 000 units from the origin | Error | Remove the pivot |
| `guides` entry whose position is not finite, or more than 1000 guides | Warning | Remove it, and the extra ones |
| `camera` channel with bad keys (negative, out of order, duplicated or not finite) | Error | As for channel keys |
| `camera.zoom` outside 0.05 to 50 | Warning | Held within the range when evaluated |
| `fill_keys` or `stroke_keys` with bad keys or a colour that is not finite | Error | As for channel keys |
| `trim_keys` with bad keys or values that are not finite | Error | As for channel keys |
| `trim` or a trim key outside 0 to 1 | Warning | Held within 0 to 1 |
| `fill_keys` on a shape with no fill, `stroke_keys` on one with no stroke | Warning | Remove the keys |
| `rect` that is not finite, or has no size, or a radius outside its range | Warning | Remove `rect` |
| Bone `limits` not a finite range with min ≤ max | Error | Remove the limits |
| Bone `strength` not a positive number | Warning | Use the default |
| Drawings on negative frames | Error | Remove them |
| Drawings out of order or two on one frame | Error | Sort; of two on one frame, keep the later in the file |
| Image or audio `media` name outside the `media` folder | Error | None; the file is not read |
| Image picture file missing | Warning | None; drawn as a grey placeholder |
| Image size not positive | Error | Set to 100 × 100 |
| Image `grid` outside 1 to 64 | Error | Clamp it and remove the `skin` |
| Image `skin` not matching the mesh, or naming missing bones | Error | Remove the `skin` |
| Switch keys out of order, duplicated or negative | Error | As for channel keys |
| Audio `volume` negative or not finite | Error | Set to 1 |
| Sound file missing | Warning | None; the layer is silent |
| Action whose `driver` does not exist | Error | Remove the action |
| Action `length` not positive | Error | Set to 1 |
| Action keys for a bone that does not exist | Error | Remove those keys |
| Action keys out of order, duplicated, negative or not finite | Error | As for channel keys |
| Keyframe after the `end` frame | Warning | None; the key is kept |
| Opacity outside 0 to 1 | Warning | None; clamped when drawn |

## Versioning

- A change that older readers would misread bumps `version`.
- Readers refuse files with a newer version than they support, rather than guessing.
- Fanimate opens every older version, converting on load.
- Format changes are proposed in an issue and documented here before they ship (see [GOVERNANCE.md](../../GOVERNANCE.md)).

## Version history

| Version | First written by | Changes | Converting older files |
|---|---|---|---|
| 0 | Fanimate 0.0.1 | First version: layers, shapes, bones, channels | |
| 1 | Development builds after Fanimate 0.0.5 (never released) | Shapes may have a stroke width profile (`widths`), written by the brush | Version 0 files need no conversion. A version 0 reader would ignore `widths` and draw brush strokes at one width, so the version was bumped. |
| 2 | Fanimate 0.0.6 | Shapes may have point keyframes (`path_keys`) | Older files need no conversion. An older reader would ignore `path_keys` and draw animated drawings without their animation, so the version was bumped. |
| 3 | Fanimate 0.0.7 | Layers may have a label colour (`label`); bones may have `limits`, `pinned` and `strength`; shapes may have a smooth binding (`skin`); bone layers may have actions and Smart Bone actions; image layers and the `media` folder; frame-by-frame, switch and audio layers | Older files need no conversion. An older reader would drop labels and draw smoothly bound artwork unbound, so the version was bumped. |
| 4 | Development builds after Fanimate 0.2.1 | Layers may have a pivot (`transform.pivot`); shapes may say they are rectangles (`rect`) | Older files need no conversion: without a pivot a layer turns about its origin, as it always did, and without `rect` a shape is a path. An older reader would turn a layer with a pivot about its origin and show it in the wrong place, so the version was bumped. |
| 5 | Fanimate 0.2.3 | The project may have guides (`guides` in `document.json`); layers may be locked (`locked`); shapes may be hidden (`visible: false`); the project may have a camera (`camera`); shapes may have colour keys (`fill_keys`, `stroke_keys`), stroke exposure (`trim`, `trim_keys`) and rectangles may have a radius for each corner (`rect.radii`) | Older files need no conversion: they have no guides, camera or colour keys and nothing locked or hidden. An older reader would show a hidden shape, ignore a lock, draw the colour keys' shapes in one colour, draw whole strokes that are trimmed, round all four corners alike and lose the guides and camera when it saved, so the version was bumped. |
| 6 | Development builds after Fanimate 0.2.3 | Frames count from 1 and frame 0 is the rest pose; channels may loop (`loop_mode`); layers may change their place among their siblings over time (`order`); the project may hold a preview picture (`thumbnail.png`); shapes may hide edges (`hidden_edge_flags`); the project may hold saved brush styles (`styles`) | Files before version 6 are converted on load: every frame number goes up by 1 (`start`, `end`, every key, drawing and action frame, audio `start`), so the project plays as it did; no key is made on frame 0. Nothing else needs converting: older files have no loops, no order and no preview. An older reader would read the new frame numbers one frame late, play loops as a held last value, ignore the layer order, draw hidden edges and lose the loops, orders, hidden edges, styles and preview when it saved, so the version was bumped. |

### The single-file container

Development builds after Fanimate 0.2.1 save a project as one ZIP file instead of a folder (see [Layout](#layout)). This did not bump the version by itself: the files inside are the ones version 3 defines. A reader that only knows folders refuses a single file as "not a Fanimate project" rather than misreading it, and folders written by earlier versions open unchanged. The first change to what the files hold (the pivot and the rectangle, version 4) bumped the version.
