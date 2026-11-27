// What the Profile and Calibration tabs share (SidePanels.tsx builds one): whether a calibration run is playing (the Profile tab does not read while
// it does) and the calibration kept per device. No module state: the KeyValue is an argument.
import { createSignal } from 'solid-js';
import { createActiveCalibrations } from '@digi/workbench-core/profile/calibration-active.ts';
import type { KeyValue } from '@digi/workbench-core/storage.ts';

export function createCalibrationHub(storage: KeyValue) {
  const [calibrating, setCalibrating] = createSignal(false);
  return { calibrating, setCalibrating, active: createActiveCalibrations(storage) };
}

export type CalibrationHub = ReturnType<typeof createCalibrationHub>;
