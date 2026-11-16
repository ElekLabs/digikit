// P8d, DSP stats window (docs/plans/profiling.md 13.1): the cases the SHARC parts must pass. Pending until `DK_SPAN_BEGIN/END` sit at the mark sites of
// fx.s and dsp-voice-*.s and the dsp-link block tail folds the window. Each todo says what it will assert; turn it into a case with the SHARC parts.
import { describe, it } from 'vitest';

describe('DSP stats on the rig (pending the SHARC parts)', () => {
  // Rig: one machine and one effect module (the conformance set's smallest), the same block run twice on a fresh core per case.
  it.todo(
    'stats off adds exactly 6 instructions per module call: instructions(new image) - instructions(stock image) = 6 * calls, for a machine call and an effect call',
  );
  it.todo(
    'stats on gives calls = voices x blocks for the machine row and calls = blocks for the effect row, with cycles > 0 and largest block >= largest call',
  );
  it.todo('stats on adds at most 300 instructions per block (the fold included), and off adds at most 100');
  it.todo('the PCM of the block is equal with stats on and off (bit exact, every conformance case)');
  it.todo('a clear generation change zeroes every row and the header; the counters then count again from 0');
});

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
