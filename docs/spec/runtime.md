# The Fanimate runtime format (`.fnmr`), version 1

A `.fnmr` file is an animation in the form a game or a web page can play with a small player: **slots** (shapes) with a path, paint and **baked transform keys**. The animation program has already worked out what every frame looks like (bones, Smart Bones, drivers, easing, text, the camera), so a player needs no solver and no easing code, and it draws exactly what the canvas shows.

**Status: version 1, alpha.** The format may still change before Fanimate 1.0; the `version` field is how a reader tells. Reference readers: `fanimate_export::runtime::Player` (Rust, checked against the evaluator on a rigged walk cycle, frame by frame) and `runtimes/web/fanimate-runtime.mjs` (JavaScript, no dependencies, plays a file in a browser or Node).

## What it carries and what it does not

Carried: vector shapes with fills and strokes, their paths (written once, or keyed when they change: skinned shapes and text move this way), their transform, opacity and paint over time, the background colour, the camera (folded into every slot's matrix), and the frame range and rate.

**Not carried, by design: how the animation was made.** There are no bones, weights, Smart Bones or drivers in version 1, so a game cannot repose a character, retarget an animation or attach a prop to a hand. Slots play back what was baked. Also left out, and counted when exporting: pictures (image layers), masks, clips and blend modes (drawn as plain layers), a brush stroke's width profile (drawn as an even stroke), hidden edges and stroke exposure (strokes are drawn whole), and see-through shapes blend in sRGB. The exporter prints each of these. A later version adds bones, mesh slots with weights and pictures; a file that needs a feature a reader does not know says so in `version`.

## The file

A ZIP archive (the same container as `.fnm`) holding `runtime.json`. A reader accepts the bare JSON as well. UTF-8, no comments, numbers finite.

```json
{
  "format": "fanimate-runtime",
  "version": 1,
  "name": "Walk cycle",
  "fps": 24,
  "width": 1920,
  "height": 1080,
  "first_frame": 1,
  "last_frame": 96,
  "background": "#ffffff",
  "slots": [ ... ]
}
```

`first_frame` to `last_frame` are the project's own frame numbers (the first frame is 1; the file keeps the numbers so a game can match them to the project). `fps` is frames per second. `width` and `height` are the picture size in the unit all paths and matrices use. `background` is the canvas colour, or `null`; it is also written as the first slot, so a player may ignore it. `slots` are listed **back to front**, which is the draw order for every frame.

## A slot

```json
{
  "name": "head front",
  "d": "M10 10L30 10L30 30L10 30Z",
  "present": [[1, 40], [60, 96]],
  "paint": {"fill": {"color": "#e63946", "alpha": 1}, "stroke": {"color": "#000000", "alpha": 1, "width": 2}},
  "paint_keys": [{"frame": 1, "fill": {...}, "stroke": null}],
  "path_keys": [{"frame": 1, "d": "M..."}],
  "keys": [{"frame": 1, "m": [1, 0, 0, 1, 0, 0], "opacity": 1}]
}
```

- `d` is the path in the slot's own space, as SVG path data (`M`, `L`, `C`, `Z`, absolute, with an optional space or none between a letter and a number). Where the path changes over time, `d` is the first path and `path_keys` lists every change.
- `present` lists the inclusive frame ranges on which the slot is drawn. On any other frame it is not drawn at all (not drawn is not the same as transparent: keys do not interpolate across a gap).
- `paint` is the fill (`color` as `#rrggbb`, `alpha` 0 to 1) and stroke (`color`, `alpha`, `width` in the slot's own units, so the matrix scales it) on the slot's first frame; either may be `null`. `paint_keys` is present only when paint changes.
- `path_keys` and `paint_keys` are **stepped**: the value of the last key at or before the frame (the whole part of the frame) holds. A key has `frame` and the new value.
- `keys` are the **motion**: `m` is the matrix `[a, b, c, d, tx, ty]` taking the slot's space to the picture (the SVG `matrix(a b c d e f)`: `x' = a x + c y + tx`, `y' = b x + d y + ty`, y down), with `opacity` 0 to 1 for the slot and all its parents. Between two keys a player **interpolates each of the seven numbers linearly**; before the first key and after the last it holds. A key is written wherever the value differs from the frame before or after, so a run of unchanged frames costs two keys and a value that never changes costs one. A player that runs faster than the frame rate therefore moves smoothly, and at the frame rate it shows the baked frames exactly.

Fractional frames: `present`, `path_keys` and `paint_keys` use the whole part of the frame; `keys` use the fraction.

A frame is drawn by: for each slot back to front, if the whole frame is in a `present` range, take the path and paint in force, the interpolated matrix and opacity, and draw the path with that matrix, fill and stroke at that opacity (multiplied into each paint's alpha). The web player's `toSvg(frame)` is exactly that, written as SVG.

## Limits

At most 20,000 frames and 3,000,000 keys in a file. A reader should refuse a file past its own limits rather than allocate without bound, and must not trust any number: it may be missing, not a number or huge. The reference readers draw nothing for a slot they cannot read and never fail on one.

## Versions

Version 1 is what this document describes. A reader that does not know a file's `version` must refuse it with a message. Fields it does not know in a known version are ignored.
