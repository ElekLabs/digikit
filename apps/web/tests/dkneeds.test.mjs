// What a .dkm takes of the device: patchkit.wasm (`pk_needs`, packages/dkneeds) and the patchkit command line (`patchkit needs --json`) run the
// same Rust code on real packed modules and must agree to the field; the wasm reader refuses what is not a .dkm without a crash. The arithmetic itself
// is proved against the runtime in the scenarios `inferred-needs-*` and by the Rust unit tests of dkneeds. Skips when patchkit.wasm is not built.
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { DkNeeds } from '../src/modules/dkneeds.ts';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const wasmPath = join(root, 'apps/web/public/patchkit.wasm');
const py = (...args) =>
  execFileSync('uv', ['run', 'python', '-m', 'tools.patchkit.patchkit', ...args], {
    cwd: root,
    env: { ...process.env, PYTHONPATH: root },
    encoding: 'utf8',
  });
const MODULES = ['test/sine', 'digikit/remote', 'test/dyn-params', 'test/dyn-demo'];

test(
  'the wasm reader and the command line give the same needs for packed modules',
  { skip: !existsSync(wasmPath) },
  async () => {
    const work = mkdtempSync(join(tmpdir(), 'dkneeds-'));
    const reader = await DkNeeds.create(readFileSync(wasmPath));
    const files = MODULES.map((id) => {
      const path = join(work, `${id.replace('/', '-')}.dkm`);
      py('pack', id, '--device', 'dt2', '-o', path);
      return path;
    });
    const cli = JSON.parse(py('needs', '--json', ...files));
    const seen = {};
    for (const [id, path] of MODULES.map((id, i) => [id, files[i]])) {
      const bytes = new Uint8Array(readFileSync(path));
      const n = reader.of(bytes);
      assert.deepEqual(n, cli[path], id);
      assert.equal(n.file_size, bytes.length, id);
      seen[id] = n;
    }
    // what the modules are: a machine with DSP code takes a DSP range, a block for its relocated image, a voice row and a machine row
    const sine = seen['test/sine'];
    assert.ok(sine.DSP_AREA > 0 && sine.DSP_AREA % 64 === 0, 'the DSP range is whole granules');
    assert.ok(sine.ARENA_XFER > 0 && sine.VOICE_ROWS >= 1 && sine.MACHINE_ROWS >= 1);
    assert.equal(sine.ARENA % 16, 0);
    // a tool with no DSP code and no records takes the arena block only
    const remote = seen['digikit/remote'];
    assert.deepEqual([remote.DSP_AREA, remote.ARENA_XFER, remote.VOICE_ROWS, remote.MACHINE_ROWS], [0, 0, 0, 0]);
    assert.ok(remote.ARENA > 0);
  },
);

test('what is not a .dkm has no needs and does not crash the reader', { skip: !existsSync(wasmPath) }, async () => {
  const reader = await DkNeeds.create(readFileSync(wasmPath));
  for (const bytes of [
    new Uint8Array(0),
    new Uint8Array(0xa0),
    new Uint8Array(4096).fill(0xff),
    Uint8Array.of(1, 2, 3),
  ]) {
    assert.equal(reader.of(bytes), null);
    assert.match(reader.read(bytes).error, /not a \.dkm/);
  }
  // a header-sized file with the right magic, format and size fields but nothing else: the sizes do not fit
  const b = new Uint8Array(0xa0);
  const v = new DataView(b.buffer);
  v.setUint32(0, 0x444b4d46);
  v.setUint32(4, 1);
  v.setUint32(8, 0xa0);
  v.setUint32(0x0c, 0xa0);
  assert.match(reader.read(b).error, /sizes/);
  // the reader still works after refusing files
  assert.equal(reader.of(new Uint8Array(10)), null);
});
