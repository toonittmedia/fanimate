// Five styles of mouth shapes for lip sync, as SVG: nine files per style named after
// Rhubarb's letters (A to H and X), which Fanimate's Lip Sync matches by name.
//
//   node scripts/template-art/mouths.mjs
//
// writes crates/fanimate-editor/assets/templates/mouths/<style>/<letter>.svg. Every file is
// 240 x 180 with the mouth centred on (120, 90), so any style swaps for another in place.
// Only plain fills and strokes are used: Fanimate's SVG import has no clips or gradients.
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';
import { write, sub, shift, reverse, curveD, d, ellipseD, path, svg, group } from './lib.mjs';

const OUT = join(dirname(fileURLToPath(import.meta.url)), '../../crates/fanimate-editor/assets/templates/mouths');
const CX = 120;
const CY = 90;

/**
 * The nine poses. `a` is the half width. `top` and `low` are how far the open mouth reaches
 * above and below the corners. `round` pulls the corners in to make an O. `teeth` is the
 * depth of the upper row, `lower` the depth of the lower row, `tongue` its height.
 */
const POSES = {
  A: { closed: true, a: 46, seam: 2 },
  B: { a: 50, top: 3, low: 11, teeth: 7, lower: 5 },
  C: { a: 48, top: 10, low: 26, teeth: 9, tongue: 9 },
  D: { a: 52, top: 14, low: 46, teeth: 12, tongue: 16 },
  E: { a: 30, top: 24, low: 24, round: true, tongue: 9 },
  F: { a: 17, top: 17, low: 17, round: true },
  G: { a: 48, top: 3, low: 17, teeth: 14, lipOverTeeth: 5 },
  H: { a: 46, top: 9, low: 28, teeth: 8, tongue: 24, tongueUp: true },
  X: { smile: true, a: 40, lift: 5 },
};

const STYLES = {
  // glossy lips, dark mouth, tongue
  classic: { lip: '#c9505f', lipEdge: '#9d3748', cavity: '#4a1420', teeth: '#fffaf0', tongue: '#e8707e', lipW: 7, k: 1 },
  // two tones and no outlines
  flat: { cavity: '#3b1d3a', teeth: '#ffffff', tongue: '#ff7a8a', line: '#3b1d3a', lineW: 7, k: 1 },
  // thick black ink, like old cartoons
  ink: { ink: '#16131a', inkW: 8, cavity: '#8c2431', teeth: '#fffdf4', tongue: '#f08fa0', k: 1 },
  // outlines only, a doodle
  line: { ink: '#25222c', inkW: 5, k: 1 },
  // small and round, for a cute character
  cute: { cavity: '#6e2a36', teeth: '#ffffff', tongue: '#ff97a8', line: '#6e2a36', lineW: 6, blush: '#ffb3c0', k: 0.72 },
};

/** The mouth's outline as two cubic curves: the top edge left to right, the bottom edge back. */
function cavity(pose, k, grow = 0) {
  const a = (pose.a + grow) * k;
  const top = (pose.top + grow) * k;
  const low = (pose.low + grow) * k;
  const cx = pose.round ? 1 : 0.5;
  const up = [[CX - a, CY], [CX - a * cx, CY - top * 1.333], [CX + a * cx, CY - top * 1.333], [CX + a, CY]];
  const down = [[CX + a, CY], [CX + a * cx, CY + low * 1.333], [CX - a * cx, CY + low * 1.333], [CX - a, CY]];
  return { up, down };
}

/** A band hugging a curve: the curve's span [u0, u1], and the same span moved by `dy`. */
function band(curve, u0, u1, dy) {
  const s = sub(curve, u0, u1);
  const t = shift(s, 0, dy);
  return d(s[0], [[s[1], s[2], s[3]], [t[3]], [t[2], t[1], t[0]]]);
}

/** Everything one pose draws, back to front, as [id, path data, style]. */
function pieces(pose, S) {
  const k = S.k;
  const out = [];
  const inked = (fill) => ({ fill, stroke: S.ink, sw: S.inkW });

  if (pose.smile) {
    const a = pose.a * k, lift = pose.lift * k;
    const curve = [[CX - a, CY - lift], [CX - a * 0.5, CY + lift * 1.2], [CX + a * 0.5, CY + lift * 1.2], [CX + a, CY - lift]];
    const colour = S.ink ?? S.line ?? S.lip;
    const w = S.inkW ?? S.lineW ?? S.lipW + 2;
    out.push(['smile', curveD(curve), { stroke: S.ink ?? S.lip ?? colour, sw: w }]);
    return withBlush(out, S);
  }

  if (pose.closed) {
    const a = pose.a * k;
    // two lips pressed together: a lens with a seam
    const lens = {
      up: [[CX - a, CY], [CX - a * 0.5, CY - 11 * k], [CX + a * 0.5, CY - 11 * k], [CX + a, CY]],
      down: [[CX + a, CY], [CX + a * 0.5, CY + 13 * k], [CX - a * 0.5, CY + 13 * k], [CX - a, CY]],
    };
    const lensD = d(lens.up[0], [[lens.up[1], lens.up[2], lens.up[3]], [lens.down[1], lens.down[2], lens.down[3]]]);
    const seam = curveD([[CX - a, CY], [CX - a * 0.4, CY + 3 * k], [CX + a * 0.4, CY + 3 * k], [CX + a, CY]]);
    if (S.lip) {
      out.push(['lips', lensD, { fill: S.lip }]);
      out.push(['seam', seam, { stroke: S.lipEdge, sw: 4 }]);
    } else if (S.ink) {
      out.push(['lips', lensD, inked(S.cavity ?? 'none')]);
      out.push(['seam', seam, { stroke: S.ink, sw: S.inkW - 2 }]);
    } else {
      out.push(['seam', seam, { stroke: S.line, sw: S.lineW }]);
    }
    return withBlush(out, S);
  }

  const { up, down } = cavity(pose, k);
  const hole = d(up[0], [[up[1], up[2], up[3]], [down[1], down[2], down[3]]]);

  if (S.lip) {
    const o = cavity(pose, k, S.lipW);
    out.push(['lips', d(o.up[0], [[o.up[1], o.up[2], o.up[3]], [o.down[1], o.down[2], o.down[3]]]), { fill: S.lip }]);
  }
  out.push(['mouth', hole, S.ink ? inked(S.cavity ?? 'none') : { fill: S.cavity ?? 'none', stroke: S.cavity ? undefined : S.line, sw: S.lineW }]);

  // filled styles paint teeth and tongue; the doodle only draws one line for each
  const filled = Boolean(S.cavity);
  const edge = (id, dd, fill) => out.push([id, dd, { fill }]);
  const stroke = (id, dd) => out.push([id, dd, { stroke: S.ink, sw: S.inkW - 1 }]);

  if (pose.tongue) {
    // a mound on the floor of the mouth; raised, it fills the middle
    const floor = sub(down, 0.2, 0.8);
    const h = pose.tongue * k;
    const lift = pose.tongueUp ? 1.5 : 1.333;
    const top = [[floor[3][0] + 6 * k, floor[3][1] - h * lift], [floor[0][0] - 6 * k, floor[0][1] - h * lift], floor[0]];
    if (filled) edge('tongue', d(floor[0], [[floor[1], floor[2], floor[3]], top]), S.tongue);
    else stroke('tongue', d(floor[3], [top], false));
  }
  if (pose.teeth) {
    const depth = pose.teeth * k;
    if (filled) edge('teeth', band(up, 0.14, 0.86, depth), S.teeth);
    else stroke('teeth', curveD(shift(sub(up, 0.14, 0.86), 0, depth)));
  }
  if (pose.lower && filled) {
    edge('teeth lower', band(reverse(down), 0.14, 0.86, -pose.lower * k), S.teeth);
  }
  if (pose.lipOverTeeth) {
    // the lower lip tucks under the upper teeth (F and V)
    const depth = pose.lipOverTeeth * k;
    if (filled) edge('lower lip', band(reverse(down), 0.1, 0.9, -depth), S.lip ?? S.tongue);
    else stroke('lower lip', curveD(shift(sub(reverse(down), 0.1, 0.9), 0, -depth)));
  }
  return withBlush(out, S);
}

/** The cute style's rosy cheeks sit either side of the mouth. */
function withBlush(out, S) {
  if (!S.blush) return out;
  const side = (id, x) => [id, ellipseD(x, CY + 2, 13, 8), { fill: S.blush }];
  return [side('cheek L', CX - 62), side('cheek R', CX + 62), ...out];
}

for (const [style, S] of Object.entries(STYLES)) {
  for (const [letter, pose] of Object.entries(POSES)) {
    const items = pieces(pose, S).map(([id, dd, st]) => path(id, dd, st));
    write(join(OUT, style, `${letter}.svg`), svg(240, 180, group(`mouth ${letter}`, items)));
  }
}
console.log(`wrote ${Object.keys(STYLES).length * Object.keys(POSES).length} mouth shapes to ${OUT}`);
