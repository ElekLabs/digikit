// The active calibration of the Profile pane: a dk.calibration/1 file the user loaded for a device, kept per device in the app's KeyValue, and the
// estimate it gives for the emulator's DSP rows (instructions per block times the budget factor, as dkprof's `calibrate report` does per row).
// Pure rules and one small store; no module-level state (the app object gives the KeyValue). Host tests: tests/calibration-active.test.ts.
import { createSignal } from 'solid-js';
import type { KeyValue } from '../storage';
import { fmtFactor, type CalibrationDoc } from './calibration-view';
import type { DspView } from './profile-core';

const CALIBRATION_SCHEMA = 'dk.calibration/1';

/** What is kept for a device: the file's name and its content. */
export interface ActiveCalibration {
  name: string;
  doc: CalibrationDoc;
}

const KEY = (device: string) => `digikit.calibration.${device}`;

/** Read a calibration file for `device` (a device id), or the reason it cannot be used. */
export function checkCalibration(
  text: string,
  device: string | undefined,
): { doc: CalibrationDoc; error?: undefined } | { doc?: undefined; error: string } {
  let doc: CalibrationDoc;
  try {
    doc = JSON.parse(text) as CalibrationDoc;
  } catch {
    return { error: 'not a JSON file' };
  }
  if (!doc || typeof doc !== 'object' || doc.schema !== CALIBRATION_SCHEMA)
    return { error: `not a ${CALIBRATION_SCHEMA} file` };
  const f = doc.dsp?.factor;
  if (!f || typeof f.fit !== 'number' || typeof f.budget !== 'number' || !doc.dsp?.error)
    return { error: 'the file has no DSP fit (nothing to estimate with)' };
  if (typeof doc.key?.device !== 'string') return { error: 'the file names no device' };
  if (!device) return { error: 'the target device is not known yet: read the target first' };
  if (doc.key.device !== device)
    return { error: `the file is a calibration of ${doc.key.device}, the target is ${device}` };
  if (doc.key.device_unit === 'insn')
    return { error: 'the file pairs the emulator with itself (device_unit insn): it is no device calibration' };
  return { doc };
}

/** Per-device calibrations in a KeyValue, reactive: `get` re-runs its readers after `set` or `clear`. */
export function createActiveCalibrations(storage: KeyValue) {
  const [version, setVersion] = createSignal(0);
  const read = (device: string): ActiveCalibration | undefined => {
    try {
      const raw = storage.get(KEY(device));
      if (!raw) return undefined;
      const v = JSON.parse(raw) as { name?: unknown; doc?: unknown };
      if (typeof v.name !== 'string' || !v.doc) return undefined;
      // a stored file is checked again: a changed rule must not keep an unusable one
      const c = checkCalibration(JSON.stringify(v.doc), device);
      return c.doc ? { name: v.name, doc: c.doc } : undefined;
    } catch {
      return undefined;
    }
  };
  return {
    get(device: string | undefined): ActiveCalibration | undefined {
      version();
      return device ? read(device) : undefined;
    },
    /** Check `text` for `device` and keep it; returns the reason when refused. */
    load(name: string, text: string, device: string | undefined): string | undefined {
      const c = checkCalibration(text, device);
      if (c.error) return `${name}: ${c.error}`;
      storage.set(KEY(device!), JSON.stringify({ name, doc: c.doc }));
      setVersion((n) => n + 1);
      return undefined;
    },
    clear(device: string | undefined): void {
      if (device) storage.remove(KEY(device));
      setVersion((n) => n + 1);
    },
  };
}

export type ActiveCalibrations = ReturnType<typeof createActiveCalibrations>;

export interface EstimateRow {
  name: string;
  insn: number;
  /** insn times the budget factor: never below a measured pair of the calibration. */
  cycles: number;
  /** The share of the device's measured DSP block; undefined when the file has no block length. */
  share?: number;
}

export interface Estimate {
  rows: EstimateRow[];
  total: EstimateRow;
  /** `fit 1.31`: what the fit factor alone would give is `fitCycles` of the total. */
  fitCycles: number;
  /** The worst underprediction of the fit factor, a fraction. */
  worstUnder: number;
  blockCycles?: number;
}

/** The modules' DSP instructions as device cycles. Undefined when the calibration has no DSP fit. */
export function estimateDsp(view: DspView, cal: CalibrationDoc): Estimate | undefined {
  const d = cal.dsp;
  if (!d) return undefined;
  const block = cal.key?.block_cycles && cal.key.block_cycles > 0 ? cal.key.block_cycles : undefined;
  const row = (name: string, insn: number): EstimateRow => {
    const cycles = insn * d.factor.budget;
    return { name, insn, cycles, ...(block ? { share: cycles / block } : {}) };
  };
  return {
    rows: view.modules.map((m) => row(m.name, m.insnPerBlock)),
    total: row('total', view.total),
    fitCycles: view.total * d.factor.fit,
    worstUnder: d.error.worst_under,
    ...(block ? { blockCycles: block } : {}),
  };
}

/** `calibrated: <name> <version>, fit 1.31 ±1.3 %` */
export function calibrationChip(deviceName: string, cal: CalibrationDoc): string {
  const d = cal.dsp;
  return `calibrated: ${deviceName}, fit ${fmtFactor(d?.factor.fit)} ±${((d?.error.worst_under ?? 0) * 100).toFixed(1)} %`;
}

export const fmtCycles = (n: number): string => Math.round(n).toLocaleString('en-US');
export const fmtBlockShare = (s: number | undefined): string => (s === undefined ? '–' : `${(s * 100).toFixed(1)} %`);
