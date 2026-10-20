# Fanimate brand

<img src="logo/fanimate-lockup-light.svg" alt="Fanimate" height="64">

Brand assets and guidelines for Fanimate. The reasoning behind these choices is in [section 1 of the Phase 1 breakdown](../docs/roadmap/phase-1.md#1-branding-assets).

> **Status: v1.** The name "Fanimate" has passed its trademark and domain check. Registering the name and logo as trademarks is part of the [legal review](../docs/legal/review-scope.md#5-brand).

## Contents

| Folder | What is in it |
|---|---|
| [`logo/`](logo/) | App icon, one-colour mark, wordmark and horizontal lockups (SVG) |
| [`app-icon/`](app-icon/) | App icon in PNG at 16 to 1024 px, Windows `.ico`, macOS `.icns`, favicons |
| [`file-icon/`](file-icon/) | Icon for `.fnm` project files (SVG, PNG, `.ico`, `.icns`) |
| [`social/`](social/) | GitHub social preview card (1280 × 640 PNG) |
| [`tokens/`](tokens/) | Design tokens: `tokens.json` (source) and generated `themes.css` |
| [`fonts/`](fonts/) | Inter, used for the UI and the social card, with its licence |
| [`scripts/`](scripts/) | `build_brand.py`, which regenerates everything above |

## The mark

**Bone-F**: an *F* whose middle arm meets the stem at a pivot joint, like a bone in a rig. It says *rig* and *Fanimate* in one shape, and the joint stays readable at 16 px.

The joint always has a **pivot dot**: a see-through hole that shows whatever is behind the mark (the tile, the background, or the onion-skin copies). It is part of every version of the mark; never fill it in or drop it.

| Asset | Use it for |
|---|---|
| `logo/fanimate-icon.svg` | The app icon. Violet gradient tile with the white mark. Use from 48 px up. |
| `logo/fanimate-icon-small.svg` | Sizes of 32 px and below. Thicker strokes, a larger joint and pivot dot, flat violet-600 tile. |
| `app-icon/fanimate-macos.svg` | macOS only. The tile sits on Apple's 824/1024 grid with a shadow. |
| `logo/fanimate-mark-{violet,white,black}.svg` | One-colour mark with no tile: embossing, stickers, single-ink print. |

## The wordmark

Lowercase *fanimate*, custom-drawn as monoline strokes with round ends. It is not set in a font, so it looks the same everywhere and needs no font licence. The dot of the *i* is a **keyframe diamond**, the one decorative detail. Do not add others.

| File | Background |
|---|---|
| `fanimate-wordmark-violet.svg` | White or light lavender |
| `fanimate-wordmark-ink.svg` | Light backgrounds where violet would clash |
| `fanimate-wordmark-white.svg` | Violet or dark backgrounds |
| `fanimate-lockup-light.svg` | Icon plus wordmark on light backgrounds |
| `fanimate-lockup-dark.svg` | Icon plus wordmark on dark backgrounds |

**Clear space:** keep empty space around the logo of at least the height of the *i* diamond.
**Minimum size:** icon 16 px; wordmark 80 px wide; lockup 120 px wide.

## Colour

The hero brand colour is **Fanimate Violet `#8450F5`** (`violet-500`).

| Token | Hex | | Token | Hex |
|---|---|---|---|---|
| violet-50 | `#F5F0FF` | | violet-600 | `#6D35DE` |
| violet-100 | `#EBE0FF` | | violet-700 | `#5826B8` |
| violet-200 | `#D6C2FF` | | violet-800 | `#431D8C` |
| violet-300 | `#BC9BFF` | | violet-900 | `#2F1463` |
| violet-400 | `#9F73FF` | | violet-950 | `#1C0B3D` |
| **violet-500** | **`#8450F5`** | | | |

**Purple marks interaction, not content.** Inside the app, purple is for selection, focus, active tools and primary buttons. Animation state has its own colours, so it is never confused with the brand:

| Role | Night | Day |
|---|---|---|
| Keyframe / playhead | `#FFB84D` | `#9A5B00` |
| Bones | `#5EEAD4` | `#0F766E` |
| Success, onion skin future | `#3DDC97` | `#157347` |
| Error, onion skin past | `#FF6B8E` | `#C2294F` |

Never use colour as the only signal; pair it with an icon or a label.

## Themes

Four themes are defined in [`tokens/tokens.json`](tokens/tokens.json):

| Theme | Use |
|---|---|
| **Fanimate Night** | Default. Purple-tinted dark. |
| **Fanimate Day** | Light. Also the website and docs theme. |
| **Fanimate Studio Grey** | Neutral greys with a purple accent, for colour-critical work. |

`tokens/themes.css` exposes them as CSS custom properties. Night is the default; Day applies when the system prefers light; any theme can be forced with `data-theme`:

```html
<link rel="stylesheet" href="brand/tokens/themes.css">
<html data-theme="studio-grey">
```

```css
.button-primary { background: var(--button); color: var(--on-button); }
.panel          { background: var(--panel); border: 1px solid var(--border); }
```

Every theme is checked against WCAG AA contrast: 4.5:1 for text, 3:1 for state colours such as keyframes and bones. The build fails if any pair falls short.

## Typography

| Use | Font | Licence |
|---|---|---|
| UI and marketing text | [Inter](https://github.com/rsms/inter) (variable, in `fonts/`) | SIL Open Font License 1.1, see [`fonts/Inter-OFL.txt`](fonts/Inter-OFL.txt) |
| Frame numbers, scripting console | JetBrains Mono (to be added with the UI work) | SIL Open Font License 1.1 |
| Wordmark | Custom strokes, not a font | Part of this repository |

## Do and don't

**Do**

- Use the files in this folder as they are.
- Use the small icon for 32 px and below.
- Put the white wordmark on violet-600 or darker, or on the Night background.

**Don't**

- Recolour the mark outside the brand palette, or add effects such as outlines or glows.
- Stretch, rotate or redraw the logo, or set "fanimate" in another font as a substitute.
- Place the gradient icon on a busy photo without a solid backing.
- Use the logo in a way that suggests the project endorses another product.

## Rebuilding the assets

Everything except `tokens.json`, the fonts and this README is generated. Change the geometry in `scripts/build_brand.py` or the colours in `tokens/tokens.json`, then run:

```sh
pip install pillow pymupdf
python brand/scripts/build_brand.py
```

The script checks contrast first and stops if a theme fails, then writes the SVGs, PNGs, `.ico`, `.icns`, `themes.css` and the social card. PyMuPDF cannot draw SVG gradients or masks, so for the PNG, `.ico` and `.icns` files the script paints gradient tiles with Pillow, renders only the flat shapes through PyMuPDF, and punches the pivot hole with Pillow. The SVG files themselves keep their gradients and use a mask for the hole.

## Still to do

From the [brand asset checklist](../docs/roadmap/phase-1.md#17-brand-asset-checklist):

- [x] Trademark and domain check for the name
- [ ] Splash screen and about dialog (onion-skin F illustration, as on the social card)
- [ ] Interface icon set v1 (about 120 icons). Interface icons come from [Lucide](https://lucide.dev) (ISC licence), in `crates/fanimate-app/icons/lucide/`; the bone tools, which Lucide lacks, are drawn in Lucide's style in `crates/fanimate-app/icons/fanimate/`. About 35 are in use so far
- [ ] Installer art (Windows banner, macOS DMG background, Flatpak metadata)
- [ ] Add JetBrains Mono to `fonts/`
- [ ] Optional: a mascot rigged in Fanimate

## Licence

These assets are covered by the repository's MIT / Apache 2.0 licences, and Inter by its own OFL licence. The licences cover copyright only. The Fanimate name and logo identify the project, so forks and other products should use their own.
