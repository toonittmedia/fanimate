// The Fanimate runtime player for the web: reads a .fnmr file (version 1) and says what
// is drawn on any frame, or draws it as an SVG string. The reference for other
// languages: its meaning is the specification in docs/spec/runtime.md, and the Rust
// reader in fanimate-export checks the same file against the animation program.
//
//   import { Runtime } from './fanimate-runtime.mjs';
//   const runtime = await Runtime.open(await (await fetch('walk.fnmr')).arrayBuffer());
//   const frame = runtime.first + (t * runtime.fps) % runtime.count;
//   svgElement.innerHTML = runtime.toSvgBody(frame);
//
// No dependencies. Works in browsers and in Node 18 or later (it unpacks the archive with
// DecompressionStream).

export const FORMAT = 'fanimate-runtime';
export const VERSION = 1;

/** Read a file from a ZIP archive: stored or deflated entries, nothing else. */
async function unzip(bytes, name) {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  // the end of central directory record is within the last 64 KB + 22 bytes
  let end = -1;
  for (let i = bytes.length - 22; i >= Math.max(0, bytes.length - 65557); i--) {
    if (view.getUint32(i, true) === 0x06054b50) {
      end = i;
      break;
    }
  }
  if (end < 0) throw new Error('not a ZIP archive');
  const entries = view.getUint16(end + 10, true);
  let at = view.getUint32(end + 16, true);
  for (let n = 0; n < entries; n++) {
    if (view.getUint32(at, true) !== 0x02014b50) throw new Error('damaged ZIP directory');
    const method = view.getUint16(at + 10, true);
    const compressed = view.getUint32(at + 20, true);
    const nameLength = view.getUint16(at + 28, true);
    const extra = view.getUint16(at + 30, true);
    const comment = view.getUint16(at + 32, true);
    const local = view.getUint32(at + 42, true);
    const entryName = new TextDecoder().decode(bytes.subarray(at + 46, at + 46 + nameLength));
    at += 46 + nameLength + extra + comment;
    if (entryName !== name) continue;
    const dataAt = local + 30 + view.getUint16(local + 26, true) + view.getUint16(local + 28, true);
    const data = bytes.subarray(dataAt, dataAt + compressed);
    if (method === 0) return data;
    if (method !== 8) throw new Error(`ZIP method ${method} is not supported`);
    const stream = new Blob([data]).stream().pipeThrough(new DecompressionStream('deflate-raw'));
    return new Uint8Array(await new Response(stream).arrayBuffer());
  }
  throw new Error(`${name} is not in the archive`);
}

/** The matrix and opacity at `frame`: linear between keys, holding before the first and after the last. */
export function motion(keys, frame) {
  if (!Array.isArray(keys) || keys.length === 0) return { m: [1, 0, 0, 1, 0, 0], opacity: 1 };
  const after = keys.findIndex((k) => k.frame > frame);
  if (after === 0) return { m: keys[0].m, opacity: keys[0].opacity };
  if (after < 0) {
    const last = keys[keys.length - 1];
    return { m: last.m, opacity: last.opacity };
  }
  const a = keys[after - 1];
  const b = keys[after];
  const t = b.frame > a.frame ? (frame - a.frame) / (b.frame - a.frame) : 0;
  return { m: a.m.map((v, i) => v + (b.m[i] - v) * t), opacity: a.opacity + (b.opacity - a.opacity) * t };
}

/** The last stepped key at or before `frame`, or undefined. */
function stepped(keys, frame) {
  if (!Array.isArray(keys)) return undefined;
  let found;
  for (const k of keys) {
    if (k.frame <= frame) found = k;
    else break;
  }
  return found;
}

export class Runtime {
  constructor(root) {
    this.root = root;
    this.name = root.name ?? '';
    this.fps = root.fps;
    this.width = root.width;
    this.height = root.height;
    this.first = root.first_frame;
    this.last = root.last_frame;
    /** How many frames the animation has. */
    this.count = this.last - this.first + 1;
  }

  /** Read a .fnmr archive, or the bare JSON of one (an ArrayBuffer, a typed array or text). */
  static async open(input) {
    let text;
    if (typeof input === 'string') {
      text = input;
    } else {
      const bytes = input instanceof Uint8Array ? input : new Uint8Array(input);
      const zipped = bytes.length > 4 && bytes[0] === 0x50 && bytes[1] === 0x4b;
      text = new TextDecoder().decode(zipped ? await unzip(bytes, 'runtime.json') : bytes);
    }
    let root;
    try {
      root = JSON.parse(text);
    } catch (e) {
      throw new Error(`not a runtime file: ${e.message}`);
    }
    if (root?.format !== FORMAT) throw new Error('not a Fanimate runtime file');
    if (root.version !== VERSION) {
      throw new Error(`runtime version ${root.version} is not one this player knows (${VERSION})`);
    }
    return new Runtime(root);
  }

  /** What is drawn on `frame`, back to front: name, path data, paint, matrix and opacity. */
  sample(frame) {
    const whole = Math.floor(frame);
    const out = [];
    for (const slot of this.root.slots ?? []) {
      const shown = Array.isArray(slot.present) && slot.present.some((r) => whole >= r[0] && whole <= r[1]);
      if (!shown) continue;
      const path = stepped(slot.path_keys, whole);
      const paint = stepped(slot.paint_keys, whole) ?? slot.paint ?? {};
      const { m, opacity } = motion(slot.keys, frame);
      out.push({
        name: slot.name ?? '',
        d: path ? path.d : slot.d ?? '',
        fill: paint.fill ?? null,
        stroke: paint.stroke ?? null,
        m,
        opacity,
      });
    }
    return out;
  }

  /** The frame as the inside of an <svg> element (paths with transforms). */
  toSvgBody(frame) {
    const n = (v) => String(Math.round(v * 10000) / 10000);
    let out = '';
    for (const s of this.sample(frame)) {
      const attrs = [`d="${s.d}"`, `transform="matrix(${s.m.map(n).join(' ')})"`];
      if (s.fill) {
        attrs.push(`fill="${s.fill.color}"`);
        const a = s.fill.alpha * s.opacity;
        if (a < 0.999) attrs.push(`fill-opacity="${n(a)}"`);
      } else {
        attrs.push('fill="none"');
      }
      if (s.stroke) {
        attrs.push(`stroke="${s.stroke.color}"`, `stroke-width="${n(s.stroke.width)}"`);
        const a = s.stroke.alpha * s.opacity;
        if (a < 0.999) attrs.push(`stroke-opacity="${n(a)}"`);
      }
      out += `<path ${attrs.join(' ')}/>\n`;
    }
    return out;
  }

  /** The frame as a whole SVG document. */
  toSvg(frame) {
    const n = (v) => String(Math.round(v * 1000) / 1000);
    return (
      `<svg xmlns="http://www.w3.org/2000/svg" width="${n(this.width)}" height="${n(this.height)}" ` +
      `viewBox="0 0 ${n(this.width)} ${n(this.height)}">\n${this.toSvgBody(frame)}</svg>\n`
    );
  }
}
