import path from 'node:path';
import { button, emulatorClock, stepWithProgress, waitClock, type Session } from '@digi/test-support';
import { record } from './artifacts.ts';
import { frames, runToFrame } from './dsp-state.ts';
import { bounded, rpc } from './guest-access.ts';
import { pickSine, type Rt } from './panel-waits.ts';

export interface NoteOptions {
  /** WAV file for the PCM capture. */
  file: string;
  /** Absolute DSP frame at which the note starts (after the block boundary). */
  trigger: number;
  /** Names the setup error and the fault check. */
  tag: string;
  /** Frames captured after the trigger. */
  length?: number;
  /** `pcm.tap start` was already sent by the caller. */
  tapStarted?: boolean;
  /** Skip the 44 000-instruction offset after the block boundary. */
  skipStep?: boolean;
  /** Capture without pressing TRIG 1. */
  noPress?: boolean;
  /** Runs once after the key is down, inside the capture. */
  during?: () => Promise<void>;
}

/** Pauses the run, then reads the frame counter: a trigger `lead` frames on is relative to a stopped clock, so host load cannot move the firmware's phase at the key press. */
export async function pausedTrigger(session: Session, lead = 300): Promise<{ current: number; trigger: number }> {
  await rpc(session, 'run.mode', { mode: 'paused' });
  const current = await frames(session);
  return { current, trigger: current + lead };
}

/** The shared note sequence: run to `trigger`, capture `length` frames with TRIG 1 held, then check for faults. */
export async function captureNote(session: Session, opts: NoteOptions): Promise<void> {
  const { file, trigger, tag, length = 1500 } = opts;
  session.requireDsp('captureNote');
  const current = await frames(session);
  if (current >= trigger - 20) throw new Error(`${tag} setup too late: current frame ${current}, trigger ${trigger}`);
  if (!opts.tapStarted) await rpc(session, 'pcm.tap', { action: 'start' });
  try {
    await runToFrame(session, trigger);
    if (!opts.skipStep) await stepWithProgress(session.client, 44_000);
    await rpc(session, 'pcm.tap', { action: 'capture', path: file });
    if (!opts.noPress) await bounded(() => session.client.inputButton(button('TRIG 1'), true));
    await opts.during?.();
    await runToFrame(session, trigger + length);
  } finally {
    try {
      await rpc(session, 'pcm.tap', { action: 'capture_stop' });
    } finally {
      await bounded(() => session.client.inputButton(button('TRIG 1'), false));
    }
  }
  await waitClock(emulatorClock(session), 50); // the last frames of the note reach the capture
  await bounded(() => session.expectNoFault(tag));
}

/** Future-frame choice follows the legacy restarted-note path; boundary offset and 1500-frame hold are unchanged. */
export async function capture(runtime: Rt, file: string, selection?: number, triggerFrame?: number): Promise<number> {
  const { session } = runtime;
  await rpc(session, 'run.mode', { mode: 'paused' });
  await rpc(session, 'pcm.tap', { action: 'start' });
  await pickSine(runtime, selection);
  const { current, trigger } = await pausedTrigger(session);
  const trig = triggerFrame ?? trigger;
  record(path.dirname(file), `${path.basename(file, path.extname(file))}-timing`, {
    currentFrame: current,
    triggerFrame: trig,
  });
  await captureNote(session, { file, trigger: trig, tag: 'FX note', tapStarted: true });
  return trig;
}

/**
 * Play TRIG 1 from DSP frame `trigger` (the same instruction after the block boundary in every run) and capture 1500 frames to `file`. `during`
 * runs once after the key is down, inside the capture. The key stays down for the whole capture. Python `Rig.note`.
 */
export async function playNote(session: Session, file: string, trigger: number, during?: () => Promise<void>) {
  await captureNote(session, { file, trigger, tag: 'note', during });
}
