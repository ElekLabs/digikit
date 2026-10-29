// patchkit.wasm's flash entries (`pk_prepare`, the `pk_sender_*` handles) and the `patchkit flash` command line make the same image: byte for byte
// the file `--save` writes, for a patch and for a restore, on both devices. The summary carries the checks and the message boundaries, and the
// Rust sender paces from JS. Skips when the wasm, a stock image or the CLI is absent.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { PatchKit } from '../src/patch/patchkit-core.ts';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../..');
const wasmPath = join(root, 'apps/web/public/patchkit.wasm');
const images = {
  dt2: join(root, 'firmware', 'Digitakt_II_OS1.17.syx'),
  dn2: join(root, 'firmware', 'Digitone_II_OS1.12.syx'),
};
const need = [wasmPath, ...Object.values(images)].every(existsSync);
const py = (module, ...args) =>
  execFileSync('uv', ['run', 'python', '-m', module, ...args], {
    cwd: root,
    env: { ...process.env, PYTHONPATH: root },
    encoding: 'utf8',
    maxBuffer: 1 << 28,
  });

test(
  'pk_prepare and `patchkit flash --save` make the same image, for a patch and a restore',
  { skip: !need },
  async () => {
    const work = mkdtempSync(join(tmpdir(), 'pk-flash-'));
    const bundlePath = join(work, 'bundle.json');
    py('tools.patchkit.patchkit', 'bundle', '-o', bundlePath);
    const kit = await PatchKit.create(readFileSync(wasmPath), readFileSync(bundlePath));
    for (const [name, path] of Object.entries(images)) {
      const stock = readFileSync(path);
      for (const restore of [false, true]) {
        const out = join(work, `${name}${restore ? '-restore' : ''}.syx`);
        py('tools.patchkit.patchkit', 'flash', path, '--dry-run', '--save', out, ...(restore ? ['--restore'] : []));
        const cli = readFileSync(out);
        const p = kit.prepare(new Uint8Array(stock), restore);
        assert.equal(
          Buffer.compare(Buffer.from(p.image), cli),
          0,
          `${name} ${restore ? 'restore' : 'patch'}: wasm image equals the CLI's`,
        );
        assert.equal(p.ok, true, JSON.stringify(p.checks.filter((c) => !c.ok)));
        assert.equal(p.device, name);
        assert.equal(p.restore, restore);
        assert.equal(p.paceMs, 2);
        // every check of the CLI is there, including the DSP scan
        const names = p.checks.map((c) => c.name);
        assert.ok(names.includes('DSP scan') && names.includes('round trip'), names.join());
        assert.equal(names.includes('image id'), !restore);
        assert.equal(p.imageId !== null, !restore);
        assert.equal(p.runtime !== null, !restore);
        // the messages tile the image: a 16-byte header, 128-byte data messages, a 16-byte footer
        assert.equal(p.ends.at(-1), p.image.length);
        assert.equal(p.ends[0], 16);
        assert.ok(p.ends.slice(1, -1).every((e, i) => e - (p.ends[i] ?? 0) === 128));
        assert.equal(p.ends.at(-1) - p.ends.at(-2), 16);
        assert.equal(p.shaImage.length, 64);
        if (restore) assert.equal(Buffer.compare(Buffer.from(p.image), stock), 0, 'a restore sends the stock file');
      }
    }
  },
);

test('a patched file is refused as a stock file, with a message', { skip: !need }, async () => {
  const work = mkdtempSync(join(tmpdir(), 'pk-flash-'));
  const bundlePath = join(work, 'bundle.json');
  py('tools.patchkit.patchkit', 'bundle', '-o', bundlePath);
  const kit = await PatchKit.create(readFileSync(wasmPath), readFileSync(bundlePath));
  const stock = new Uint8Array(readFileSync(images.dn2));
  const patched = kit.prepare(stock, false).image;
  assert.throws(() => kit.prepare(patched, false), /patched image; give the stock file/);
  assert.throws(() => kit.prepare(new Uint8Array(100), false), /./);
});

test(
  'the Rust sender paces from JS: the wait is the rest of 2 ms, an error stops it for good',
  { skip: !existsSync(wasmPath) },
  async () => {
    const kit = await PatchKit.create(readFileSync(wasmPath), '{"ports":[],"modules":[]}');
    const s = kit.sender(3);
    assert.deepEqual(s.next(100), { kind: 'send', index: 0 });
    assert.deepEqual(s.next(100), { kind: 'wait', ms: 2 });
    assert.deepEqual(s.next(101), { kind: 'wait', ms: 1 });
    assert.deepEqual(s.next(102), { kind: 'send', index: 1 });
    assert.deepEqual(s.next(110), { kind: 'send', index: 2 });
    assert.deepEqual(s.next(110), { kind: 'done' });
    assert.equal(s.sent(), 3);
    s.free();
    const f = kit.sender(5);
    assert.deepEqual(f.next(0), { kind: 'send', index: 0 });
    f.fail();
    assert.deepEqual(f.next(1000), { kind: 'aborted' });
    // a new sender starts from the header, independent of the one that failed
    const n = kit.sender(5);
    assert.deepEqual(n.next(2000), { kind: 'send', index: 0 });
    assert.deepEqual(kit.sender(0).next(0), { kind: 'done' });
  },
);
