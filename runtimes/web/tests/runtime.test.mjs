// Tests of the web player: a hand-made file (every rule of the format), and a real file
// made by fanimate-cli from a project built with the Node client, when the CLI is built.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { Runtime, motion } from '../fanimate-runtime.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));

const file = {
  format: 'fanimate-runtime',
  version: 1,
  name: 'hand made',
  fps: 24,
  width: 200,
  height: 100,
  first_frame: 1,
  last_frame: 10,
  background: '#ffffff',
  slots: [
    {
      name: 'ball',
      d: 'M0 0L10 0L10 10L0 10Z',
      present: [[1, 4], [7, 10]],
      paint: { fill: { color: '#ff0000', alpha: 1 }, stroke: null },
      paint_keys: [
        { frame: 1, fill: { color: '#ff0000', alpha: 1 }, stroke: null },
        { frame: 8, fill: { color: '#0000ff', alpha: 0.5 }, stroke: { color: '#000000', alpha: 1, width: 2 } },
      ],
      path_keys: [
        { frame: 1, d: 'M0 0L10 0L10 10L0 10Z' },
        { frame: 9, d: 'M0 0L20 0L20 20L0 20Z' },
      ],
      keys: [
        { frame: 1, m: [1, 0, 0, 1, 0, 0], opacity: 1 },
        { frame: 5, m: [1, 0, 0, 1, 40, 0], opacity: 0.5 },
      ],
    },
  ],
};

test('motion interpolates linearly between keys and holds at both ends', () => {
  const keys = file.slots[0].keys;
  assert.deepEqual(motion(keys, 1).m, [1, 0, 0, 1, 0, 0]);
  assert.deepEqual(motion(keys, 3).m, [1, 0, 0, 1, 20, 0]);
  assert.equal(motion(keys, 3).opacity, 0.75);
  assert.deepEqual(motion(keys, 0).m, [1, 0, 0, 1, 0, 0], 'held before the first key');
  assert.deepEqual(motion(keys, 99).m, [1, 0, 0, 1, 40, 0], 'held after the last key');
  assert.deepEqual(motion([], 3), { m: [1, 0, 0, 1, 0, 0], opacity: 1 });
  assert.deepEqual(motion(undefined, 3).m, [1, 0, 0, 1, 0, 0]);
});

test('a slot is drawn only on the frames it is present, with stepped paint and path', async () => {
  const runtime = await Runtime.open(JSON.stringify(file));
  assert.equal(runtime.count, 10);
  assert.equal(runtime.sample(2).length, 1);
  assert.equal(runtime.sample(5).length, 0, 'between its two ranges');
  assert.equal(runtime.sample(6.9).length, 0);
  assert.equal(runtime.sample(7).length, 1);
  assert.equal(runtime.sample(11).length, 0, 'after the end');
  assert.equal(runtime.sample(2)[0].fill.color, '#ff0000');
  assert.equal(runtime.sample(8)[0].fill.color, '#0000ff');
  assert.equal(runtime.sample(8)[0].stroke.width, 2);
  assert.equal(runtime.sample(8)[0].d, 'M0 0L10 0L10 10L0 10Z');
  assert.equal(runtime.sample(9)[0].d, 'M0 0L20 0L20 20L0 20Z');
});

test('the frame is written as SVG with the matrix and opacity on each path', async () => {
  const runtime = await Runtime.open(JSON.stringify(file));
  const svg = runtime.toSvg(3);
  assert.match(svg, /viewBox="0 0 200 100"/);
  assert.match(svg, /transform="matrix\(1 0 0 1 20 0\)"/);
  assert.match(svg, /fill="#ff0000" fill-opacity="0.75"/);
  assert.ok(svg.trim().endsWith('</svg>'));
  assert.equal((await Runtime.open(JSON.stringify(file))).toSvgBody(5), '');
});

test('what is not a runtime file is refused with a reason, and a damaged one never throws on sample', async () => {
  await assert.rejects(Runtime.open('not json'), /not a runtime file/);
  await assert.rejects(Runtime.open('{"format": "other"}'), /not a Fanimate runtime file/);
  await assert.rejects(Runtime.open('{"format": "fanimate-runtime", "version": 9}'), /version 9/);
  const hollow = await Runtime.open('{"format": "fanimate-runtime", "version": 1, "first_frame": 1, "last_frame": 2, "slots": [{"present": "x", "keys": 3}, {}]}');
  assert.deepEqual(hollow.sample(1), []);
  await assert.rejects(Runtime.open(new Uint8Array([0x50, 0x4b, 1, 2, 3, 4])), /ZIP/);
});

function locateCli() {
  if (process.env.FANIMATE_CLI) return process.env.FANIMATE_CLI;
  for (const name of ['fanimate-cli.exe', 'fanimate-cli']) {
    const p = path.join(here, '..', '..', '..', 'target', 'debug', name);
    if (fs.existsSync(p)) return p;
  }
  return null;
}

test('a .fnmr written by fanimate-cli plays: the square moves along its eased keys', { skip: !locateCli() && 'fanimate-cli was not found' }, async () => {
  const cli = locateCli();
  const { Host } = await import('../../../clients/node/index.mjs');
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'fnmr-'));
  try {
    const project = path.join(dir, 'a.fnm');
    const host = await Host.start({ cli, save: project });
    const layer = await host.add_layer({ kind: 'vector', name: 'art' });
    await host.add_shape({ layer, shape: { kind: 'rectangle', x: 10, y: 10, width: 20, height: 20, fill: '#e63946' } });
    await host.set_layer_transform({ id: layer, frame: 1, x: 0 });
    await host.set_layer_transform({ id: layer, frame: 5, x: 40 });
    await host.save();
    await host.close();
    const out = path.join(dir, 'a.fnmr');
    const r = spawnSync(cli, ['export', project, '-o', out, '--start', '1', '--end', '5'], { encoding: 'utf8' });
    assert.equal(r.status, 0, r.stderr + r.stdout);
    const runtime = await Runtime.open(new Uint8Array(fs.readFileSync(out)));
    assert.equal(runtime.count, 5);
    const smooth = (u) => u * u * (3 - 2 * u);
    for (let i = 0; i < 5; i++) {
      const square = runtime.sample(1 + i).find((s) => s.fill?.color === '#e63946');
      assert.ok(square, `frame ${1 + i}`);
      assert.ok(Math.abs(square.m[4] - 40 * smooth(i / 4)) < 0.01, `frame ${1 + i}: ${square.m[4]}`);
    }
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
