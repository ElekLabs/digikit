import { pcmPinFile } from '@digi/sharc-rig-kit/pins';
import { defineEquiv, DT2_TRIG, type Equiv } from '@digi/regression/runtime/equiv.ts';

const SCENARIOS: Equiv[] = [
  {
    // the one full-firmware audio check of the DT2 run-time machine path (onset, level, mixer): onset 194, 12000 frames (DT2 1.17, note 60, 261.67 Hz, crest 1.420, harmonics -119 dB, L = R)
    name: 'equiv-dt2-sine',
    device: 'dt2',
    modules: ['test/sine'],
    play: { module: 'test/sine', machine: 'sine' },
    trigFrame: DT2_TRIG,
    voices: 1,
    ignoreRunMachines: ['Sine'],
    pcmPin: pcmPinFile(import.meta.dirname, 'equiv-dt2-sine'),
    sine: { note: 60 },
  },
  {
    name: 'equiv-dn2-set',
    device: 'dn2',
    modules: ['test/dyn-dn2', 'test/sine'],
    hint: 6,
    trigFrame: 7000,
    voices: 2,
    ignoreRunMachines: ['DynSyn', 'Sine'],
    // onset 400, 12000 frames (DN2 1.12, note 60, 261.65 Hz); approved by Em 2026-11-01 as a hash (exact RECIPS, finding 102)
    pcmPin: pcmPinFile(import.meta.dirname, 'equiv-dn2-set'),
  },
];

defineEquiv(SCENARIOS);
