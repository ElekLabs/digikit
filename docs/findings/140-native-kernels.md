# Native kernels for the Syntakt peer CPU (first slice)

Slice proof of ahead-of-time translation of hot ColdFire code (plan: `docs/plans/syntakt.md` P4). Image: Syntakt OS 1.42 section 7 (the peer
program, `st-1.42-peer`). Marks: **[D]** measured or read once, **[O]** open (no second-agent check yet, so no **[V]**).

## The kernel

- **[D]** The two hottest 64-byte buckets of the frame handler, 0x400029c0 (10 %) and 0x40002a00 (14 %), are the loop of one function,
  `FUN_4000290e` (0x4000290e..0x40002a62, 340 bytes, 98 instructions, 18 call sites). It is an interpolating oscillator: per voice it fills 32
  output longs. The loop body (0x400029da..0x40002a30) is 27 instructions run 32 times (`moveq #0x20,d0`), the prologue and epilogue about 65.
  Over 300 M MAIN ticks (cold boot, trig 1, PLAY): 60,230 runs (23.6 per frame), 47.3 M instructions, 21 % of the frame handler (221.97 M) and
  13.9 % of all peer instructions.
- Inputs: two stack arguments (voice index, record base); the record at base + index * 128 holds the pointers and counters (fields 0x78..0xcc:
  two data pointers, the wavetable pointer at 0xb0, phase increments, the phase accumulator). Memory reads: the record, the 32 samples of two
  input streams `(A1)+`, `(A2)+` (the EMAC load forms), the wavetable `(A5,D2*4)`. Writes: two output streams `(A3)+` and `(A0)+`, the record fields back,
  the saved registers on the stack (movem).
- EMAC: ACC0 and ACC1 are loaded (`move.l d7,acc1`, `move.l d3,acc0`), MACs run in the signed fractional mode the firmware uses (MACSR 0x20; the kernel
  does not assume it) and both accumulators are drained with `movclr`. CCR at exit: N and Z from the last move, C and V clear, X from the loop's last `subq`.

## Result

- **[D]** Parity. `DIGI_CF_NATIVE=check` runs the kernel and the interpreter from the same state at each of the 60,230 entries and compares registers,
  CCR, EMAC state, instruction count and every bus read and write in order: no difference over 47,293,566 instructions. The WAV of the run (trig 1
  and PLAY, 300 M ticks after the boot, `wav:`) has SHA-256 prefix af5d9dd8cd6c with the interpreter only, with kernels, and in check mode (the reference
  on this branch before the change; the earlier f043c14de9a1 was another run length). `coldfire/tests/native_kernels.rs`: 559 kernel runs from random
  states (random MACSR with all mode bits, accumulators, MASK, holes that refuse access, budgets that end a run anywhere, pointers into the code)
  agree, 304 of them stopped early. A first version that dropped flags a stop point could see failed the check on the first entry (the interpreter
  continues at a block start and needs the CCR exactly); stop points now count as readers.
- **[D]** Kernel speed on flat memory, interpreter and kernel interleaved, best of 7 rounds (host loaded, load average 13): 15.9 against 4.6 ns per
  instruction in one run and 7.5 against 2.1 in another: 3.4x and 3.6x. Command: `cargo test --release -p coldfire --test native_kernels -- --ignored
  --nocapture` with `CF_NATIVE_DIR` set.
- **[D]** Whole run (cold boot to 1053 M ticks, same binary, `DIGI_CF_NATIVE=0` against default, two interleaved pairs, host loaded so wall times
  scatter): host instructions retired (`/usr/bin/time -l`, stable within 0.01 %) 268.72 G against 263.97-264.03 G: 4.7 G fewer, 1.75 %; cycles
  49.3 G against 48.1 G: 2.3 %; peer wall time in the 300 M window 7.23 and 6.51 s against 6.46 and 6.06 s: 10 % less. Peer time is about a third
  of the run; MAIN and the rest two thirds.

## Projection **[O]**

- Ceiling for the peer: its frame handler is 65 % of the peer's instructions; the ten hottest buckets are 64 % of it (41.8 % of peer instructions).
  At 3.4x on all of them, peer time falls by 0.418 * 0.706 = 30 %, the whole run by about 10 % (1.11x).
- MAIN's vector 191 mixer (handler 0x400a38ce, 7550 bytes) is not measured here. MAIN is about two thirds of the run; if the mixer is a third to a
  half of MAIN and gets 3.4x too, the run falls by another 16 to 24 %. Both together: roughly 1.3 to 1.5x. MAIN's remaining code (sequencer, UI,
  peripherals) stays interpreted, so the whole-system factor stays well under the kernel's.
- Risks: each kernel needs its own parity run; self-modifying or device-touching functions need the interpreter (kernels refuse them by stopping);
  generated kernels are one more input of the build cache (`CF_NATIVE_DIR`); the peer bus does a byte fold per read, which probably now limits the native
  loop (its share is not measured **[O]**).
