# Fanimate in the browser

The web build (`crates/fanimate-web`) runs the same engine, renderer and importers as the desktop app, compiled to WebAssembly and drawing through WebGPU. It is for **watching and checking work**: share a project with a director or a client without them installing anything.

## What it does

- Opens a Fanimate project (a single `.fnm` file, or a project folder packed as a `.zip`), a Moho project (`.moho`, `.mohoproj`) or an Animate `.fla`: pick it with **Open…** or drop it on the page.
- Plays the animation at the project's frame rate (Space plays and pauses) and scrubs with the frame slider.
- Shows and hides layers.
- Pans (drag) and zooms (scroll); double-click fits the canvas again.
- Lists what an import left out, as the desktop app does.

## Limits

- **No editing, saving or export.** Drawing, rigging, saving and video export are in the desktop app.
- **WebGPU only.** Current Chrome and Edge have it, as do Safari 26 and later and Firefox on Windows; support elsewhere is arriving. Without WebGPU the page says so and points to the desktop app. WebGL is not used, because the renderer draws through an sRGB view of its canvas texture, which WebGL cannot do.
- **No sound.** Audio layers are not played.
- **Files stay on the computer.** The page reads the file you pick in the browser; nothing is uploaded anywhere.
- **Colours** can look slightly different from the desktop app, because browsers adjust page colours to the display's colour profile.
- **Project sync with the desktop app** (the roadmap's longer-term goal) is not built: move projects as `.fnm` files.

## Building and hosting

```sh
scripts/build-web.sh dist/web     # downloads the matching wasm-bindgen if needed
python -m http.server --directory dist/web 8000
```

Then open <http://localhost:8000>. Any static web host works: copy the folder's four files (`index.html`, `fanimate_web.js`, `fanimate_web_bg.wasm`, `favicon.svg`). Every release has the folder as `fanimate-<version>-web.zip`.
