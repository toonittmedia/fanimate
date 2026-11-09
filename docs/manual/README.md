# Fanimate user manual

This is the outline of the manual, with the essentials of each chapter. Chapters grow as testers ask questions; open an issue for anything that is missing or unclear.

## 1. Getting started

- Install: see [Installing and updating](../install.md).
- First start: choose a keyboard shortcut set (Fanimate's standard shortcuts, or Moho's) and a theme. Both can be changed in Edit › Preferences.
- The screen follows Moho Pro's layout: tools on the left, the canvas in the middle, the timeline along the bottom, Properties, Style and Actions on the right above Layers. Every panel can be moved, tabbed, closed and reopened from the Window menu. **Window › Workspace** arranges the panels for animating, drawing, rigging, frame-by-frame work or review.
- **F1** or **Ctrl+Shift+P** opens the command palette: type part of any command's name.
- Right-click anything (layers, keys, the canvas) for what you can do with it.

## 2. Projects

- **File › New**, **Open**, **Save** and **Save As**. A project is one `.fnm` file (a ZIP archive of readable files; [format](../spec/fnm.md)). Older projects saved as folders still open and stay folders.
- Fanimate keeps a copy of your work after every change. If it closes unexpectedly, the next start offers to recover it.
- **File › Open Moho or Animate Project…** brings in `.moho` and `.fla` files: see [Importing](../import.md).
- **File › Import** (Ctrl+I) adds pictures (PNG, JPEG), Photoshop files and OpenRaster (`.ora`, from Krita, MyPaint and GIMP) files (as layers, groups kept; an OpenRaster layer's blend mode other than normal is shown as normal, and the status bar says how many), SVG drawings and sound (WAV, MP3, Ogg, FLAC, AAC and M4A, turned into WAV when imported) to the open project. You can also drop these files on the window.

## 3. Drawing

- Freehand brush with pressure and a stabiliser, rectangle, ellipse, pen and Add Point tools, eraser, paint bucket and eyedropper.
- Select Points and Transform Points edit drawings point by point. Combine Shapes joins, subtracts and intersects.
- The Style panel sets colours, line width and taper for new drawings, and lists the colours your artwork uses (Palette): change a swatch and every use of that colour changes at once.
- **Painting with pixels:** Layer > New Raster Layer makes a layer you paint on with the Brush and the Eraser (Style sets the size and the edge's hardness). The **Paint Bucket** on a raster layer floods the area you click, up to the lines around it and the edge of the page, with the fill colour (the line colour when fill is off). It also covers the soft edge of a line behind the line, so no pale halo is left. Small breaks in the outline (up to about 6 pixels) do not let the fill out; a wider one does, so close it, or undo (one click is one undo step). The **Eyedropper** on a raster layer takes the colour of the painted pixel you click as the line colour (and the fill colour when fill is on). Styles can be shared: in the Style panel, the "…" menu next to the style name has **Export style…** (a small `.fnstyle` file) and **Import style…** (adds it to your list and chooses it; a name you already have gets a number, so nothing is replaced). The Style panel's **Flow** (how strongly each dab lays paint: low builds colour up as you go over it) **Smudge** (above 0 the Brush drags the paint under it along the stroke instead of laying the line colour), **Pressure to flow** (with a pen, a light touch lays lighter paint instead of only a smaller dab) and **Spacing** (the distance between dabs, as a share of the brush size) shape the raster brush too. It also has **Bucket tolerance** (how different a colour may be and still be filled; higher fills across soft edges) and **Bucket gap** (0 to 8 pixels: the breaks it holds back are about twice that wide; 0 lets the paint out of any break).

### Masks, clips and blend modes

Layer › **Compositing** (or the Compositing section of Properties, and the right-click menu of a layer) says how a layer is mixed with the others:

- **Mask.** Set a layer's mask role to **Add to mask** or **Subtract from mask** and it stops being drawn. The mask layers of one group (or of the top level) together decide where that group's other layers show: what an adding layer covers is let through, what a subtracting layer covers is taken out of it. A layer marked **not held back by the mask** shows regardless. Hiding every mask layer turns the mask off, so you can look past it. The Layers panel says "Mask" beside such a layer.
- **Clip to the layer below.** The layer shows only where the layer under it is drawn, and takes its own colours there, as a clipping mask in a paint program does. Several layers clipped in a row to one base and the base are one picture; hiding the base hides them.
- **Blend mode.** Multiply darkens, Screen lightens and Add adds light. A layer with a blend mode is drawn as a picture of its own and laid over the rest, its opacity applying to the picture.
- **Flatten** (groups). Without it a group's opacity fades each shape separately, so overlapping shapes show through each other; with it the group is drawn as one picture and then faded.

Masks, clips and blend modes are drawn on the canvas, in renders and in the browser. SVG and Lottie exports draw such layers plain (the mask layers are left out) and say so. The Moho, Animate and PSD importers still count them as left out.

## 4. Animating

- Every value on the timeline can have keys. Editing an animated value sets a key on the current frame; editing an unanimated one changes it for the whole animation.
- Drag keys to retime them; right-click a key for its interpolation. A key's mark says how it moves on: a round amber key is smooth, a blue diamond linear, a red square step, and each named ease has a shape of its own. The three plain eases are green and follow one idea: a round side is gentle and a flat side is abrupt, so ease in is round at the start and flat at the end, ease out is the reverse, and ease in and out is round at both ends. The livelier three are purple: a triangle for overshoot (it shoots past), a dome for bounce and a plus for elastic. A custom curve is a purple circle with its curve drawn in it. The interpolation menu next to the transport controls and a key's right-click menu show the same marks, larger, with a small graph of each curve beside the name. The dashed line in the graph is the value the next key holds: overshoot crosses it once, bounce meets it from below again and again, elastic swings across it.
- Points of a drawing can be keyed too, to change its shape over time.

### The graph editor

**Window › Graph** shows the keys of the selected layer as curves, one property at a time (pick it with the buttons on top): value against frame, the x and y of a position as two curves. Drag a key to move it in time and in value; it stops before the keys on either side. Right-click a key for the interpolation choices; **Custom curve** puts two handles on the key that you drag to shape the motion toward the next key. Each drag is one undo step. Angles are shown in degrees and opacity in percent.

In the graph, scroll the wheel to move up and down, Shift+scroll to move along the frames, Ctrl+scroll to zoom the frames about the pointer and Ctrl+Shift+scroll to zoom the values. The buttons at the top right (and the zoom shortcuts while the pointer is over the graph) zoom in and out and fit the curves, overshoots and bounces included. Press on empty space and drag to scrub the playhead.

**Bake expression** (the button under the property buttons) writes a formula as keys. Type a formula such as `value + 10 * sin(frame / 3)`, choose the frames (and X or Y for a position) and press Bake or Enter: the formula is worked out on every frame of the range and the answers replace the keys there, in one undo step. `value` is what the curve had on that frame, in the units the graph shows. Other names are `frame`, `time`, `progress`, `first` and `last`; functions include `sin`, `clamp`, `lerp`, `smoothstep`, `if`, `mod`, and the seeded `noise`, `random` and `blink` (the same on every frame order and every computer). If any frame fails, nothing is written and the status bar says which.

## 5. Rigging

- Bone layers hold a skeleton. **Add Bone** draws chains; **Select Bone** edits the rest pose; **Manipulate Bones** poses with inverse kinematics (Alt for one bone) and always keys the frame.
- Bind artwork to a bone (Ctrl+B), smoothly to several (Ctrl+Shift+B), or paint weights (W). Pictures bend with bones on a mesh.
- **Actions** and **Smart Bones** store poses that bones drive: a turning head, a correcting elbow.
- **Controllers** (Actions panel, New Slider or New Pad) are one control for a whole movement: right-click one and choose Play Action for each action it should play. Dragging a slider from 0 to 1 plays the action from its first frame to its last, and a pad has two values (across and down). Like any value, a controller is keyed on frames after the rest pose.
- The rig health check in Properties lists bones that move nothing and Smart Bones that need attention.

To use an action on a different rig, right-click it in the Actions panel and choose **Copy to Rig**. Bones are paired by name ("arm L", "Arm_L" and "arm.l" are the same bone) and each one's turn away from its own rest angle is kept, so a walk made on a tall character plays on a short one. Bones the other rig has no match for are left out, and the status bar says how many.

## 6. Frame-by-frame and lip sync

- Frame-by-frame layers hold one drawing per exposure, with rough, colour, line and overlay parts. **New Drawing** (F7) copies the drawing on show.
- Onion skin (Alt+O) and the light table (Alt+L) help drawing between frames. The X-sheet panel shows exposures down the page.
- Switch layers show one child at a time: mouths, hands, eyes. With a switch layer selected, **Lip Sync** (in the Layer menu or the layer's right-click menu) listens to a WAV dialogue track and keys its mouths.

## 6a. Text

*New in the development version (format 7); not in a release yet.*

- **Layer › New Text Layer** puts a text layer on the canvas. Type the words in the **Properties** panel, which also sets the font (any font installed on the computer, or one of the bundled ones), size, weight, letter spacing, line height, alignment, direction, wrapping at a width, colour and outline. Text is laid out with the font's own ligatures and kerning, and wraps at the places the Unicode rules allow.
- Text is drawn as shapes, so it moves, scales and fades like any layer, and it exports like any other layer. **Layer › Convert Text to Shapes** turns it into drawing you can edit point by point; it stops being text and loses its animators.
- **Text on a path.** Select the text layer and, with it, a layer that has a shape (hold Ctrl and click), then choose **Layer › Set Text Path from Shape** (or the button under **On a path** in Properties). The letters stand on the shape and turn with it. A closed shape carries the text round; an open one cuts off what does not fit. **Read from the end** runs it the other way, and **Start along path** (a percentage of the length, keyed like other values) slides the text along. The shape is copied, so later changes to it do not move the text, and **Remove Text Path** sets the text straight again.
- **Fonts.** The font menu lists the fonts Fanimate ships (Inter, JetBrains Mono and the Noto fonts below) and, under a search box, every font installed on the computer. A text keeps the name of the font it was set in. Bold works where the font has a bold file (or, like Inter, a weight axis): text of weight 600 or more uses it. A project that uses a font this computer does not have is set in Inter instead, and Properties says so; the name is kept, so the project looks right again on a computer that has the font. The bundled fonts look the same everywhere, so use one of them (or convert the text to shapes) when a project must look identical on every computer. Arabic, Hebrew, Devanagari and Thai are bundled (Noto Sans): type or paste them into any text and the right font is used for those letters, with the joined forms, conjuncts and mark placement the language needs; one text can mix scripts. Their fonts have one weight, so the weight control only changes Inter. Chinese, Japanese and Korean are not bundled yet.

### Text animators

An animator acts on the text one **character, word or line** at a time. The easy way in is **Layer › Text animator** (or **+ Add** beside Animators in the Text panel): *fade in by character*, *typewriter*, *rise in by word*, *pop in by character* and a looping *wave*, starting at the playhead and running for a second. Each is an ordinary animator you can change.

On the timeline an animator is a white **range stick** under its text layer, from its first key to its last, so its keys are not confused with the diamonds of the layer's own move, turn, scale and fade. Sticks that overlap in time sit in separate lanes, and a lane is used again once the animator before it has ended. Click a stick to pick the animator (double-click to open it in the Text panel), drag it to move all its keys, drag either end to stretch or squeeze them, and right-click for Duplicate, Delete and **Key type** (how every key of the animator moves on).

An animator has:

- a **range** (start, end and offset, in percent of the text) saying which units it reaches, and a **selection** shape saying how much: a hard edge, a ramp, a bell. Moving the range along the text is what makes an effect sweep across it. The **soft edge** lets the square shape fade units in and out at the ends of the range instead of switching them;
- what the reached units **do**: move, turn, scale, fade, change colour. Turning and scaling happen about the middle of the character, word or line;
- an optional **random order**, with a seed, so units are reached in a shuffled order that is the same every time.

Every value can have keys: the diamond beside it adds or removes a key on the current frame, and editing a value on a later frame sets a key (the rest pose, frame 0, changes the value itself), as everywhere else. Interpolation includes the new *ease in*, *ease out*, *overshoot*, *bounce* and *elastic* choices (right-click a key, or the interpolation menus).

### Captions

**File › Import Captions…** (or dropping an `.srt` or `.vtt` file on the window) reads a subtitle file into a new **switch layer** with one text layer for each caption. Each caption is white with a dark outline, centred low on the canvas, and shows from its start to its end; the animation is made longer if the last caption runs past its end. The captions are ordinary text layers: open the layer to change a caption’s words, style or position, or give one a text animator. A caption that overlaps the next one ends where the next begins. Lines of the file that cannot be read (a bad time, no words) are left out, and the status bar says how many.

**Layer › Text › Restyle All Captions** (with one caption selected) makes every other caption in its set look like it: the font, size, colour, outline, alignment, animators and where it stands, but not the words. Style one caption, then run it once. It is one undo step.

**File › Export Captions…** writes the selected caption layer back as SubRip (`.srt`) or WebVTT (`.vtt`), by the file name you choose. Formatting tags and screen positions in the file are not kept: a caption’s look is its layer’s.

### Secondary motion (springs)

A tail, an ear or a scarf should lag behind the body and swing back, not turn rigidly with it. Key the body the way you want it, select the first bone of the tail in the **Bones** panel, and under **Secondary motion** set the spring: **Stiffness** (how hard the bone is pulled back to its key: about 100 is a loose tail, 1000 a stiff one), **Damping** (how fast the swing dies; about twice the square root of the stiffness stops it overshooting) and **Inertia** (how much the turning of the bones above throws it: 0 none, 1 like a free weight, more for a whip). Then **Animation › Bake Spring Motion**. The bone and every bone below it get a key on every frame over the whole animation, replacing the keys they had there, so the result is ordinary animation you can edit, retime or delete; undo brings the old keys back. Bake again only after undoing, or the spring will spring the spring.

### Text in SVG files

**File › Import** of an SVG file now brings its text in as **text layers**, above the drawing: the words, lines made with `tspan`, the font (when it is one Fanimate has; otherwise Inter), size, weight, colour, left, centred or right alignment and where the SVG put it, even when turned. Text set on a path is left out and the status bar says so. The drawing order between text and shapes is not kept: text always comes in on top.

### The Text tool

Press **T** (or choose the **T** in the Draw tools) and click the canvas to type: the words appear where you clicked, set in the font and colour of any new text (the line colour). Click existing text to edit it in place. **Escape** or a click elsewhere finishes; the Text panel (a tab beside Bones, brought to the front by the Text tool) sets the font, size and the rest. The tool makes the layer when you type the first letter, so a click that types nothing leaves nothing behind.

Not built yet: text following a shape live (a copy of the path is used), templates, and animator rows in the timeline.

## 6b. Helpers that need no AI

**Edit › Assist** (also in the right-click menu of a layer, and in the command palette) holds three helpers. They use no model and no network, so they work with every AI switch off. Each is one undo step, so Undo is how you reject a result, and each marks the layer it changed or made (Layer properties show it) so the file says what was assisted.

- **Clean Up Lines** (Ctrl+Alt+L) refits the lines of the selected shapes, or of the whole layer, with the fewest smooth curves that stay within a tolerance. The tolerance is in **Properties › Outline › Tolerance** (smaller keeps more of the wobble). Corners you drew stay corners, and a brush stroke keeps its thick and thin. Shapes that have point keys, hidden edges or smooth binding are left as they are, and the status bar says how many.
- **Suggest Inbetween** (Ctrl+Alt+I) works on a frame-by-frame layer. With the playhead on a drawing it adds a drawing halfway to the next one; with the playhead on a later frame it adds one there. Shapes are paired by position and size, and points move along arcs, so a swinging arm keeps its length. A shape with no partner is held as it was and counted. Edit the new drawing like any other.
- **Auto-Rig Character** (Ctrl+Alt+R) builds bones from layer names: torso, body or chest; head, face or hair; arm, forearm and hand; leg, thigh, shin and foot; tail. A side is named with L, R, A, B, left or right. The layers go into a new bone layer and each is bound to the bone of its part without moving the artwork. A character needs a torso layer; a layer that names no body part stays where it was. Everything is an ordinary rig afterwards: move the bones, add more.

**Edit › Combine Shapes › Offset Path** (Ctrl+Alt+O) adds an outline a set distance outside a shape, or inside when the distance is negative. Set the distance in **Properties › Outline**.

## 6bb. AI models

The helpers above need no model. Some later tools (turning a scanned drawing into lines, speech to text for captions) are meant to use small models that run on your computer, off until you turn AI features on. **Fanimate ships none yet.** **Edit > Preferences > AI models** lists the models you have installed, installs one from a `.fnmodel` file and removes one. A model is accepted only if its licence is a permissive one, its training data is documented (commissioned, donated, public domain or openly licensed, never art scraped from the web), it carries Fanimate's signature and its weights are exactly what was signed; anything else is refused with the reason. See [the policy and the list of models](../legal/models.md).

## 6c. Poses you keep, canvas rotation and plugins

- **My poses.** Right-click an action in the Actions panel and choose **Save to My Poses**. The pose appears under **My poses** at the bottom of the panel in every project: click it to add it to the selected rig. Bones are matched by name (`arm L`, `Arm_L` and `arm.l` are one bone) and each bone's turn from its own rest angle is carried over, so a wave made on one character works on another. The library is the file `poses.json` in Fanimate's data folder.
- **Rotate Canvas.** View › Rotate Canvas, or Ctrl+Alt+Left and Ctrl+Alt+Right, turns the view 15 degrees; Shift + middle-drag turns it freely. Only how you look at the animation turns: the artwork and everything saved are not changed, and drawing and selecting work as usual. Ctrl+Shift+0, or a click on the angle in the status bar, turns it upright. While it is turned the rulers are off and guides cannot be moved.
- **Plugins.** Window › Plugins lists the programs in the plugins folder. Choosing one asks you first, in words, what it asks to be allowed to do. A plugin is a program: Fanimate limits what it does to your project, not to your computer, so run only plugins you trust. Everything a plugin does is one undo step, and Window › Plugins › Stop Plugin ends it. See [Plugins](../plugins.md) for making one.

## 7. Exporting

- **File › Export Video** (Ctrl+Shift+R), at any size and frame range. With nothing else installed: MP4 (H.264 with sound, made by Windows' or macOS's own encoder), MP4 (AV1), uncompressed AVI, animated GIF, animated PNG, and PNG, TIFF or OpenEXR sequences. With FFmpeg installed: also H.265, WebM, MKV, ProRes 4444 (with transparency), ProRes 422 HQ, DNxHR HQ and animated WebP. **Edit › Preferences › Video formats** shows what this computer can make; see [More video formats with FFmpeg](../install.md#more-video-formats-with-ffmpeg).
- Renders run in the render queue, a window of its own that keeps working after Fanimate closes.
- **File › Export for Web and Games** makes things that are not video, in the background, and tells you in the status bar what each one could not carry:
  - **Export Frame as SVG…** writes the frame at the playhead as a vector picture of what the canvas shows (the camera, bones, text and key colours included). Pictures are left out, a brush stroke's width changes are drawn as an even stroke, and a see-through shape blends in a different colour space from the canvas, so it can look slightly different.
  - **Export as Lottie…** bakes the whole animation into a dotLottie (`.lottie`) or Lottie JSON file that web and phone players read. Every frame is baked, so it looks like the canvas frame by frame, but bones, easing curves and Smart Bones are gone: only their result is kept. Frames that do not change cost nothing, so a still pose is small. Pictures are left out.
  - **Export as Animated SVG…** bakes the animation into one SVG that browsers play without a script or a player. A frame that does not change costs nothing. Inkscape and Illustrator show only the first frame.
  - **Export Runtime File…** writes a `.fnmr` for games and web pages: each shape with its path and a few baked transform keys, so a character plays exactly as it looks here with a very small player ([the format](../spec/runtime.md)). Bones are not in it, so a game cannot repose the character. From the command line, `--tolerance 2` leaves out keys the player would not miss, and `--estimate` says how big the file would be without writing it.
  - **Export Paint Layers (OpenRaster)…** (in the File menu) writes the raster layers and picture layers as they are on the frame at the playhead, as an `.ora` file that Krita, MyPaint, GIMP and others open with the layers, their places, opacity and visibility kept. Raster layers are trimmed to the paint on them and nothing is re-drawn, so the pixels are the ones you painted. Vector shapes, text, bones, frame-by-frame drawings, layers that are turned or scaled, and blend and mask settings are not written, and the status bar counts them. From a terminal: `fanimate-cli export scene.fnm -o scene.ora --frame 12`.
  - **Export Timeline (OpenTimelineIO)…** writes the timing and sounds for an editing program: a video clip you link to your rendered movie, and each sound placed and trimmed on its own track, with the sound files written beside it. Drawings do not travel.
  - **Export Sprite Sheet…** packs every frame into one PNG with the empty margins trimmed, a held pose stored once, and a JSON atlas beside it (TexturePacker's "JSON hash" layout, which Phaser, PixiJS, Godot and most engines read, plus a `clips` list). From a script or the command line you can name clips: `fanimate-cli sheet scene.fnm -o walk.png --clips walk=1-24,idle=25-48`.

## 8. Keyboard shortcuts

**Help › Keyboard Shortcuts** lists every shortcut of the set in use. Edit › Keyboard Shortcuts switches between Fanimate's standard set and Moho's.

## 9. Fanimate in the browser

The [web build](../web.md) opens, plays and checks projects without installing anything.
