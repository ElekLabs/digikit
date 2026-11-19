// P8d, DSP stats window (docs/plans/profiling.md 13.1): the device case, pending until it can run on the emulator.
import { describe, it } from 'vitest';

// The rig cases of P8d are real tests now: modules/core/adapters/dsp-link/tests/dsp-stats.rig.test.ts (window 3, the fold, the pages), and the span
// marks of the services in modules/core/adapters/dsp-voice/tests/dsp-stats.rig.test.ts and modules/core/adapters/fx/tests/dsp-stats.rig.test.ts.

// Device shape (modules/tools/debug, registered in its runtimeAcceptance with the SHARC parts; DT2 and DN2; `dsp: 'sharc'`; one machine and one effect
// module loaded with digikit/debug):
//   1. dk.profileDsp('get'): on false, no sweep.
//   2. dk.profileDsp('start'), play a note on the machine track with the effect on its send, wait in emulated time until `events`/poll sees a sweep.
//   3. dk.profileDsp('get'): sweep present; machine row calls = voices x blocks over the same window, effect row calls = blocks; sumCycles > 0.
//   4. dk.profileDsp('stop'): after 2 s emulated, the sweep number no longer moves; dk.profileDsp('clear') then 'start': rows count from 0.
//   5. With stats off the main output PCM equals the pinned PCM (tests/scenarios).
describe('DSP stats on a device (pending the SHARC parts)', () => {
  it.todo(
    'dsp: sharc, DT2 and DN2: PROFILE_DSP reports calls = voices x blocks for one machine and one effect, cycles > 0, and nothing while off',
  );
});
