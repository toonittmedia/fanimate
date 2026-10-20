# Video import: design note

Status: **design only, no code.** Asked for during the 0.2.3 UX pass; audio import (MP3, OGG, FLAC, AAC and M4A, converted to WAV) shipped there, video did not. Phase 1 does not include video layers, so this records the open questions for when it is scheduled.

## What users want

Drop a video onto the project as a reference to animate over (rotoscoping, timing a scene to live action) and, less often, as an element in the final render.

## Proposed shape

- **A new layer kind, `video`**, like `image` and `audio`: it names a media file, an offset (the project frame where the clip starts) and a transform and opacity like any layer. It plays at the project's frame rate; a clip at another rate shows its nearest frame.
- **A proxy, not the original.** Scrubbing needs random access to any frame, which long-GOP H.264 cannot give quickly. On import, the user's own FFmpeg (found by `fanimate_export::ffmpeg::find`, never shipped, see `docs/legal/video-export.md`) converts the clip to a proxy: a picture sequence or an intra-frame codec at a reduced size (longest side 960 px). The proxy lives in `Document::media`; the original is only referenced by path so a project does not balloon, with an option to embed it.
- **Without FFmpeg**, the layer cannot be made. The import says what is missing and how to get it, as video export already does for formats the computer cannot make.
- **Sound** from the clip is split off to an audio layer through the same path as audio import.
- **Export** draws the layer from the proxy by default (a preview-quality render) or from the original through FFmpeg for the final render. That choice, and whether the final render may ever use the proxy, needs deciding.

## Open questions

1. **File size.** A proxy at 960 px of a 60 s clip is tens of megabytes as a picture sequence. Is an intra-frame codec in the `.fnm` acceptable, and which one can the app decode without a GPL codec? Fanimate links no FFmpeg, so decoding the proxy must use a permissively licensed decoder (or the system's, as `fanimate-oscodec` does for encoding).
2. **Format version.** A new layer kind needs a version bump (6), a validator entry, a migration (none needed: older files have no video layers) and a decision on what an older reader does with the layer (it must refuse the file rather than drop the layer silently).
3. **Frame accuracy.** Variable frame rate clips drift against a fixed project rate. The proxy step should resample to a constant rate, at the cost of one more conversion.
4. **Time budget.** Gate 1 allows no UI action over 100 ms. The proxy has to be built off the UI thread with progress shown (status bar and, on Windows, the taskbar), like the render queue.
5. **Licensing of codecs in the user's files.** Some containers carry patent-encumbered codecs. Because FFmpeg is the user's own, decoding stays their responsibility, but the web build has no FFmpeg at all: video layers would not play there, and that should be stated.
6. **Cut-out and masking.** Is chroma key or a rotoscope matte in scope? Assumed not.

## Not decided

- Whether the first version is reference-only (never in the render).
- Whether a clip can be trimmed in the timeline or only offset.
