// The Calibration section's backend (src/calibration-backend.ts) over fakes: the profile list from the files, the MIDI path of each target, the fit
// through dkprof.
import assert from 'node:assert/strict';
import { test } from 'vitest';
import { DkError, type DkSession } from '@digi/dk-session';
import type { DkProf } from '@digi/dk-session/dkprof';
import { readCaptureSet } from '@digi/workbench-core/profile/calibration-view.ts';
import { createCalibrator, type CalibrationDeps } from '../src/calibration-backend.ts';

const texts = () => ({
  profiles: {
    'heavy.toml': `schema = "dk.calprofile/1"\nname = "heavy"\ndescription = "d"\nwarmup_s = 0.1\ncapture_s = 0.1\nrepeats = 2\n[[case]]\nid = "stock"\nsteps = "hold"\n`,
  },
  steps: { 'hold.toml': `schema = "dk.steps/1"\nname = "hold"\n[[step]]\naction = "midi.note"\ntrack = 1\nkey = 60\n` },
  cases: {},
});

function deps(extra: Partial<CalibrationDeps> = {}) {
  const sent: string[] = [];
  const session = {
    clock: { now: async () => 0, pause: async () => {} },
    info: async () => ({ port: 'dn2-1.12', runtime: '1', fw_id: 'ab', log_sink: true }),
    modules: async () => [],
    logPage: async () => {
      throw new Error('no log');
    },
    logConfig: async () => {
      throw new Error('no log');
    },
    profileState: async () => ({}),
    profileAll: async () => ({ hz: 1, elapsedTicks: 0n, spans: [] }),
    // a debug module without PROFILE_DSP: spans only
    profileDsp: async () => {
      throw new DkError({ error: 'unknown command', kind: 'device', status: 'E_PROTO' });
    },
  } as unknown as DkSession;
  const d: CalibrationDeps = {
    session: async () => session,
    loadLibrary: async () => [],
    prof: async () => ({ calibrateFit: (req: unknown) => ({ echoed: req }) }) as unknown as DkProf,
    texts,
    ...extra,
  };
  return { d, sent };
}

test('the profiles come from the files, with their length', () => {
  const [p] = createCalibrator(deps().d).profiles();
  assert.deepEqual([p!.name, p!.description, Math.round(p!.seconds * 10)], ['heavy', 'd', 4]);
});

test('an emulator target plays through the controller (midi.usb.in); a device target through the host MIDI port', async () => {
  const calls: string[] = [];
  const rpc = {
    call: async (method: string, params?: Record<string, unknown>) => {
      calls.push(`${method} ${String(params?.bytes ?? '')}`);
      return { queued: method === 'midi.usb.in' && params?.bytes ? 3 : 0 };
    },
  };
  const emu = await createCalibrator(deps({ rpc: rpc as never }).d).run({ profile: 'heavy' });
  assert.equal(emu.error, undefined);
  assert.ok(calls.some((c) => c.startsWith('midi.usb.in 903c')));
  const sent: number[][] = [];
  const dev = await createCalibrator(deps({ midi: { send: async (b) => void sent.push(Array.from(b)) } }).d).run({
    profile: 'heavy',
    onEvent: () => {},
  });
  assert.equal(dev.emulator, false);
  assert.deepEqual(sent[0], [0x90, 60, 100]);
});

test('without a target MIDI path a run says so', async () => {
  await assert.rejects(createCalibrator(deps().d).run({ profile: 'heavy' }), /no MIDI port/);
});

test('the fit goes through dkprof calibrateFit with the device run as device and the emulator run as model', async () => {
  const doc = (unit: string) => ({ schema: 'dk.profile/1', run: { id: `${unit}-1`, case_id: 'bd' }, dsp: { unit } });
  const emu = readCaptureSet([{ name: 'e.json', text: JSON.stringify(doc('insn')) }]);
  const dev = readCaptureSet([{ name: 'd.json', text: JSON.stringify(doc('cycles')) }]);
  const out = (await createCalibrator(deps().d).fit(emu, dev)) as unknown as {
    echoed: { device: { dsp: { unit: string } }[]; model: { dsp: { unit: string } }[] };
  };
  assert.equal(out.echoed.device[0]!.dsp.unit, 'cycles');
  assert.equal(out.echoed.model[0]!.dsp.unit, 'insn');
});
