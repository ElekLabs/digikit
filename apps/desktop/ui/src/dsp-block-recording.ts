import { DkError, type DkSession } from '@digi/dk-session';
import { dspStatsPart, type BuildInput } from '@digi/workbench-core/profile/profile-artifact.ts';

export interface DspBlockCapture {
  unit: 'cycles' | 'instructions';
  blocks: number;
  overruns: number;
  rows: {
    row: number;
    kind: 'machine' | 'effect';
    calls: number;
    meanBlock: number;
    maxBlock: number;
    maxCall: number;
  }[];
}

type Session = Pick<DkSession, 'profileDsp'>;

/** The shared span recorder owns the PROFILE_DSP channel. Stopping spans freezes the latest complete DSP sweep. */
export function createDspBlockRecording() {
  let owner: Session | undefined;
  let generation: number | undefined;
  let startedOk = false;
  let capture: DspBlockCapture | undefined;
  let artifact: BuildInput['dsp'];
  let note = '';

  function reset() {
    startedOk = false;
    owner = undefined;
    generation = undefined;
    capture = undefined;
    artifact = undefined;
    note = '';
  }

  async function start(session: Session, supported: boolean) {
    reset();
    if (!supported) {
      note = 'This target has no SHARC DSP.';
      return;
    }
    try {
      const cleared = await session.profileDsp('clear');
      owner = session;
      generation = cleared.generation;
      const started = await session.profileDsp('start');
      if (started.generation !== cleared.generation || !started.on)
        throw new Error('DSP counters changed while starting the recording.');
      startedOk = true;
    } catch (error) {
      note =
        error instanceof DkError && error.status === 'E_PROTO'
          ? 'The selected target does not support DSP block profiling.'
          : `DSP counters unavailable · ${error instanceof Error ? error.message : String(error)}`;
    }
  }

  async function readSweep(session: Session, emulator: boolean, live: boolean) {
    const reading = await session.profileDsp('get');
    if (reading.generation !== generation || reading.on !== live) throw new Error('The DSP recording changed.');
    return dspStatsPart(reading.sweep, emulator ? 'insn' : 'cycles');
  }

  function view(part: BuildInput['dsp'], emulator: boolean): DspBlockCapture | undefined {
    if (!part || !part.header.blocks) return undefined;
    return {
      unit: emulator ? 'instructions' : 'cycles',
      blocks: part.header.blocks,
      overruns: part.header.overruns,
      rows: part.rows.map((row) => ({
        row: row.row,
        kind: row.kind,
        calls: row.calls,
        meanBlock: row.sum_cycles / part.header.blocks,
        maxBlock: row.max_block,
        maxCall: row.max_call,
      })),
    };
  }

  async function live(session: Session, emulator: boolean) {
    if (!startedOk) return undefined;
    return view(await readSweep(session, emulator, true), emulator);
  }

  async function freeze(session: Session, emulator: boolean) {
    if (!startedOk) return;
    try {
      const part = await readSweep(session, emulator, false);
      capture = view(part, emulator);
      if (capture) artifact = part;
      else note = 'No complete DSP sweep arrived before Stop. Record for longer and try again.';
    } catch (error) {
      note = `DSP capture unavailable · ${error instanceof Error ? error.message : String(error)}`;
    } finally {
      startedOk = false;
      generation = undefined;
      owner = undefined;
    }
  }

  async function clearRunning() {
    if (!owner || !startedOk) return;
    const cleared = await owner.profileDsp('clear');
    generation = cleared.generation;
    if (!cleared.on) {
      await owner.profileDsp('start').then((result) => {
        if (!result.on || result.generation !== generation) throw new Error('DSP counters changed during reset.');
      });
    }
  }

  async function release() {
    if (!owner || generation === undefined) return;
    const current = await owner.profileDsp('get');
    if (current.on && current.generation === generation) await owner.profileDsp('stop');
    reset();
  }

  return {
    start,
    clearRunning,
    live,
    freeze,
    reset,
    release,
    armed: () => startedOk,
    capture: () => capture,
    artifact: () => artifact,
    note: () => note,
  };
}
