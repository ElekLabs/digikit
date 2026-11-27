import fs from 'node:fs';
import path from 'node:path';
import { be32, type DkSession } from '@digi/dk-session';
import { expect } from 'vitest';
import { blob } from '@digi/regression/runtime/upload-bytes.ts';
import { cardRig } from '@digi/regression/runtime/card-rig.ts';
import { RT } from '@digi/regression/runtime/runtime-roots.ts';
import { type CardRig } from '@digi/regression/runtime/card-rig-api.ts';
import { rawModule } from '@digi/regression/runtime/service-files.ts';
import { SETTLE } from '@digi/regression/runtime/panel-waits.ts';
import type { Device } from '@digi/test-support';

/**
 * Power cuts during an SDK file write (tests/fixtures/modules/file-write), the way upload-faults.device.test.ts does for SysEx; shared by
 * file-write-cut-{write,replace,batch}.device.test.ts. The job writes `data/sdk-cut.bin` (two 4096-byte chunks and a 777-byte tail: three data writes), then
 * file_close commits (table, header B, header A). The first write is the repair of header A that BEGIN does on a card built by plusdrive (`sync_headers`): seven
 * card writes in all. `DKRT_UPLOAD_CUT` = N refuses every write after the Nth; `DKRT_UPLOAD_TEAR` = N tears the Nth (one sector) and refuses the rest.
 *
 * The device samples a few points per kind and shows what only the firmware and the card show: the next boot reads the store whole, and plusdrive reads the same from
 * the card. The sweep over every card write (old or new table, hashes verify, the space freed, the next write works) is host-tested over the real writer on the fake
 * card: runtime/core/src/rs/src/store_tests.rs (`an_sdk_write_cut_...`, `an_sdk_replace_cut_...`, `a_batch_commit_cut_...`, with the tear variants).
 */

export const CMD = 0x24;
export const WRITE = 1;
export const REPLACE = 0x40; // added to the case: DK_FILE_REPLACE
const BATCH = 11; // the fixture's batch case: after the path, u8 n files and u8 mode (0: dk_files_commit after the last close)
export const BATCH_N = 4;
export const BATCH_SIZE = 2000;
const BATCH_BASE = 'batch/cut';
export const SIZE = 2 * 4096 + 777;
export const PATH = 'data/sdk-cut.bin';
export const pattern = (n: number) => Buffer.from(Uint8Array.from({ length: n }, (_, i) => (i * 7 + (i >> 8)) & 255));
export const shifted = (n: number, i: number) =>
  Buffer.from(Uint8Array.from({ length: n }, (_, k) => ((k + 13 * i) * 7 + ((k + 13 * i) >> 8)) & 255));
export const batchPath = (i: number) => `${BATCH_BASE}${String(i).padStart(2, '0')}`;
const cstr = (s: string) => Buffer.concat([Buffer.from(s), Buffer.from([0])]);

/** The sampled points of a single-file write: the first write, the last data write, a tear of header B, and a cut after header B. */
export const cases = [
  ['a cut after the first write (the repair of header A)', { DKRT_UPLOAD_CUT: 1 }, false],
  ['a cut after the last data write (the table is refused)', { DKRT_UPLOAD_CUT: 4 }, false],
  ['a tear of header B', { DKRT_UPLOAD_TEAR: 6 }, false],
  ['a cut after header B (header A is refused)', { DKRT_UPLOAD_CUT: 6 }, true],
] as const;

/** Start a write job of the fixture and run the emulator until it has ended; returns the job's status and the close result. */
export async function runJob(
  rig: CardRig,
  dk: DkSession,
  size: number,
  file: string,
  kind = WRITE,
): Promise<{ job: number; close: number }> {
  const r = await dk.request(CMD, Buffer.concat([Buffer.from([kind]), be32(size), cstr(file)]));
  expect(r.status, 'the job is queued').toBe(0);
  for (let i = 0; i < 200; i++) {
    await rig.runtime.ui.wait({ ms: 100, why: SETTLE });
    const b = Buffer.from((await dk.request(CMD + 1)).body);
    if (b[0] === 2) return { job: b.readInt32BE(13), close: b.readInt32BE(5) };
  }
  throw new Error('the job did not end');
}

/** Run the fixture's batch job (BATCH_N deferred files, one commit) until it has ended; returns the job's status. */
export async function runBatch(
  rig: CardRig,
  dk: DkSession,
  replace = false,
): Promise<{ job: number; close: number; commit: number }> {
  const body = Buffer.concat([
    Buffer.from([BATCH | (replace ? REPLACE : 0)]),
    be32(BATCH_SIZE),
    cstr(BATCH_BASE),
    Buffer.from([BATCH_N, 0]),
  ]);
  expect((await dk.request(CMD, body)).status, 'the batch job is queued').toBe(0);
  for (let i = 0; i < 300; i++) {
    await rig.runtime.ui.wait({ ms: 100, why: SETTLE });
    const b = Buffer.from((await dk.request(CMD + 1)).body);
    if (b[0] === 2) return { job: b.readInt32BE(13), close: b.readInt32BE(5), commit: b.readInt32BE(41) };
  }
  throw new Error('the batch job did not end');
}

/** The card rig of the three cut files (the same name: the cards are shared through the card cache). */
export const cutRig = (device: Device) =>
  cardRig(device, 'file-write-cut', {
    dsp: 'zero',
    roots: RT,
    symbols: ['DKRT_UPLOAD_CUT', 'DKRT_UPLOAD_TEAR', 'DKRT_UPLOAD_WRITES'],
    project: true,
    uploads: true,
    relaxFs: true,
  });

/** The old module file (3000 bytes) of the base cards. */
export function oldModule(rig: CardRig, name: string): { old: Buffer; oldFile: string } {
  const old = blob(3000, 21);
  const oldFile = path.join(rig.artifacts, name);
  fs.writeFileSync(oldFile, old);
  rawModule(oldFile);
  return { old, oldFile };
}
