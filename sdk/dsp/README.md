# SDK DSP kernel library

Small SHARC+ kernels for module authors. A module includes the file it needs with `#include "dsp/<name>.inc"`; there is
no shared copy at run time, the text is assembled into each module. The module build finds `sdk/` in its include path.

Two forms:

- Step macros (this directory, now): one sample of a kernel, for a module's own block loop.
- Block routines (`call.inc`, `biquad.inc`): whole-block routines on the stock C convention.

Style: [docs/sharc/style.md](../../docs/sharc/style.md), in particular section 5a (rules from the manuals).

## Register rule for macros

Every macro names all registers it uses as arguments: inputs, outputs, temporaries and constants. The caller allocates
the registers and can see which ones the macro writes. A macro touches no other register, flag-dependent state or pointer.
Constants (0.5, 1.5, 2.0, 1/32) are passed in registers that the caller loads before the loop. The caller writes the
`;` after a macro call. A macro is several instructions separated by `;`, so a loop-end label must never be on the
first line of a macro expansion: put the label on a trailing instruction (usually the store).

The exact oscillator/noise extractions below retain inline fixed constants where the existing machine schedule
loads them. Their destination registers are explicit arguments; they do not hide any register use. Integer and
floating-point register names alias the same storage: pass the matching R aliases required by the sine macro.

Rules that apply to the loop around a macro (style.md 5a):

- A block loop has 4 or more instructions.
- Do not use a DAG register within 4 instructions of loading it.
- A multiplier input that comes from the previous instruction costs stalls (2 if both inputs, 1 if one).
- Keep data in DM.
- The stall costs are not measured in the emulator; it counts instructions.

## Macros

| File | Macro | Instructions per sample | Notes |
| --- | --- | --- | --- |
| `gain.inc` | `DK_GAIN_STEP(OUT, IN1, GN)` | 1 | y = x g |
| `gain.inc` | `DK_MIX_STEP(OUT, IA, IB, MIXV, TMP)` | 3 | y = a + m (b - a) |
| `ramp.inc` | `DK_RAMP_SETUP(STEP, CUR, TGT, INVN)` | 2 per block | step = (target - cur) / 32 |
| `ramp.inc` | `DK_RAMP_STEP(CUR, STEP)` | 1 | cur = cur + step |
| `onepole.inc` | `DK_ONEPOLE_STEP(OUT, IN1, COEF, TMP)` | 3 | y = y + a (x - y) |
| `sat.inc` | `DK_SAT_STEP(OUT, IN1, ONE, HALF, K15, TMP)` | 5 | cubic soft clip, \|y\| <= 1 |
| `recip.inc` | `DK_RECIP(OUT, IN1, TWO, TMP)` | 7 | 1 / x, 2 Newton steps |
| `recip.inc` | `DK_DIV(QUO, NUM, DEN, TWO, TMP, TMP2)` | 8 | n / d |
| `recip.inc` | `DK_RSQRT(OUT, IN1, HALF, K15, TMP, TMP2)` | 16 | 1 / sqrt(x), 3 Newton steps |
| `noise.inc` | `DK_XORSHIFT32_STEP(STATE, TMP, RIGHT)` | 6 | unsigned xorshift32; RIGHT holds -17 |
| `noise.inc` | `DK_XORSHIFT32_LOAD_STEP(STATE, TMP)` | 7 | same recurrence, retains BD's inline shift load |
| `oscillator.inc` | `DK_SINE_POLY_STEP(OUT, X, SQUARE, COEF, OUT_BITS, COEF_BITS)` | 16 | BD/PD polynomial on already folded phase, -0.25 to +0.25 |
| `sample.inc` | `DK_SAMPLE_CURSOR_STEP(INDEX, FRAC, HI, LO, MASK, TMP, CARRY)` | 5 | exact split integer/fraction update for signed Q16.16 rate |

Each file has a header with the maths, the arguments, the instruction count and the accuracy. The float32 reference
models are in `tools/patchkit/dsp_ref.py`; the tests are in `tests/patchkit/test_dsp_kernels.py`.

The extracted noise/sine references and tests are `tools/patchkit/dsp_osc_ref.py` and
`tests/patchkit/test_dsp_osc_kernels.py`. Noise seeds, state storage and float conversion remain caller-owned;
zero is an absorbing state. The sine macro approximates `-sin(2*pi*x)` on its folded input domain, with separate
float32 rounding at each operation; phase wrapping, folding and modulation remain in the machine. These macros
replace the corresponding code in BD, PD and pluck while preserving the assembled instructions and data.

## Speed

Sequential macros do not pack stages into multifunction instructions (a multiply and an add in one instruction, or a
load with a compute). The counts above are the sequential counts. For peak speed a module interleaves the stages by
hand; the block routines (below) do this inside.

## Block routines

A block routine processes one block of `DK_BLOCK` (32) samples and uses the stock / selcc C convention, so assembly modules
and `selcc`-compiled C call it the same way. `call.inc` has the macros and the full contract; the points that matter:

- Arguments R4, R8, R12; no result. Normal-word DM addresses (a C caller passes them as `int`; selcc's byte-address pointers do
  not match). The caller sets up M5 = 0, M6 = 1, M7 = -1, M14 = 1 as the module contract already guarantees.
- A caller assumes R0, R1, R2, R4, R8, R12, I4, I12, MR and ASTAT are clobbered. R3, R5-R7, R9-R11, R13-R15, I0-I3, I5, M1-M3
  and B3 are kept. I6 and I7 are restored by the routine's `RFRAME`.
- Stack: `DK_CALL(fn, return_label)` (`CJUMP` plus two pushes) builds the frame, `DK_ENTER(N)` reserves N save slots at `[I6 - 2]` downward, and
  `DK_LEAVE` returns and runs `RFRAME`.
- Every call supplies a continuation label unique within the assembled object, including repeated calls to the same routine.
  The macro defines that label and pushes its relocated address minus one through an immediate Type16a store.
  The former one-argument form is unsupported; storing the PC UREG produced an invalid manual return frame.
- Cost of a call with S saved registers: 2 S + 7 instructions on top of the work.
- Symbols: `_dk_<name>` for assembly, `dk_<name>.` for selcc C. Include `dsp/call.inc` once, then the routine files.

`DK_CALL` uses the CJUMP ABI; the machine adapter enters a module with JUMP and a different return contract.
A module calling a block routine must preserve its adapter-return registers I12/I13 and restore its exact entry I7.
Entry I7 is an exclusive stack boundary: reserve space below it before constructing the CJUMP frame, so no store
overwrites the adapter's context. The [sample-proof fixture](../../tests/fixtures/modules/sample-proof/README.md)
contains a tested bridge for both adapters; this fixture does not define a general module-call API.

| File | Routine | Instructions per sample | Per call (32 samples, with `DK_CALL`) | Notes |
| --- | --- | --- | --- | --- |
| `biquad.inc` | `_dk_biquad` | 5 | 203 | transposed direct form II; R12 = 7 words b0, b1, b2, a1, a2, s1, s2; in place allowed; reads only the input block |
| `delay.inc` | `_dk_delay` | 4 | 175 | linearly interpolated delay, hardware circular ring; R12 = 4 words ring base, ring length L (>= 34), write offset w, delay d (float, 1 <= d <= L - 2); in place allowed; L1 and L2 are 0 on return |
| `halfband.inc` | `_dk_halfband_down` | 20 per output (2 inputs) | 701 | 2x decimator, 23-tap half-band (Kaiser window), 64 inputs to 32 outputs; R12 = 2 words history ring base (32 words), offset (unchanged by a call); coefficient table 2 c_j in DM (`.SECTION/DM`); L1 is 0 on return |
| `sample.inc` | `_dk_sample_linear` | path-dependent | ceiling 1,380 including three argument loads | bounded mono float32 view; split cursor, clamped linear interpolation, zero tail and completion |
| `sample_pcm16.inc` | `_dk_sample_pcm16_linear` | path-dependent | ceiling 2,148 including three argument loads | packed signed PCM16 view; same cursor, interpolation and completion |

For the older block kernels, consult their headers and `tests/patchkit/test_dsp_kernels.py` for the distinction
between static instruction counts and measured counts. The sample reader's measured evidence is described below.

## Numerical sample reader

`sample.inc` provides numerical playback only: it does not load files, allocate sample memory or define a sample
resource ABI. Include `dsp/call.inc` first. Pass normal-word DM addresses in R4 (source), R8 (32-float output) and
R12 (six integer state words): `start`, exclusive `end`, `index`, `fraction`, signed Q16.16 `rate`, `done`.
Initialize forward playback at `(start, 0)`, reverse at `(end - 1, 0)`, and clear `done`. A zero rate holds position.

The source, state, output and stack must not overlap. The source view is immutable and its entire interval must
be mapped/addressable; see the include for integer range and register preconditions. The reader interpolates
`a + fraction/65536 * (b - a)` with separate float32 rounding and clamps the neighbor to `end - 1`. An empty or
finished interval reads no source and writes silence. Completion is recorded even when it occurs on the final
sample of a block; subsequent calls stay silent until the caller resets the state. Only index, fraction and done
are written back. The cursor uses integer arithmetic, so large frame positions do not lose fractional precision.

The reference and tests are `tools/patchkit/dsp_sample_ref.py` and `tests/patchkit/test_dsp_sample_kernels.py`.
Tests compare against an independent combined-position oracle and watch for out-of-interval reads. The two-call
harness measured 557–2,231 instructions across its cases, below its 2,761-instruction ceiling. These are concrete-core
instruction counts, not hardware cycles or a speedup claim. This first implementation is sequential and has no
anti-alias filter, playback looping or boundary fades. Float32 input is a numerical view, not a decision to store
all transferred PCM as float32. Resource ownership and stereo playback remain separate work.


`sample_pcm16.inc` adds `_dk_sample_pcm16_linear` (`dk_sample_pcm16_linear.` for C).
Include `dsp/call.inc`, then `dsp/sample.inc` for its shared cursor macro, then
`dsp/sample_pcm16.inc`. It uses the same six integer state words and calling
convention. R4 addresses immutable packed normal-word DM: frame `i` is in
`base + floor(i/2)`, with even frames in the low 16 bits and odd frames in the
high 16 bits. Little-endian transport bytes store each frame's low byte first.
The backing storage has `ceil(total_frames/2)` full words; an odd frame count
still requires the final word's unused high half. Odd starts and ends need only
the containing words; the unused halves are ignored.

The selected half is sign-extended and normalized exactly as `int16/32768` in
float32, then interpolated in the same operation order as the float reader.
The neighbor clamps to `end - 1`; no containing word beyond the view is read.
Empty, completed and out-of-bounds cursors read no source and emit silence.
The caller handles resource readiness. The source base is a normal-word address,
without a firmware address, allocation policy or physical-memory mapping in this
contract. The numerical tests exercise packed transport bytes, signed extrema,
odd boundaries, extreme rates, repeated calls, preserved registers and stack
boundaries, guarded source reads and completion immediately after output 31.
The ceiling above counts instructions, not hardware cycles.
