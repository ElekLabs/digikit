import { readFileSync } from 'node:fs';
import path from 'node:path';
import { Sample, prepareUpload } from '@digi/dk-session/dksample';
import { openSamples } from '@digi/dk-session/samples';
import { repoRoot } from '@digi/test-support';
import { expect } from 'vitest';
import type { Session } from '@digi/test-support';
import { installWithDebug, liveArtifacts, memory, panel, record, runtimeTests, settleUi } from '../../../../tests/regression/runtime/live.ts';
import { planSymbol, prepareRuntimePlan } from '../../../../tests/regression/runtime/runtime-plan.ts';
import {
  AUDIBLE,
  RATE,
  captureNote,
  expectSplitPair,
  onset,
  selectDynamicMachine,
  trackApi,
  trackRecord,
  until,
  wav,
} from './spark-support.ts';

// DT2 1.17: the on-device sample picker of digikit/spark (docs/design/sample-picker.md), driven with the panel keys:
//   SETTINGS > DIGIKIT > MODULES > (SPARK) RIGHT > SETTINGS... > SAMPLES... > YES on track 1 > folder > YES on the file.
// The result is read from the runtime's own records (`dk_strack` of track 1, `sample_track_status` through test/track-select), not from the
// framebuffer; the screens are saved in the artifacts for a look. Menus keep their selected row, so every list is reset with UP first and the row
// is counted from the listing (the browser) or read from the menu's own row tables (MODULES: DK_LOAD, the side panel: DK_PANEL).

const FOLDER = 'KICKS';
const TRACK = 0;

/** Rows of a 24-byte text table of core/digikit-menu (`DK_LOAD`/`DK_LOAD_N`, `DK_PANEL`/`DK_PANEL_N`). */
async function menuRows(session: Session, rows: number, count: number): Promise<string[]> {
  const n = await session.client.memPeek(count);
  if (n === null || n < 0 || n > 40) throw new Error(`row count ${n}`);
  const out: string[] = [];
  for (let i = 0; i < n; i++) {
    const row = Buffer.alloc(24);
    for (let off = 0; off < 24; off += 4) row.writeUInt32BE(((await session.client.memPeek(rows + i * 24 + off)) ?? 0) >>> 0, off);
    out.push(row.toString('latin1').split('\0')[0]!);
  }
  return out;
}

/** The order the picker shows a folder in: folders first, then names, case-insensitive. */
const pickerOrder = (entries: { name: string; kind: 'file' | 'folder' }[]) =>
  [...entries].sort((a, b) => {
    const [x, y] = [a.name.toLowerCase(), b.name.toLowerCase()];
    return Number(b.kind === 'folder') - Number(a.kind === 'folder') || (x < y ? -1 : x > y ? 1 : 0);
  });

runtimeTests(
  'the SPARK picker selects, plays, clears and refuses on track 1 through the panel keys',
  async (runtime) => {
    const { session, dk } = runtime;
    const artifacts = liveArtifacts(session, 'spark-picker');
    const api = trackApi(runtime);
    const ui = panel(session);
    const mem = memory(session);
    const log: Record<string, unknown> = {};
    const shot = async (name: string) => record(artifacts, `screen-${name}`, await session.client.framebufferGet({ hash: true, data: true }));
    try {
      await settleUi(session);
      await installWithDebug(runtime, 'digikit/spark', 'test/track-select');
      await settleUi(session);
      const plan = await prepareRuntimePlan(session, artifacts, ['RECORDS', 'DK_LOAD', 'DK_LOAD_N', 'DK_PANEL', 'DK_PANEL_N']);
      const sym = (name: string) => planSymbol(plan, name);
      await selectDynamicMachine(runtime, artifacts);

      // ---- fixtures: a stereo 48 kHz file KICKS/K1 and a file that is not a sample, KICKS/BAD -----------------------------------------
      const sample = await Sample.create(
        Uint8Array.from(readFileSync(path.join(repoRoot(), 'apps/web/public/dksample.wasm'))),
      );
      const backend = openSamples('stock-rpc', { session: dk, hashOf: (data) => sample.hash(data) });
      await backend.mkdir(FOLDER);
      const good = prepareUpload(sample, 'K1.wav', wav(3, [440, 660]));
      expect(good.kind).toBe('converted');
      await backend.put(FOLDER, good);
      expect((await backend.verify(FOLDER, good)).ok, 'K1 verifies on the card').toBe(true);
      const bad = prepareUpload(sample, 'BAD', Uint8Array.from({ length: 300 }, (_, i) => (i * 37 + 11) & 0xff));
      await backend.put(FOLDER, bad);
      const root = pickerOrder(await backend.list(''));
      const inFolder = pickerOrder(await backend.list(FOLDER));
      log.listing = { root, inFolder };
      const folderRow = 1 + root.findIndex((e) => e.name === FOLDER); // row 0 is ".."
      const fileRow = (name: string) => 1 + inFolder.findIndex((e) => e.name === name);
      expect(folderRow, `${FOLDER} is in the root listing`).toBeGreaterThan(0);
      expect(fileRow('K1'), 'K1 is in the folder listing').toBeGreaterThan(0);
      expect(fileRow('BAD'), 'BAD is in the folder listing').toBeGreaterThan(0);

      const down = async (n: number) => {
        for (let i = 0; i < n; i++) await ui.tap('DOWN');
      };
      const toTop = async (rows = 24) => {
        for (let i = 0; i < rows; i++) await ui.tap('UP');
      };

      // ---- navigation: SETTINGS > DIGIKIT > MODULES > SPARK > RIGHT > SETTINGS... > SAMPLES... --------------------------------------
      // From the main screen to the track list of the picker (the menus keep their rows, so each list is reset with UP first).
      async function openTracks(first: boolean) {
        for (let i = 0; i < 8; i++) await ui.tap('NO');
        await ui.tap('SETTINGS', 600);
        await toTop(10);
        await down(8); // the ninth row of SETTINGS is DIGIKIT
        await ui.tap('YES', 600);
        await toTop(6);
        await down(3); // ABOUT, HEALTH, MEMORY, MODULES
        await ui.tap('YES', 800);
        const modules = await menuRows(session, sym('DK_LOAD'), sym('DK_LOAD_N'));
        const sparkRow = modules.findIndex((r) => /^spark/i.test(r.trim()));
        if (first) log.modules = modules;
        expect(sparkRow, `SPARK in the MODULES list ${JSON.stringify(modules)}`).toBeGreaterThanOrEqual(0);
        await toTop(modules.length + 2);
        await down(sparkRow);
        await ui.tap('RIGHT', 600);
        const side = await menuRows(session, sym('DK_PANEL'), sym('DK_PANEL_N'));
        if (first) log.sidePanel = side;
        const settingsRow = side.findIndex((r) => r.startsWith('SETTINGS'));
        expect(settingsRow, `SETTINGS... in the side panel ${JSON.stringify(side)}`).toBeGreaterThanOrEqual(0);
        await toTop(side.length + 2);
        await down(settingsRow);
        await ui.tap('YES', 800);
        if (first) await shot('module-settings');
        await ui.tap('YES', 800); // the module's menu has one row, SAMPLES...
        if (first) await shot('track-list');
      }
      await openTracks(true);

      // ---- pick KICKS/K1 for track 1 ---------------------------------------------------------------------------------------------------
      await toTop(18);
      const watched: { t: number; state: number; token: number }[] = [];
      let watching = true;
      const t0 = Date.now();
      const watcher = (async () => {
        while (watching) {
          const s = await api.status(TRACK);
          if (!watched.length || watched.at(-1)!.state !== s.state) watched.push({ t: Date.now() - t0, state: s.state, token: s.token });
          await new Promise((r) => setTimeout(r, 50));
        }
      })();
      expect((await api.status(TRACK)).state, 'track 1 starts unused').toBe(0);
      await ui.tap('YES', 1500); // track 1: the browser lists the root
      await shot('browser-root');
      await toTop(root.length + 3);
      await down(folderRow);
      await ui.tap('YES', 1500); // into KICKS
      await shot('browser-folder');
      await toTop(inFolder.length + 3);
      await down(fileRow('K1'));
      await ui.tap('YES', 300); // select K1: the browser closes by itself
      const ready = await api.settle(TRACK);
      watching = false;
      await watcher;
      log.pick = { watched, ready };
      const states = watched.map((s) => s.state);
      expect(states, `the track went queued/loading to ready ${JSON.stringify(watched)}`).toEqual(expect.arrayContaining([3]));
      expect(states.filter((s) => s === 1 || s === 2).length, `queued or loading seen ${JSON.stringify(watched)}`).toBeGreaterThan(0);
      expect(states[0], 'the first state is not an error').toBeGreaterThanOrEqual(0);
      expect(ready.state).toBe(3);
      const r1 = await trackRecord(session, sym('RECORDS'), TRACK);
      log.record = r1;
      expect(r1.ready, 'dk_strack ready').toBe(1);
      expect(r1.channels).toBe(2);
      expect(r1.rate).toBe(RATE);
      expect(Math.abs(r1.frames! - 3 * RATE), `frames ${r1.frames}`).toBeLessThanOrEqual(64);
      expect(r1.token).toBe(ready.token);
      // The picker passed the root-relative path "KICKS/K1": the runtime found the file under the device root (a wrong form would be NOT_FOUND).
      await shot('track-list-ready');

      // ---- refusal: BAD keeps the old sample ---------------------------------------------------------------------------------------------
      // The track list is open (the picker opened it again) on row 1; the browser opens in KICKS where it was left.
      await toTop(18);
      await ui.tap('YES', 1500);
      await toTop(inFolder.length + 3);
      await down(fileRow('BAD'));
      await ui.tap('YES', 1500);
      await expect.poll(async () => (await api.status(TRACK)).state, { timeout: 20_000, interval: 100 }).not.toBeGreaterThan(0);
      const afterBad = await api.status(TRACK);
      const recBad = await trackRecord(session, sym('RECORDS'), TRACK);
      log.refusal = { status: afterBad, record: recBad };
      await shot('after-bad');
      // run 1: the select of BAD is accepted and the load ends with E_FORMAT (-5), which is the sticky status of the track (the row shows `!BAD FORMAT`);
      // the DSP record of the old sample is not touched, so K1 keeps playing (the note below).
      expect(afterBad.state, 'the refusal is the sticky E_FORMAT status').toBe(-5);
      expect(recBad.ready).toBe(1);
      expect(recBad.token, 'the old token stays in the DSP record').toBe(ready.token);
      expect(recBad.frames).toBe(r1.frames);

      // ---- play a note with the menus closed: both source channels reach the output ------------------------------------------------------
      for (let i = 0; i < 8; i++) await ui.tap('NO');
      const note = await captureNote(runtime, path.join(artifacts, 'note.wav'), async (clock) => {
        await until(clock, (await clock()) + 1.5 * RATE);
      });
      const o = onset(note);
      expect(o, 'the note is audible').toBeGreaterThanOrEqual(0);
      log.noteSplit = expectSplitPair('picker K1', note, o + 4800, o + RATE, 440, 660);
      await session.expectNoFault('picker note');

      // ---- CLEAR from the track panel ------------------------------------------------------------------------------------------------------
      await openTracks(false);
      await toTop(18);
      await ui.tap('RIGHT', 600); // the panel of track 1: CLEAR, INFO
      await shot('track-panel');
      await toTop(4);
      await ui.tap('YES', 1000); // CLEAR
      await shot('after-clear');
      await expect
        .poll(async () => (await api.status(TRACK)).state, { timeout: 20_000, interval: 100 })
        .toBe(0);
      await expect
        .poll(async () => (await trackRecord(session, sym('RECORDS'), TRACK)).ready, { timeout: 20_000, interval: 100 })
        .toBe(0);
      log.cleared = await trackRecord(session, sym('RECORDS'), TRACK);

      const modulesNow = await dk.modules();
      for (const m of modulesNow) expect(m.faults, `${m.name} faults`).toBe(0);
      await session.expectNoFault('picker');
      void mem;
      void AUDIBLE;
    } finally {
      record(artifacts, 'measurements', log);
      console.info('spark-picker measurements', JSON.stringify(log));
      try {
        record(artifacts, 'runtime-log', await dk.readLog());
      } catch {
        // The log is a diagnostic only.
      }
    }
  },
  { devices: ['dt2'], patchSet: 'sample-tracks', timeout: 900_000 },
);
