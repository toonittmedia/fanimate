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
- The Style panel sets colours, line width and taper for new drawings.

## 4. Animating

- Every value on the timeline can have keys. Editing an animated value sets a key on the current frame; editing an unanimated one changes it for the whole animation.
- Drag keys to retime them; right-click a key for its interpolation. A key's mark says how it moves on: a round amber key is smooth, a blue diamond linear, a red square step, a green circle with a curve in it is one of the plain eases (ease in, ease out, ease in and out; each has its own curve), and a purple circle is a livelier one (overshoot, bounce, elastic) or a custom curve. The interpolation menu next to the transport controls shows the same marks, larger.
- Points of a drawing can be keyed too, to change its shape over time.

### The graph editor

**Window › Graph** shows the keys of the selected layer as curves, one property at a time (pick it with the buttons on top): value against frame, the x and y of a position as two curves. Drag a key to move it in time and in value; it stops before the keys on either side. Right-click a key for the interpolation choices; **Custom curve** puts two handles on the key that you drag to shape the motion toward the next key. Each drag is one undo step. Angles are shown in degrees and opacity in percent.

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

An animator acts on the text one **character, word or line** at a time. The easy way in is **Layer › Text animator** (or **Add animator…** in Properties): *fade in by character*, *typewriter*, *rise in by word*, *pop in by character* and a looping *wave*, starting at the playhead and running for a second. Each is an ordinary animator you can change.

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

Press **T** (or choose the **T** in the Draw tools) and click the canvas to type: the words appear where you clicked, set in the font and colour of any new text (the line colour). Click existing text to edit it in place. **Escape** or a click elsewhere finishes; the Properties panel still sets the font, size and the rest. The tool makes the layer when you type the first letter, so a click that types nothing leaves nothing behind.

Not built yet: text following a shape live (a copy of the path is used), templates, and animator rows in the timeline.

## 7. Exporting

- **File › Export Video** (Ctrl+Shift+R), at any size and frame range. With nothing else installed: MP4 (H.264 with sound, made by Windows' or macOS's own encoder), MP4 (AV1), uncompressed AVI, animated GIF, animated PNG, and PNG, TIFF or OpenEXR sequences. With FFmpeg installed: also H.265, WebM, MKV, ProRes 4444 (with transparency), ProRes 422 HQ, DNxHR HQ and animated WebP. **Edit › Preferences › Video formats** shows what this computer can make; see [More video formats with FFmpeg](../install.md#more-video-formats-with-ffmpeg).
- Renders run in the render queue, a window of its own that keeps working after Fanimate closes.
- **File › Export for Web and Games** makes things that are not video, in the background, and tells you in the status bar what each one could not carry:
  - **Export Frame as SVG…** writes the frame at the playhead as a vector picture of what the canvas shows (the camera, bones, text and key colours included). Pictures are left out, a brush stroke's width changes are drawn as an even stroke, and a see-through shape blends in a different colour space from the canvas, so it can look slightly different.
  - **Export as Lottie…** bakes the whole animation into a dotLottie (`.lottie`) or Lottie JSON file that web and phone players read. Every frame is baked, so it looks like the canvas frame by frame, but bones, easing curves and Smart Bones are gone: only their result is kept. Frames that do not change cost nothing, so a still pose is small. Pictures are left out.
  - **Export Sprite Sheet…** packs every frame into one PNG with the empty margins trimmed, a held pose stored once, and a JSON atlas beside it (TexturePacker's "JSON hash" layout, which Phaser, PixiJS, Godot and most engines read, plus a `clips` list). From a script or the command line you can name clips: `fanimate-cli sheet scene.fnm -o walk.png --clips walk=1-24,idle=25-48`.

## 8. Keyboard shortcuts

**Help › Keyboard Shortcuts** lists every shortcut of the set in use. Edit › Keyboard Shortcuts switches between Fanimate's standard set and Moho's.

## 9. Fanimate in the browser

The [web build](../web.md) opens, plays and checks projects without installing anything.
