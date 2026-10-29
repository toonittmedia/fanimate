// Small helpers for writing the template artwork as SVG. No dependencies.
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname } from 'node:path';

/** Write a text file, making its folder first. */
export function write(path, text) {
  mkdirSync(dirname(path), { recursive: true });
  writeFileSync(path, text);
}

const r = (n) => Math.round(n * 100) / 100;

/** A point as "x y". */
export const pt = ([x, y]) => `${r(x)} ${r(y)}`;

/** A cubic Bezier as four points; `sub` cuts it to the span [a, b] of its parameter. */
export function sub([p0, p1, p2, p3], a, b) {
  const lerp = (p, q, t) => [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
  const split = ([q0, q1, q2, q3], t) => {
    const a01 = lerp(q0, q1, t), a12 = lerp(q1, q2, t), a23 = lerp(q2, q3, t);
    const b0 = lerp(a01, a12, t), b1 = lerp(a12, a23, t);
    const c = lerp(b0, b1, t);
    return [[q0, a01, b0, c], [c, b1, a23, q3]];
  };
  let curve = [p0, p1, p2, p3];
  if (b < 1) curve = split(curve, b)[0];
  if (a > 0) curve = split(curve, a / b)[1];
  return curve;
}

/** Move every point of a curve by (dx, dy). */
export const shift = (c, dx, dy) => c.map(([x, y]) => [x + dx, y + dy]);

/** The same curve walked the other way. */
export const reverse = (c) => [c[3], c[2], c[1], c[0]];

/** Path data from a start point and cubic curves (each as three points), optionally closed. */
export function d(start, parts, close = true) {
  let s = `M${pt(start)}`;
  for (const p of parts) {
    s += p.length === 3 ? `C${pt(p[0])} ${pt(p[1])} ${pt(p[2])}` : `L${pt(p[0])}`;
  }
  return s + (close ? 'Z' : '');
}

/** Path data for a whole cubic curve. */
export const curveD = (c, close = false) => d(c[0], [[c[1], c[2], c[3]]], close);

/** Path data for an ellipse, as four cubic curves. */
export function ellipseD(cx, cy, rx, ry) {
  const k = 0.5523;
  return (
    `M${r(cx + rx)} ${r(cy)}` +
    `C${r(cx + rx)} ${r(cy + ry * k)} ${r(cx + rx * k)} ${r(cy + ry)} ${r(cx)} ${r(cy + ry)}` +
    `C${r(cx - rx * k)} ${r(cy + ry)} ${r(cx - rx)} ${r(cy + ry * k)} ${r(cx - rx)} ${r(cy)}` +
    `C${r(cx - rx)} ${r(cy - ry * k)} ${r(cx - rx * k)} ${r(cy - ry)} ${r(cx)} ${r(cy - ry)}` +
    `C${r(cx + rx * k)} ${r(cy - ry)} ${r(cx + rx)} ${r(cy - ry * k)} ${r(cx + rx)} ${r(cy)}Z`
  );
}

/** A rounded rectangle as path data. */
export function rectD(x, y, w, h, rad = 0) {
  rad = Math.min(rad, w / 2, h / 2);
  if (rad <= 0) return `M${r(x)} ${r(y)}H${r(x + w)}V${r(y + h)}H${r(x)}Z`;
  const k = rad * 0.4477;
  return (
    `M${r(x + rad)} ${r(y)}H${r(x + w - rad)}C${r(x + w - k)} ${r(y)} ${r(x + w)} ${r(y + k)} ${r(x + w)} ${r(y + rad)}` +
    `V${r(y + h - rad)}C${r(x + w)} ${r(y + h - k)} ${r(x + w - k)} ${r(y + h)} ${r(x + w - rad)} ${r(y + h)}` +
    `H${r(x + rad)}C${r(x + k)} ${r(y + h)} ${r(x)} ${r(y + h - k)} ${r(x)} ${r(y + h - rad)}` +
    `V${r(y + rad)}C${r(x)} ${r(y + k)} ${r(x + k)} ${r(y)} ${r(x + rad)} ${r(y)}Z`
  );
}

/** One <path> element. `style` is { fill, stroke, sw }; a missing fill means none. */
export function path(id, dd, { fill, stroke, sw = 0 } = {}) {
  const f = fill ?? 'none';
  const s = stroke ? ` stroke="${stroke}" stroke-width="${sw}" stroke-linecap="round" stroke-linejoin="round"` : '';
  return `<path id="${id}" d="${dd}" fill="${f}"${s}/>`;
}

/** A group: each top-level group of a file becomes a layer in Fanimate. */
export const group = (id, ...items) => `<g id="${id}">\n${items.flat().join('\n')}\n</g>`;

/** A whole SVG file; the view box is the size, so units match document units. */
export function svg(w, h, ...items) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${w} ${h}">\n${items.flat().join('\n')}\n</svg>\n`;
}
