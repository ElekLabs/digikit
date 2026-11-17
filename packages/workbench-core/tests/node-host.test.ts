// The NodeHost (src/node/node-host.ts) over temp folders, small stand-in bridge programs and a stand-in control socket: the file rules
// of the Host (local.rs, library.rs), the profiles, the MIDI and bulk bridges and the emulator RPC.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { afterAll, beforeAll, expect, test } from 'vitest';
import { createNodeHost, NodeHostUnsupported } from '../src/node/node-host.ts';

let dir: string;
const at = (...p: string[]) => path.join(dir, ...p);
const text = (b: ArrayBuffer) => Buffer.from(b).toString('utf8');

/** A program that stands in for a bridge binary. */
function program(name: string, source: string): string {
  const file = at(name);
  fs.writeFileSync(file, `#!/usr/bin/env node\n${source}`, { mode: 0o755 });
  return file;
}

beforeAll(() => {
  dir = fs.mkdtempSync(path.join(os.tmpdir(), 'digi-node-host-'));
  fs.mkdirSync(at('samples/drums'), { recursive: true });
  fs.writeFileSync(at('samples/kick.wav'), 'KICK');
  fs.writeFileSync(at('samples/drums/snare.wav'), 'SNARE!');
  fs.writeFileSync(at('samples/.DS_Store'), 'x');
  fs.writeFileSync(at('secret.txt'), 'outside');
  fs.symlinkSync(at('secret.txt'), at('samples/escape.txt'));
  fs.mkdirSync(at('out/modules/dt2/acme'), { recursive: true });
  fs.writeFileSync(at('out/modules/dt2/acme/synth.dkm'), 'SYNTH');
  fs.writeFileSync(at('out/modules/dt2/notes.txt'), 'not a module');
  fs.mkdirSync(at('devices'));
  fs.writeFileSync(
    at('devices/one.toml'),
    '[device]\nname = "Box One"\nshort = "b1"\n[storage]\nsamples = "stock-rpc"\n',
  );
  fs.writeFileSync(at('devices/two.toml'), '[device]\nname = "Box Two"\nshort = "b2"\n');
  fs.writeFileSync(at('devices/one-leds.toml'), '[leds]\nname = "none"\n');
  fs.mkdirSync(at('apps/web/public'), { recursive: true });
  fs.writeFileSync(at('apps/web/public/patchkit.wasm'), 'WASM');
});

afterAll(() => fs.rmSync(dir, { recursive: true, force: true }));

test('fs lists one level, folders first, without dot files and links that leave the folder', async () => {
  const host = createNodeHost({ root: dir });
  expect(await host.fs.list(at('samples'), '')).toEqual([
    { name: 'drums', kind: 'folder', size: 0 },
    { name: 'kick.wav', kind: 'file', size: 4 },
  ]);
  expect(await host.fs.list(at('samples'), 'drums')).toEqual([{ name: 'snare.wav', kind: 'file', size: 6 }]);
});

test('fs reads a file or its head and refuses paths outside the folder', async () => {
  const host = createNodeHost({ root: dir });
  assert.equal(text(await host.fs.read(at('samples'), 'drums/snare.wav', null)), 'SNARE!');
  assert.equal(text(await host.fs.read(at('samples'), 'drums/snare.wav', 3)), 'SNA');
  await expect(host.fs.read(at('samples'), '../secret.txt', null)).rejects.toThrow(/not a path inside/);
  await expect(host.fs.read(at('samples'), 'escape.txt', null)).rejects.toThrow(/not a path inside/);
  await expect(host.fs.read(at('samples'), '', null)).rejects.toThrow(/no file named/);
  await expect(host.fs.list(at('samples'), 'a//b')).rejects.toThrow(/not a path inside/);
});

test('dialogs and the app commands are unsupported; saves go to the output folder only when one is given', async () => {
  const host = createNodeHost({ root: dir });
  await expect(host.fs.pickFolder({ start: null })).rejects.toBeInstanceOf(NodeHostUnsupported);
  await expect(host.emu.launch()).rejects.toBeInstanceOf(NodeHostUnsupported);
  expect(host.emu.inApp()).toBe(false);
  await expect(host.fs.saveLog(new Uint8Array([1]), 'x.txt')).rejects.toBeInstanceOf(NodeHostUnsupported);
  const out = createNodeHost({ root: dir, outDir: at('saved') });
  const file = await out.fs.saveLog(new Uint8Array([65, 66]), 'log.txt');
  assert.ok(file);
  assert.equal(fs.readFileSync(file, 'utf8'), 'AB');
  await expect(out.fs.saveLog(new Uint8Array(), '../x')).rejects.toThrow(/not a file name/);
});

test('the library lists .dkm files by path, reads them and checks the names', async () => {
  const lib = createNodeHost({ root: dir }).library;
  expect(await lib.scan('dt2')).toEqual([{ path: 'acme/synth.dkm', size: 5 }]);
  expect(await lib.scan('dn2')).toEqual([]);
  await expect(lib.scan('../x')).rejects.toThrow(/not a device id/);
  assert.equal(text(await lib.read('dt2', 'acme/synth.dkm')), 'SYNTH');
  await expect(lib.read('dt2', 'acme/../../x.dkm')).rejects.toThrow(/not a library file/);
  await expect(lib.read('dt2', 'notes.txt')).rejects.toThrow(/not a library file/);
});

test('the device storage comes from the profile files', async () => {
  const lib = createNodeHost({ root: dir }).library;
  expect(await lib.deviceStorage('b1')).toEqual({ name: 'Box One', samples: 'stock-rpc' });
  await expect(lib.deviceStorage('b2')).rejects.toThrow(/no \[storage\]/);
  await expect(lib.deviceStorage('zz')).rejects.toThrow(/no device profile/);
});

test('the wasm files come from the checkout, and a missing one names the build', async () => {
  const fw = createNodeHost({ root: dir }).firmware;
  assert.equal(text(await fw.patchkitWasm()), 'WASM');
  await expect(fw.sampleWasm()).rejects.toThrow(/mise run wasm:patchkit/);
});

test('the MIDI bridge: ports, then whole messages out and back', async () => {
  const bin = program(
    'dkmidi',
    `const [cmd] = process.argv.slice(2);
if (cmd === 'ports') console.log('Fake A\\nFake B');
else { process.stdin.on('data', (d) => process.stdout.write(d)); process.stdin.on('end', () => process.exit(0)); }`,
  );
  const { midi } = createNodeHost({ root: dir, dkmidiBin: bin }).ports;
  expect(await midi.ports()).toEqual(['Fake A', 'Fake B']);
  await midi.open('Fake A');
  const msg = Uint8Array.from([0xf0, 0x7d, 1, 2, 3, 0xf7]);
  await midi.send(msg);
  expect(new Uint8Array(await midi.recv(2000))).toEqual(msg);
  // nothing more arrives: the wait ends at its time with an empty buffer, and `cancel` ends it at once
  expect((await midi.recv(50)).byteLength).toBe(0);
  const long = midi.recv(60000);
  await midi.cancel();
  expect((await long).byteLength).toBe(0);
  await midi.close();
  await expect(midi.send(msg)).rejects.toThrow(/no MIDI port is open/);
});

test('a missing bridge binary says which one', async () => {
  const host = createNodeHost({ root: dir, dkmidiBin: at('nope') });
  await expect(host.ports.midi.ports()).rejects.toThrow(/no dkmidi binary at/);
  await expect(createNodeHost({ root: dir }).ports.bulk.list()).rejects.toThrow(/no dkbulk binary given/);
});

test('the bulk bridge: the list marks the OUT-only interfaces and an OUT-only link writes frames to the bridge', async () => {
  const sink = at('bulk-out.bin');
  const bin = program(
    'dkbulk',
    `const fs = require('node:fs');
const args = process.argv.slice(2);
if (args[0] === 'list') process.stdout.write('1:2\\tPair\\tx\\t\\n3:4\\tAudio\\tx\\tout-only\\n');
else { process.stdin.on('data', (d) => fs.appendFileSync(${JSON.stringify(sink)}, d)); process.stdin.on('end', () => process.exit(0)); }`,
  );
  const { bulk } = createNodeHost({ root: dir, dkbulkBin: bin }).ports;
  expect(await bulk.list()).toEqual([
    { id: '1:2', name: 'Pair', out_only: false },
    { id: '3:4', name: 'Audio', out_only: true },
  ]);
  await bulk.openOutOnly('3:4');
  await bulk.sendOutOnly(Uint8Array.from([1, 2, 3]));
  await bulk.closeOutOnly();
  assert.deepEqual([...fs.readFileSync(sink)], [1, 2, 3]);
  await expect(bulk.sendOutOnly(new Uint8Array(1))).rejects.toThrow(/no OUT-only link/);
});

test('the emulator RPC goes over the control socket, and without one it says so', async () => {
  const sock = at('c.sock');
  const server = net.createServer((conn) => {
    let buf = '';
    conn.setEncoding('utf8');
    conn.on('data', (chunk: string) => {
      buf += chunk;
      for (let nl = buf.indexOf('\n'); nl >= 0; nl = buf.indexOf('\n')) {
        const req = JSON.parse(buf.slice(0, nl));
        buf = buf.slice(nl + 1);
        const result = req.method === 'hello' ? { schema_version: 1 } : { method: req.method, params: req.params };
        conn.write(`${JSON.stringify({ jsonrpc: '2.0', id: req.id, result })}\n`);
      }
    });
  });
  await new Promise<void>((r) => server.listen(sock, r));
  const host = createNodeHost({ root: dir, socket: sock });
  try {
    expect(await host.rpc.call('status', { a: 1 }, 7)).toEqual({ method: 'status', params: { a: 1 } });
  } finally {
    host.close();
    server.close();
  }
  await expect(createNodeHost({ root: dir }).rpc.call('status')).rejects.toThrow(/no control socket/);
});
