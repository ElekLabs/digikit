import { execFileSync, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { crc32 } from 'node:zlib';
import { afterAll, beforeAll, expect, test } from 'vitest';

const module = path.resolve(import.meta.dirname, '..');
const root = path.resolve(module, '../../../..');
let directory: string, binary: string;

const reference = `
import base64,json,random,sys
from tools.bootscreen.asset import Frame,encode,decode
out=[]
for item in json.load(sys.stdin):
 if item.get('fixture'):
  pixels=bytearray(1024)
  for x0,y0,x1,y1 in [(0,0,5,2),(123,61,127,63),(17,11,18,18),(48,32,51,39)]:
   for x in range(x0,x1+1):
    for y in range(y0,y1+1): pixels[8*x+y//8] |= 1 << (7-y%8)
  data=encode([Frame(1,random.Random(19).randbytes(1024)),Frame(1,bytes(1024)),Frame(2,bytes(pixels))])
 elif 'blob' in item: data=base64.b64decode(item['blob'])
 else: data=encode([Frame(f['ticks'],base64.b64decode(f['pixels'])) for f in item['frames']])
 try:
  asset=decode(data)
  out.append({'blob':base64.b64encode(data).decode(),'records':len(asset.frames),'ticks':sum(f.ticks for f in asset.frames),'expanded':base64.b64encode(b''.join(f.pixels*f.ticks for f in asset.frames)).decode()})
 except ValueError: out.append({'invalid':True})
print(json.dumps(out))
`;
interface Reference {
  blob: string;
  records: number;
  ticks: number;
  expanded: string;
  invalid?: boolean;
}
function python(items: object[]): Reference[] {
  return JSON.parse(
    execFileSync('uv', ['run', '--no-sync', 'python', '-c', reference], {
      cwd: root,
      env: { ...process.env, UV_CACHE_DIR: process.env.UV_CACHE_DIR ?? path.join(os.tmpdir(), 'digi-boot-query-uv') },
      input: JSON.stringify(items),
      encoding: 'utf8',
      timeout: 10_000,
      maxBuffer: 4 * 1024 * 1024,
    }),
  );
}
function run(data: Buffer) {
  return spawnSync(binary, { input: data, timeout: 2000, maxBuffer: 16 + 300 * 1024 });
}
function valid(got: Reference) {
  const result = run(Buffer.from(got.blob, 'base64'));
  expect(result.error).toBeUndefined();
  expect(result.status).toBe(0);
  expect(result.stdout.readUInt32BE(0)).toBe(got.records);
  expect(result.stdout.readUInt32BE(4)).toBe(got.ticks);
  expect(result.stdout.readUInt32BE(8)).toBe(got.records);
  expect(result.stdout.subarray(16)).toEqual(Buffer.from(got.expanded, 'base64'));
}
function record(hold: number, codec: number, data: Buffer, reserved = 0): Buffer {
  const header = Buffer.alloc(8);
  header.writeUInt16BE(hold);
  header.writeUInt16BE(data.length, 2);
  header[4] = codec;
  header[5] = reserved;
  return Buffer.concat([header, data]);
}
function asset(records: Buffer[], count = records.length, ticks = records.length): Buffer {
  const body = Buffer.concat(records),
    header = Buffer.alloc(16);
  header.write('DKBS');
  header.writeUInt16BE(1, 4);
  header.writeUInt16BE(count, 6);
  header.writeUInt32BE(ticks, 8);
  header.writeUInt32BE(crc32(body), 12);
  return Buffer.concat([header, body]);
}
function fixture(name: string): Buffer {
  const source = fs.readFileSync(path.join(root, `tests/fixtures/modules/boot-dkbs-${name}/src/asset.c`), 'utf8');
  return Buffer.from([...source.matchAll(/0x([0-9a-f]{2})/g)].map((match) => Number.parseInt(match[1]!, 16)));
}

beforeAll(() => {
  directory = fs.mkdtempSync(path.join(os.tmpdir(), 'boot-dkbs-host-'));
  binary = path.join(directory, 'asset-host');
  execFileSync(
    'cc',
    [
      '-std=c11',
      '-Wall',
      '-Wextra',
      '-Werror',
      '-I',
      path.join(module, 'src'),
      path.join(module, 'src/asset.c'),
      path.join(module, 'tests/asset-host.c'),
      '-o',
      binary,
    ],
    { timeout: 10_000 },
  );
});
afterAll(() => fs.rmSync(directory, { recursive: true, force: true }));

test('same C decoder matches the Python reference across RAW/XOR/holds and 300 ticks', () => {
  const noise = Buffer.from(Array.from({ length: 1024 }, (_, i) => (i * 73 + (i >> 3) * 17) % 256));
  const changed = Buffer.from(noise);
  changed[513]! ^= 0x80;
  const [mixed, maximum, sparse] = python([
    {
      frames: [
        { ticks: 1, pixels: noise.toString('base64') },
        { ticks: 2, pixels: changed.toString('base64') },
        { ticks: 1, pixels: Buffer.alloc(1024).toString('base64') },
      ],
    },
    { frames: [{ ticks: 300, pixels: noise.toString('base64') }] },
    {
      frames: Array.from({ length: 300 }, (_, i) => {
        const pixels = Buffer.alloc(1024);
        pixels[i % 1024] = 1 << (i % 8);
        return { ticks: 1, pixels: pixels.toString('base64') };
      }),
    },
  ]);
  for (const got of [mixed!, maximum!, sparse!]) valid(got);
});

test('valid repeated records and noncanonical literal/repeat-one streams are accepted', () => {
  const pixels = Buffer.from(Array.from({ length: 1024 }, (_, i) => i % 256));
  const literals = Buffer.concat(
    Array.from({ length: 8 }, (_, i) => Buffer.concat([Buffer.from([127]), pixels.subarray(i * 128, (i + 1) * 128)])),
  );
  const repeatOne = Buffer.concat([...pixels].map((byte) => Buffer.from([128, byte])));
  const blobs = [
    asset([record(1, 0, pixels), record(1, 0, pixels)]),
    asset([record(1, 1, literals)]),
    asset([record(1, 1, repeatOne)]),
  ];
  for (const got of python(blobs.map((blob) => ({ blob: blob.toString('base64') })))) valid(got);
});

test('committed providers match the reference and expand RAW noise, RAW zero, XOR diagnostic and hold', () => {
  const [expected] = python([{ fixture: true }]);
  expect(fixture('valid')).toEqual(Buffer.from(expected!.blob, 'base64'));
  valid(expected!);
  const corrupt = run(fixture('corrupt'));
  expect(corrupt.status).toBe(1);
  expect(corrupt.stdout.length).toBe(0);
});

test('malformed assets including invalid later records are rejected before any tick output', () => {
  const black = record(1, 0, Buffer.alloc(1024));
  const good = asset([black]);
  const over = Buffer.from(Array(9).fill([255, 0]).flat()),
    under = Buffer.from(Array(7).fill([255, 0]).flat());
  const bad = [
    Buffer.alloc(0),
    Buffer.alloc(65537),
    asset([black], 0),
    asset([black], 301),
    asset([black], 1, 0),
    asset([black], 1, 301),
    asset([black], 2),
    asset([black], 1, 2),
    asset([record(0, 0, Buffer.alloc(1024))]),
    asset([record(301, 0, Buffer.alloc(1024))]),
    asset([record(1, 2, Buffer.alloc(1024))]),
    asset([record(1, 0, Buffer.alloc(1024), 1)]),
    asset([record(1, 0, Buffer.alloc(1023))]),
    asset([record(1, 1, Buffer.alloc(0))]),
    asset([record(1, 1, over)]),
    asset([record(1, 1, under)]),
    asset([record(1, 1, Buffer.from([127, 0]))]),
    asset([record(1, 1, Buffer.from([255]))]),
    asset([black, record(1, 1, over)]), // valid prefix must not be published before full validation
  ];
  for (const cut of [0, 1, 4, 15, 16, 20, 23, 24, 25, 512, good.length - 1]) bad.push(good.subarray(0, cut));
  const crc = Buffer.from(good);
  crc[12]! ^= 1;
  bad.push(crc);
  const magic = Buffer.from(good);
  magic[0] = 0;
  bad.push(magic);
  const version = Buffer.from(good);
  version.writeUInt16BE(2, 4);
  bad.push(version);
  const trailing = Buffer.concat([good, Buffer.from([0])]);
  trailing.writeUInt32BE(crc32(trailing.subarray(16)), 12);
  bad.push(trailing);
  const short = Buffer.from(good.subarray(0, good.length - 1));
  short.writeUInt32BE(crc32(short.subarray(16)), 12);
  bad.push(short);
  const verdicts = python(bad.map((blob) => ({ blob: blob.toString('base64') })));
  for (const [index, blob] of bad.entries()) {
    expect(verdicts[index]?.invalid, `reference case ${index}`).toBe(true);
    const got = run(blob);
    expect(got.error, `case ${index}`).toBeUndefined();
    expect(got.status, `case ${index}`).toBe(1);
    expect(got.stdout.length, `case ${index}`).toBe(0);
  }
});
