// Tests of the Node client against a real fanimate-cli. Set FANIMATE_CLI or build it
// first (cargo build -p fanimate-cli); without one they are skipped.
import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { Host, FanimateError } from '../index.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
function locate() {
  if (process.env.FANIMATE_CLI) return process.env.FANIMATE_CLI;
  for (const name of ['fanimate-cli.exe', 'fanimate-cli']) {
    const p = path.join(here, '..', '..', '..', 'target', 'debug', name);
    if (fs.existsSync(p)) return p;
  }
  return null;
}
const cli = locate();
const options = { skip: cli ? false : 'fanimate-cli was not found (set FANIMATE_CLI or build it)' };

test('a layer is made and read back, with non-ASCII text', options, async () => {
  const host = await Host.start({ cli });
  try {
    assert.ok(await host.version());
    const layer = await host.add_text_layer({ content: 'Héllo مرحبا 你好', x: 10, y: 20 });
    const layers = await host.layers();
    assert.ok(layers.some((l) => l.id === layer));
  } finally {
    await host.close();
  }
});

test('a failed call rejects with the stable code', options, async () => {
  const host = await Host.start({ cli });
  try {
    await assert.rejects(host.delete_layer({ id: 9999 }), (e) => e instanceof FanimateError && e.code === 'not_found');
    // the host still answers after a failure
    assert.ok(await host.version());
  } finally {
    await host.close();
  }
});

test('permissions limit what the host does', options, async () => {
  const host = await Host.start({ cli, permissions: ['read'] });
  try {
    await assert.rejects(host.add_layer({ kind: 'vector', name: 'x' }), (e) => e.code === 'permission_denied');
    assert.deepEqual(await host.layers(), []);
  } finally {
    await host.close();
  }
});

test('a missing program is a clear error', async () => {
  await assert.rejects(Host.start({ cli: path.join(here, 'no-such-fanimate-cli') }), (e) => e.code === 'not_found');
});
