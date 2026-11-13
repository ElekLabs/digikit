import { defineEquiv, DN2_TRIG, type Equiv } from '@digi/regression/runtime/equiv.ts';

// No twin: the rows equal a boot of no module and the note of the run-time machine is pinned by the SHA-256 of its PCM from the onset
// (onset 192 on DT2, 330 on DN2; the pins were captured from the run-time pluck at the point where it played the same note as the retired
// patched-in twin, sample for sample).
const SCENARIOS: Equiv[] = [
  {
    // the note starts shortly after the panel setup ends (frame 3330 in both rigs)
    name: 'equiv-dyn-pluck',
    device: 'dt2',
    modules: ['digikit/pluck'],
    play: { module: 'digikit/pluck', machine: 'pluck' },
    hint: 7,
    trigFrame: 3500,
    voices: 1,
    ignoreRunMachines: ['Pluck'],
    pcmSha256: '3fcbef1546364d5706a466160747d5ea5a1e0c2e1cb00d0bfdcc07963944a722',
  },
  {
    // the run-time machines follow MIDI in the DN2 list
    name: 'equiv-dyn-pluck',
    device: 'dn2',
    modules: ['digikit/pluck'],
    play: { module: 'digikit/pluck', machine: 'pluck' },
    hint: 5,
    trigFrame: DN2_TRIG,
    voices: 1,
    ignoreRunMachines: ['Pluck'],
    pcmSha256: 'b3c390aa83941f294b5e861d5963cfe0bd8e0bf53ea09d356dfd65ed40aa2dd9',
  },
  {
    // Full tier: the note later in the run (audible; the pinned note is the quick cell's)
    name: 'equiv-mixed',
    device: 'dt2',
    full: true,
    modules: ['digikit/pluck'],
    play: { module: 'digikit/pluck', machine: 'pluck' },
    hint: 7,
    trigFrame: 9500,
    voices: 1,
    ignoreRunMachines: ['Pluck'],
  },
];

defineEquiv(SCENARIOS);
