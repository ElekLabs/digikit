import { defineEquiv, DT2_TRIG, pcmPinFile, type Equiv } from '@digi/regression/runtime/equiv.ts';

const SCENARIOS: Equiv[] = [
  {
    // the one full-firmware audio check of the DT2 run-time machine path (onset, level, mixer): onset frame 97, 12000 frames (DT2 1.17, note 60, 261.63 Hz, crest 1.438 over the window with its start-up settling, 1.412 in the second half, harmonics -115 dB, L = R)
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
    // onset frame 168, 12000 frames (DN2 1.12, note 60, 261.63 Hz); exact pin (exact RECIPS output, finding 102): write it with `pin-cli.ts write --exact`
    pcmPin: pcmPinFile(import.meta.dirname, 'equiv-dn2-set'),
  },
];

defineEquiv(SCENARIOS);
