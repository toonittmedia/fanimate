"""Build Fanimate brand assets.

Generates the logo SVGs from shared geometry, rasterises app and file icons
(PNG, ICO, ICNS), renders the social card, writes themes.css from
tokens.json and checks colour contrast.

Usage:  python brand/scripts/build_brand.py           rebuild everything
        python brand/scripts/build_brand.py --check   verify contrast and themes.css only
Needs:  Pillow, PyMuPDF  (pip install pillow pymupdf)
"""

import io
import json
import sys
from pathlib import Path

import fitz  # PyMuPDF, used only to rasterise SVG
from PIL import Image, ImageChops, ImageDraw, ImageFont

BRAND = Path(__file__).resolve().parent.parent
TOKENS = json.loads((BRAND / "tokens" / "tokens.json").read_text(encoding="utf-8"))
V = TOKENS["violet"]
WHITE = "#FFFFFF"
INK = TOKENS["themes"]["day"]["text"]

# ---------------------------------------------------------------- geometry --

# Wordmark: lowercase monoline letters built from lines and arcs with round
# caps. Units: stroke 16, x-height band y=40..100 (centreline 48..92).
WM_STROKE = 16
WM_WIDTH, WM_HEIGHT = 532, 104


def wordmark_paths():
    """Return (stroke_path_d, i_dot_polygon_points) for 'fanimate'."""
    d = []
    # f: stem with a hook to the right, crossbar
    x = 20
    d.append(f"M{x} 92V34A22 22 0 0 1 {x + 22} 12H{x + 30}")
    d.append(f"M{x - 12} 48H{x + 24}")
    # a: single-storey, bowl plus right stem
    cx = 92
    d.append(f"M{cx + 22} 70A22 22 0 1 0 {cx - 22} 70A22 22 0 1 0 {cx + 22} 70")
    d.append(f"M{cx + 22} 48V92")
    # n
    x = 142
    d.append(f"M{x} 48V92M{x} 70A22 22 0 0 1 {x + 44} 70V92")
    # i (dot drawn separately as a keyframe diamond)
    x_i = 214
    d.append(f"M{x_i} 48V92")
    # m
    x = 242
    d.append(f"M{x} 48V92M{x} 70A20 22 0 0 1 {x + 40} 70V92"
             f"M{x + 40} 70A20 22 0 0 1 {x + 80} 70V92")
    # a
    cx = 372
    d.append(f"M{cx + 22} 70A22 22 0 1 0 {cx - 22} 70A22 22 0 1 0 {cx + 22} 70")
    d.append(f"M{cx + 22} 48V92")
    # t: stem with a foot curving right, crossbar
    x = 430
    d.append(f"M{x} 22V80A12 12 0 0 0 {x + 12} 92H{x + 18}")
    d.append(f"M{x - 12} 48H{x + 22}")
    # e
    cx = 500
    d.append(f"M{cx - 22} 70H{cx + 22}A22 22 0 1 0 {cx + 15.56} 85.56")
    # i dot: keyframe diamond
    r, cy = 10, 18
    dot = f"{x_i},{cy - r} {x_i + r},{cy} {x_i},{cy + r} {x_i - r},{cy}"
    return "".join(d), dot


def wordmark_group(color, transform=""):
    d, dot = wordmark_paths()
    t = f' transform="{transform}"' if transform else ""
    return (f'<g{t}><path d="{d}" fill="none" stroke="{color}" '
            f'stroke-width="{WM_STROKE}" stroke-linecap="round" '
            f'stroke-linejoin="round"/>'
            f'<polygon points="{dot}" fill="{color}" stroke="{color}" '
            f'stroke-width="3" stroke-linejoin="round"/></g>')


def svg(width, height, body, defs=""):
    defs = f"<defs>{defs}</defs>" if defs else ""
    return (f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" '
            f'height="{height}" viewBox="0 0 {width} {height}">{defs}{body}</svg>\n')


TILE_GRADIENT = (f'<linearGradient id="tile" x1="0" y1="0" x2="1" y2="1">'
                 f'<stop offset="0" stop-color="{V["400"]}"/>'
                 f'<stop offset="0.45" stop-color="{V["500"]}"/>'
                 f'<stop offset="1" stop-color="{V["700"]}"/></linearGradient>')


# Pivot joint: centre and the radius of the see-through hole in it
PIVOT = (394, 480)
HOLE_R, HOLE_R_SMALL = 38, 52


def pivot_mask(small=False):
    """SVG mask that cuts the hole out of the joint. Every SVG that uses
    bone_f() must include this in its <defs>."""
    cx, cy = PIVOT
    r = HOLE_R_SMALL if small else HOLE_R
    return (f'<mask id="pivot" maskUnits="userSpaceOnUse" x="0" y="0" '
            f'width="1024" height="1024"><rect width="1024" height="1024" fill="#fff"/>'
            f'<circle cx="{cx}" cy="{cy}" r="{r}" fill="#000"/></mask>')


def bone_f(small=False, color=WHITE, hole=True):
    """The Bone-F mark on a 1024 grid: an F whose middle arm meets the stem
    at a pivot joint, like a bone. The joint has a see-through hole; pass
    hole=False for raster renders, where punch_pivot() cuts it instead."""
    (sx, mid), top, bottom, arm, mid_end = PIVOT, 236, 798, 664, 612
    if small:
        # thicker strokes, bigger joint and hole, shorter arms for 16-32 px
        w, jr, arm, mid_end = 168, 128, arm - 10, mid_end - 10
    else:
        w, jr = 124, 98
    mask = ' mask="url(#pivot)"' if hole else ""
    return (f'<g{mask}><path d="M{sx} {bottom}V{top}H{arm}M{sx} {mid}H{mid_end}" '
            f'fill="none" stroke="{color}" stroke-width="{w}" '
            f'stroke-linecap="round" stroke-linejoin="round"/>'
            f'<circle cx="{sx}" cy="{mid}" r="{jr}" fill="{color}"/></g>')


def app_icon_svg(small=False):
    if small:
        tile = f'<rect width="1024" height="1024" rx="200" fill="{V["600"]}"/>'
        return svg(1024, 1024, tile + bone_f(small=True), pivot_mask(small=True))
    tile = '<rect width="1024" height="1024" rx="225" fill="url(#tile)"/>'
    return svg(1024, 1024, tile + bone_f(), TILE_GRADIENT + pivot_mask())


def app_icon_macos_svg():
    """macOS grid: 824 px tile centred on a 1024 canvas, with a soft shadow."""
    shadow = ('<filter id="s" x="-20%" y="-20%" width="140%" height="140%">'
              '<feDropShadow dx="0" dy="10" stdDeviation="14" '
              'flood-color="#000" flood-opacity="0.3"/></filter>')
    body = (f'<g transform="translate(100 100) scale({824 / 1024})">'
            f'<rect width="1024" height="1024" rx="225" fill="url(#tile)" filter="url(#s)"/>'
            f'{bone_f()}</g>')
    return svg(1024, 1024, body, TILE_GRADIENT + shadow + pivot_mask())


def mono_mark_svg(color):
    """One-colour mark (no tile) for embossing, stamps and single-ink print."""
    return svg(1024, 1024, bone_f(color=color), pivot_mask()).replace(
        'viewBox="0 0 1024 1024"', 'viewBox="236 120 560 780"').replace(
        'width="1024" height="1024"', 'width="560" height="780"', 1)


def wordmark_svg(color):
    return svg(WM_WIDTH, WM_HEIGHT, wordmark_group(color))


def lockup_svg(word_color):
    # icon 160 px tall; wordmark x-height aligned to the middle of the icon
    icon_size = 160
    scale = icon_size / 1024
    icon = (f'<g transform="scale({scale})">'
            f'<rect width="1024" height="1024" rx="225" fill="url(#tile)"/>'
            f'{bone_f()}</g>')
    wm_scale = 1.15
    wm_y = icon_size / 2 - 70 * wm_scale  # bowl centre (y=70) on icon centre
    word = wordmark_group(word_color, f"translate(196 {wm_y:.2f}) scale({wm_scale})")
    width = int(196 + WM_WIDTH * wm_scale)
    return svg(width, icon_size, icon + word, TILE_GRADIENT + pivot_mask())


FILE_MARK_POS, FILE_MARK_SCALE = (318, 236), 0.36


def raster_file_icon(size):
    page = file_icon_svg(with_mark=False)
    img = render_supersampled(page, size) if size <= 256 else render(page, size)
    mark = raster_icon(round(size * FILE_MARK_SCALE))
    img.alpha_composite(mark, tuple(round(p * size / 1024) for p in FILE_MARK_POS))
    return img


def file_icon_svg(with_mark=True):
    """.fnm document icon: folded page, small mark, 'fnm' in wordmark letters."""
    page = (f'<path d="M232 64H640L824 248V900A60 60 0 0 1 764 960H232A60 60 0 0 1 172 900V124'
            f'A60 60 0 0 1 232 64Z" fill="{V["50"]}" stroke="{V["200"]}" stroke-width="16"/>'
            f'<path d="M640 64V188A60 60 0 0 0 700 248H824" fill="{V["100"]}" '
            f'stroke="{V["200"]}" stroke-width="16" stroke-linejoin="round"/>')
    mark = (f'<g transform="translate{FILE_MARK_POS} scale({FILE_MARK_SCALE})">'
            f'<rect width="1024" height="1024" rx="225" fill="url(#tile)"/>{bone_f()}</g>'
            ).replace(", ", " ") if with_mark else ""
    # f, n, m only, re-spaced
    letters = (f'<g transform="translate(286 690) scale(2.1)"><path d="'
               f'M20 92V34A22 22 0 0 1 42 12H50M8 48H44'
               f'M70 48V92M70 70A22 22 0 0 1 114 70V92'
               f'M142 48V92M142 70A20 22 0 0 1 182 70V92M182 70A20 22 0 0 1 222 70V92'
               f'" fill="none" stroke="{V["700"]}" stroke-width="{WM_STROKE}" '
               f'stroke-linecap="round" stroke-linejoin="round"/></g>')
    return svg(1024, 1024, page + mark + letters, TILE_GRADIENT + pivot_mask())


# ----------------------------------------------------------------- render ---

def render(svg_text, size):
    """Rasterise an SVG string to a square-ish RGBA PIL image `size` px wide."""
    doc = fitz.open(stream=svg_text.encode("utf-8"), filetype="svg")
    page = doc[0]
    zoom = size / page.rect.width
    pix = page.get_pixmap(matrix=fitz.Matrix(zoom, zoom), alpha=True)
    return Image.open(io.BytesIO(pix.tobytes("png"))).convert("RGBA")


def render_supersampled(svg_text, size):
    """Render at 4x then downsample; cleaner edges at small sizes."""
    big = render(svg_text, size * 4)
    h = round(big.height / 4)
    return big.resize((size, h), Image.LANCZOS)


# MuPDF does not render SVG gradients, so the gradient tile is painted with
# Pillow and the flat white mark is rendered on top of it.

def gradient_tile(size, radius_ratio=225 / 1024):
    ss = 4
    big = size * ss
    stops = [(0, _rgb(V["400"])), (0.45, _rgb(V["500"])), (1, _rgb(V["700"]))]
    # diagonal gradient: colour depends on (x + y); build one row of 2*big
    # samples and slice it, which is fast enough for 4096 px
    diag = Image.new("RGB", (2 * big, 1))
    dp = diag.load()
    for i in range(2 * big):
        t = i / (2 * big - 1)
        for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
            if t0 <= t <= t1:
                u = (t - t0) / (t1 - t0)
                dp[i, 0] = tuple(round(c0[k] + (c1[k] - c0[k]) * u) for k in range(3))
                break
    tile = Image.new("RGB", (big, big))
    for y in range(big):
        tile.paste(diag.crop((y, 0, y + big, 1)), (0, y))
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, big - 1, big - 1),
                                           radius=round(big * radius_ratio), fill=255)
    tile = tile.convert("RGBA")
    tile.putalpha(mask)
    return tile.resize((size, size), Image.LANCZOS)


# MuPDF also ignores SVG masks, so the hole in the pivot joint is punched
# out of rendered marks with Pillow.

def punch_pivot(img, small=False):
    """Cut the see-through hole out of a mark rendered on the 1024 grid."""
    ss = 4
    scale = img.width / 1024 * ss
    cx, cy = PIVOT
    r = HOLE_R_SMALL if small else HOLE_R
    hole = Image.new("L", (img.width * ss, img.height * ss), 255)
    ImageDraw.Draw(hole).ellipse(((cx - r) * scale, (cy - r) * scale,
                                  (cx + r) * scale, (cy + r) * scale), fill=0)
    hole = hole.resize(img.size, Image.LANCZOS)
    img.putalpha(ImageChops.multiply(img.getchannel("A"), hole))
    return img


def raster_mark(size, small=False, color=WHITE):
    """The Bone-F mark alone, with its pivot hole, as a PIL image."""
    src = svg(1024, 1024, bone_f(small=small, color=color, hole=False))
    img = render_supersampled(src, size) if size <= 256 else render(src, size)
    return punch_pivot(img, small)


def raster_icon(size, small=False):
    """App icon as a PIL image: tile plus the Bone-F mark. The small variant
    has a flat violet-600 tile and the heavier mark."""
    if small:
        tile = Image.new("RGBA", (size * 4, size * 4), (0, 0, 0, 0))
        ImageDraw.Draw(tile).rounded_rectangle((0, 0, size * 4 - 1, size * 4 - 1),
                                               radius=round(size * 4 * 200 / 1024),
                                               fill=_rgb(V["600"]))
        tile = tile.resize((size, size), Image.LANCZOS)
    else:
        tile = gradient_tile(size)
    tile.alpha_composite(raster_mark(size, small))
    return tile


def raster_icon_macos(size=1024):
    """macOS grid: 824/1024 tile centred, with a soft drop shadow."""
    from PIL import ImageFilter
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    inner = round(size * 824 / 1024)
    off = (size - inner) // 2
    icon = raster_icon(inner)
    shadow = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    alpha = icon.getchannel("A").point(lambda a: int(a * 0.3))
    shadow.paste((0, 0, 0, 255), (off, off + round(size * 10 / 1024)), alpha)
    shadow = shadow.filter(ImageFilter.GaussianBlur(size * 14 / 1024))
    canvas.alpha_composite(shadow)
    canvas.alpha_composite(icon, (off, off))
    return canvas


def write(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8", newline="\n")


# ------------------------------------------------------------ social card ---

def font(size, weight):
    path = BRAND / "fonts" / "Inter-Variable.ttf"
    f = ImageFont.truetype(str(path), size)
    try:
        f.set_variation_by_axes([size if size < 32 else 32, weight])
    except (OSError, ValueError):
        pass
    return f


def social_card(out):
    W, H = 1280, 640
    bg = Image.new("RGBA", (W, H))
    top, bottom = _rgb(V["950"]), _rgb(V["900"])
    px = bg.load()
    for y in range(H):
        for x in range(W):
            t = min(1, (x / W) * 0.4 + (y / H) * 0.6)
            px[x, y] = tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3)) + (255,)

    # Onion-skin F (brand concept B): faint offset copies behind the mark
    def ghost(color, opacity):
        # render solid, then fade the whole layer so overlaps don't darken
        img = raster_mark(520, color=color)
        img.putalpha(img.getchannel("A").point(lambda a: int(a * opacity)))
        return img

    x0 = 700
    for dx, color, op in ((-96, V["400"], 0.18), (-48, V["300"], 0.32)):
        bg.alpha_composite(ghost(color, op), (x0 + dx, 60))
    bg.alpha_composite(ghost(WHITE, 1), (x0, 60))

    word = render(wordmark_svg(WHITE), 470)
    bg.alpha_composite(word, (96, 214))
    d = ImageDraw.Draw(bg)
    d.text((100, 340), "Free to animate.", font=font(54, 700), fill=_rgb(V["200"]))
    d.text((100, 420), "Open-source 2D character animation.", font=font(30, 450),
           fill=_rgb(TOKENS["themes"]["night"]["text"]))
    d.text((100, 462), "Rig it. Draw it. Move it.", font=font(30, 450),
           fill=_rgb(TOKENS["themes"]["night"]["text-muted"]))
    d.text((100, 550), "Windows · macOS · Linux · Web", font=font(22, 500),
           fill=_rgb(V["300"]))
    bg.convert("RGB").save(out, optimize=True)


# ----------------------------------------------------------------- tokens ---

def _rgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


def _lum(h):
    c = [x / 255 for x in _rgb(h)]
    c = [x / 12.92 if x <= 0.03928 else ((x + 0.055) / 1.055) ** 2.4 for x in c]
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]


def contrast(a, b):
    la, lb = sorted((_lum(a), _lum(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def check_contrast():
    failures = []
    for name, theme in TOKENS["themes"].items():
        for fg, bg, minimum in TOKENS["contrast-checks"]:
            ratio = contrast(theme[fg], theme[bg])
            if ratio < minimum:
                failures.append(f"  {name}: {fg} on {bg} = {ratio:.2f} (needs {minimum})")
    return failures


def themes_css():
    lines = ["/* Generated from tokens.json by build_brand.py. Do not edit by hand. */", "",
             ":root {"]
    lines += [f"  --violet-{k}: {v};" for k, v in V.items()]
    for k, v in {**TOKENS["shape"], **TOKENS["motion"]}.items():
        lines.append(f"  --{k}: {v};")
    for k, v in TOKENS["font"].items():
        lines.append(f"  --font-{k}: {v};")
    lines.append("}")

    def block(selector, theme):
        out = ["", f"{selector} {{", "  color-scheme: " +
               ("light" if theme in ("day",) else "dark") + ";"]
        out += [f"  --{k}: {v};" for k, v in TOKENS["themes"][theme].items() if k != "label"]
        return out + ["}"]

    lines += block(':root, [data-theme="night"]', "night")
    lines += ["", "@media (prefers-color-scheme: light) {"]
    lines += ["  " + l if l else l for l in block(':root:not([data-theme])', "day")]
    lines += ["}"]
    for theme in ("day", "studio-grey"):
        lines += block(f'[data-theme="{theme}"]', theme)
    return "\n".join(lines) + "\n"


# ------------------------------------------------------------------- main ---

def main():
    failures = check_contrast()
    if failures:
        print("Contrast check failed:\n" + "\n".join(failures))
        sys.exit(1)
    print("Contrast checks passed.")

    css_path = BRAND / "tokens" / "themes.css"
    if "--check" in sys.argv:
        # CI mode: verify only. Rendered images differ slightly between
        # platforms, so they are not compared; the CSS is plain text.
        current = css_path.read_text(encoding="utf-8") if css_path.exists() else ""
        if current != themes_css():
            print("themes.css is out of date with tokens.json; run build_brand.py")
            sys.exit(1)
        print("themes.css is up to date.")
        return

    write(css_path, themes_css())

    logo = BRAND / "logo"
    icon_svg, small_svg = app_icon_svg(), app_icon_svg(small=True)
    write(logo / "fanimate-icon.svg", icon_svg)
    write(logo / "fanimate-icon-small.svg", small_svg)
    write(logo / "fanimate-mark-violet.svg", mono_mark_svg(V["600"]))
    write(logo / "fanimate-mark-white.svg", mono_mark_svg(WHITE))
    write(logo / "fanimate-mark-black.svg", mono_mark_svg("#000000"))
    write(logo / "fanimate-wordmark-violet.svg", wordmark_svg(V["600"]))
    write(logo / "fanimate-wordmark-white.svg", wordmark_svg(WHITE))
    write(logo / "fanimate-wordmark-ink.svg", wordmark_svg(INK))
    write(logo / "fanimate-lockup-light.svg", lockup_svg(INK))
    write(logo / "fanimate-lockup-dark.svg", lockup_svg(WHITE))

    # App icons: small variant for 16-32 px, full icon above
    icons = BRAND / "app-icon"
    icons.mkdir(exist_ok=True)
    pngs = {}
    for size in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
        img = raster_icon(size, small=size <= 32)
        img.save(icons / f"fanimate-{size}.png", optimize=True)
        pngs[size] = img
    pngs[256].save(icons / "fanimate.ico", sizes=[(s, s) for s in (16, 24, 32, 48, 64, 128, 256)],
                   append_images=[pngs[s] for s in (16, 24, 32, 48, 64, 128)])
    write(icons / "fanimate-macos.svg", app_icon_macos_svg())
    macos = raster_icon_macos(1024)
    macos.save(icons / "fanimate-macos-1024.png", optimize=True)
    macos.save(icons / "fanimate.icns")
    write(icons / "favicon.svg", small_svg)
    pngs[32].save(icons / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48)],
                  append_images=[pngs[16], pngs[48]])

    # .fnm file-type icon
    files = BRAND / "file-icon"
    write(files / "fnm-file.svg", file_icon_svg())
    fpng = {s: raster_file_icon(s) for s in (16, 32, 48, 256, 512, 1024)}
    fpng[512].save(files / "fnm-file-512.png", optimize=True)
    fpng[256].save(files / "fnm-file.ico", sizes=[(s, s) for s in (16, 32, 48, 256)],
                   append_images=[fpng[s] for s in (16, 32, 48)])
    fpng[1024].save(files / "fnm-file.icns")

    social_card(BRAND / "social" / "social-card.png")
    print("Brand assets written to", BRAND)


if __name__ == "__main__":
    main()
