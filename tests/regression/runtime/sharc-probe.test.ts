import fs from 'node:fs';
import { LOG_KIND } from '@digi/dk-session/gen_proto';
import { expect } from 'vitest';
import type { PatchedSession } from '../support.ts';
import { moduleArtifact, runtimeTests, settleLive } from './live.ts';

/** The probe transport (with remote also selected): the SHARC probe effect sends one framed word per block through the module step word, and the ColdFire collector keeps it. */
const PROBE = 'test/sharc-probe';
const COLLECTOR = 'digikit/probe';
/** Selected too: it takes service command slots (the collector got E_PROTO on a device when remote used all of them). */
const REMOTE = 'digikit/remote';
/** `STEP_PATCH` outcomes (sdk.toml log kinds). */
const STEP_PATCHED = 0;
const STEP_NONE = 3;
const CMD_STATUS = 0x32;
const CMD_RESET = 0x34;
/** A run is 1674 frames and takes about 33 s of emulated time; the test checks this many frames from the start of the capture, not a whole run. */
const FRAMES = 60;

function status(body: Uint8Array): Record<string, number> {
  const keys = [
    'format',
    'blocks',
    'records',
    'drops',
    'repeats',
    'idle',
    'words',
    'overruns',
    'cap',
    'parsed',
    'frames_ok',
    'seq_errors',
    'cks_errors',
  ];
  const view = new DataView(body.buffer, body.byteOffset, body.byteLength);
  return Object.fromEntries(keys.map((key, i) => [key, view.getUint32(4 * i)]));
}

async function expectProbeTransport({ session, dk }: PatchedSession): Promise<void> {
  await settleLive(session);
  const paths: string[] = [];
  for (const id of [REMOTE, PROBE, COLLECTOR]) {
    const bytes = fs.readFileSync(moduleArtifact(session.options.device, id));
    const path = `modules/${id.split('/').at(-1)}.dkm`;
    await dk.put(path, bytes);
    expect(await dk.verifyFile(path, bytes), `${id} reads back from the card`).toBe(true);
    paths.push(path);
  }
  await dk.select(paths);
  await dk.restart();
  await dk.waitForBoot();
  await expect
    .poll(async () => (await dk.modules()).map((m) => m.name), { timeout: 30_000, interval: 500 })
    .toEqual(expect.arrayContaining([REMOTE, PROBE, COLLECTOR]));

  // the effect's step stores went to the DSP's reply record, not to the static word that the effect core clears
  const patches = (await dk.readLog()).filter((e) => e.kind === LOG_KIND.STEP_PATCH);
  expect(
    patches.map((e) => e.args[0]),
    `STEP_PATCH outcomes: ${JSON.stringify(patches)}`,
  ).toEqual(expect.arrayContaining([STEP_PATCHED]));
  expect(
    patches.every((e) => e.args[0] === STEP_PATCHED || e.args[0] === STEP_NONE),
    'no static fallback',
  ).toBe(true);

  expect((await dk.request(CMD_RESET)).status).toBe(0);
  // the effect starts a few thousand blocks after the boot; the collector then fills its ring
  let last: Record<string, number> = {};
  await expect
    .poll(
      async () => {
        const reply = await dk.request(CMD_STATUS);
        expect(reply.status, 'STATUS answers').toBe(0);
        last = status(reply.body);
        return last.frames_ok;
      },
      { timeout: 300_000, interval: 2000, message: `${FRAMES} frames` },
    )
    .toBeGreaterThanOrEqual(FRAMES);
  expect(last.seq_errors, `sequence errors: ${JSON.stringify(last)}`).toBe(0);
  expect(last.cks_errors, `checksum errors: ${JSON.stringify(last)}`).toBe(0);
  expect(last.drops, `records the callback missed: ${JSON.stringify(last)}`).toBe(0);
  await session.expectNoFault('probe transport');
}

runtimeTests('the SHARC probe sends framed words that the collector receives without errors', expectProbeTransport, {
  timeout: 600_000,
});
