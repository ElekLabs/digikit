# 20. Patch toolchains (patchkit spike 0.5)

Goal: compile patchkit modules ahead of time into relocatable objects, link them into the firmware at patch
time with our own code, and unit-test them on the emulated cores. Plan: `docs/plans/patchkit.md`, item 0.5.
Tools: `tools/patchkit/pk_toolchains.py`, `pk_elf.py`, `pk_link.py`, `pk_cfcall.py`, `pk_sharc.py`; tests
`tests/patchkit/test_pk_cf.py`, `tests/patchkit/test_pk_sharc.py` (sources in `tests/pk_src/`). Both tool chains install under
`out/tools/` (gitignored; `mise run toolchains`); the tests skip when a toolchain or the 1.17 image is absent.

## ColdFire (MCF5441x, ISA_C + EMAC, no FPU)

- **Toolchain [D].** GNU binutils 2.47 (tarball snapshot `2.47.20260726`, sha256 `154ab23b...`) and GCC 16.2.0
  (sha256 `e6738e29...`) built from source for `m68k-elf` by `tools/patchkit/pk_toolchains.py m68k`: about 15 minutes on
  16 cores (binutils 3, gcc 11; the two tarballs are 135 MB), C only, no libc, no multilib, prefix `out/tools/m68k-elf`.
  Homebrew also has bottles of both versions (`m68k-elf-gcc` 16.2.0, `m68k-elf-binutils` 2.47); the source
  script is the pinned, local-only route. `as` and GCC know `-mcpu=54418` directly (GCC: `isa_c`, hardware
  divide, USP, EMAC, MMU, no FPU); `as -mcpu=54418` accepts `ff1`, `bitrev`, `byterev`, `mvz`, `mac`, `muls.l`
  and encodes them as the firmware does (`ff1 d0` = `04c0`). LLVM was not tried: Apple clang has no M68k
  target and the LLVM M68k backend names only 680x0 CPUs (from its documentation; **[O]**, not checked here).
- **Flags.** `-mcpu=54418 -Os -ffreestanding -fno-builtin -nostdlib -fno-pic -ffunction-sections
  -fdata-sections -fno-asynchronous-unwind-tables -Wall -Wextra -Werror`.
- **Object prototype [D].** `pk_link.object_from_elf` turns the ELF into JSON: allocatable sections as hex,
  symbols, relocations, SHA-256 of the sources, toolchain versions. `pk_link.link` works from that JSON alone
  (round trip tested), so patch time needs no ELF parser and no toolchain.
- **Relocations seen [D].** gcc emits only `R_68K_32` (absolute `jsr`, `lea`, data addresses) for C at this
  setting; the linker also handles and tests `R_68K_PC32`/`PC16`/`PC8`, `PLT32/16/8` (as `bsr.l` / `bsr.w`
  to an extern). A `bsr.w` from the expansion region to the firmware does not fit (132 MB apart): the linker
  reports the out-of-range relocation instead of truncating.
- **ISA check [D].** `pk_link.check_isa` decodes every instruction of the executable sections with our decoder
  (`packages/emulator/coldfire` `cfdis`, generated from `tools/coldfire/cfisa/coldfire.json`) and fails on an illegal word, a form
  outside `isa_a`/`isa_b`/`isa_c`/`emac`, or a privileged form. The test module (55 instructions, 22 forms)
  passes; an assembly file built with `-m68020` (`bfextu`) is rejected.
- **Running on the core [D].** `Emulator::call_function` (`packages/emulator/boot/src/runtime/call.rs`, example
  `cf_call`, wrapper `CfMachine`): boots a ready image once (7 s CPU, DT2 1.17), caches the `save_state` blob
  (73 MB) in `out/cache/pk_cfcall/`, afterwards 0.75 s per test process. A call pushes the arguments and a
  return sentinel, runs with SR 0x2700, stops when the PC reaches the sentinel, and reports D0-D7/A0-A7; the
  wrapper flags a clobbered callee-saved register (D2-D7, A2-A6) or a wrong A7. Code outside the MAIN image
  (the expansion region) runs through the bare CPU step; the boot observers apply only inside the image.
  Results against Python references: Adler-32 over 1000 bytes equals `zlib.adler32`; `.data`/`.bss`
  state persists between calls; a module function calls firmware 0x40114a28 and 0x40114984 through extern
  symbols and equals direct calls of both; a `bsr.l` (PC32) call into the firmware works; the ISA_C stub
  (`byterev`, `bitrev`, `ff1`) equals its Python model.
- **Firmware ABI notes [D].** 0x40114a28 (`this+0x10` bit test) sets only the low byte of D0 (`clr.b`): declare
  it `u8` or the caller sees garbage in the upper bytes. 0x40041d24 takes `this` on the stack and returns
  `this+0xf4`.
- **Open [O].** (1) `long long` multiply/divide and every float operation compile to libgcc helpers
  (`__muldi3`, `__udivdi3`, `__addsf3`); the libgcc built here is for the default CPU, not 54418, so modules
  must either avoid them or get helpers from the firmware's softfloat entries or a libgcc built with
  `--with-cpu=54418`. (2) Tested on DT2 1.17 only. (3) EMAC is not emitted by gcc; use inline assembly.

## SHARC+ (ADSP-2156x)

- **Toolchain [D].** Selache at `2b26d3b7` (GPLv3, public), `cargo build --release`, built by
  `tools/patchkit/pk_toolchains.py selache` into `out/tools/selache` in under a minute. `selas -proc ADSP-21569`.
- **Object facts [D].** Section `seg_pmco` (assembly, 48-bit forms) or `seg_swco` (selcc). Symbol values and
  relocation offsets are in 16-bit parcels (the PC unit). Two relocation types: **11** = absolute 24-bit target
  of a 48-bit jump/call (low 24 bits of the instruction), **12** = absolute 32-bit immediate or address (low 32
  bits). Targets are short-word (PC) addresses; relative `DO ... UNTIL` and `CALL (PC,n)` are resolved by the
  assembler. Quirks: the addend of `sym + 4` is part of the symbol name, and the field already holds the addend
  (selcc writes `-1` for a return label: `jump (m14,i12)` adds 1). `pk_sharc.link` accepts a field of 0 or the
  addend and fails otherwise. Locals (`.L_...`) appear as symbols.
- **Re-decode gate [D].** After linking, parcels are swapped to boot order and every instruction is decoded by
  `tools/sharc_dev/sharc_isa.py`: the form must exist, its extent must equal the extent Selache chose, and each relocated
  field must read back as the intended value. A corrupted word fails the build (tested). Known Selache encode
  bug (`LSHIFT ... BY -n`, finding 05) is refused at assembly time. Selache's *disassembler* prints `m4` for
  `dm(i0,m0)` stores (finding 05 bug 2 class); the bytes are right, our decoder reads M0.
- **Running on the core [D].** `pk_sharc.run_call` builds a `LoadedMemory` from the linked bytes (at the short
  word alias `0x28000000 + 2*sw`), pokes normal-word DM, runs `tools/sharc_dev/sharc_run.py` to a breakpoint. Scale-in-place
  (`F2 = F2*F1` in a `DO` loop, local `CALL`, tail `JUMP` to an extern) over four floats gives exactly
  `x * 2.5`; one word past the buffer is untouched.

## selcc (Selache C compiler) [D]

Usable for small code, with caveats. Six test functions (`tests/patchkit/pk_src/scale_c.c`) compile, every instruction (49 and 123 in two
sample files) decodes with the extent Selache chose, and they run correctly on the core: `add3`, `sum` over an `int` array, a struct
float field, a `switch`, a bit-count loop, a float scale loop. Calling convention: ADI-style frames
(`cjump`/`rframe`; frame pointer I6, return address at `[I6-1]`), arguments R4/R8/R12, result R0, symbols get a
trailing dot (`scale.`). Findings: (1) **pointers are byte addresses** (index scaled by 4); the loops are right
only with `0x2xxxxxxx` byte-alias pointers, not with normal-word addresses (`0x08xxxxxx`), and our core accepts
a byte address for a normal-word access, so whether silicon agrees is **[O]**; (2) integer divide and modulo call
`___div32.`/`___mod32.` (no library is linked); (3) `-O`, `-Oa`, `-Os` produce identical output (no optimiser to
speak of: the scale loop is 24 instructions against 7 by hand); (4) patchkit would have to supply the frame for a
top-level call (the tests build it). Recommendation: assembly for DSP code on the hot path, selcc only for glue
and only after (1) is settled on hardware or by the PRM.

## Recommendation for the `.dko` format

The JSON prototypes (`pk_link.object_from_elf`, `pk_sharc.SharcObject.to_json`) carry everything the linker
needed, so `.dko` can be that model with a binary container and a canonical encoding:

- header: arch (`cf-mcf54418`, `sharc+`), format version, source SHA-256, toolchain strings (informational),
  required port symbols with an optional signature hash;
- sections: name, kind (`text|rodata|data|bss`), alignment, size, bytes. Store SHARC code in the order the
  boot stream uses (little-endian parcels) or declare the order in the header; define SHARC relocations by
  logical field (`abs24` = bits 23:0 of the 48-bit instruction, `abs32` = bits 31:0) and let the linker pick
  byte positions;
- symbols: name, section, value, global flag, in the arch's native unit (bytes; SHARC parcels);
- relocations: `{in, offset, type, to_symbol | to_section, addend}` with the types seen here (`cf.32`, `cf.pc32`,
  `cf.pc16`, `cf.pc8`, `sharc.abs24`, `sharc.abs32`); the addend always explicit, never in the symbol name;
- the linker is arch code of about 150 lines each, runs in Python today and ports to Rust/wasm directly; the
  ISA check (cfdis, SHARC re-decode) belongs in the module build, not at patch time.


## Assembler facts found writing core/dsp-voice and machines/pluck (patchkit 4.2) **[D]**

One agent; each item was met as a build failure or a wrong result and fixed in the source.

- **Compact `ADD` collides with Type 2b in our decoder.** Selache emits `Rn = Rn + Rm` with `Rn`, `Rm` in R0-R7 as the
  16-bit form `c0xy` (`R0 = R0 + R1` = `c001`); our decoder reads `c001 xxxx` as the 32-bit Type 2b, so the
  extents disagree and `pk_sharc` stops with "Selache lists c001, our decoder c001703e". Registers R8-R15 and the
  `c1xx` subtract form are fine. Commute the operands (`R0 = R1 + R0` assembles as the 32-bit form). Which side is
  right on silicon is **[O]**; the firmware has no `c00x` word.
- **`.VAR name[N]` is dropped and a multi-value `.VAR` produces a section-relative relocation.** `pk_sharc` takes
  `.PKDATA name, bytes;` (zeros, loaded) and `.PKBSS name, bytes;` (reserved, not loaded) instead: the line is removed
  before assembly, `name` becomes an extern, and the object gets a data or bss section and a global data symbol.
  `.GLOBAL name;` for such a name makes the symbol exported twice.
- **patchkit leaves a SHARC bss out of the boot stream** (it did copy it as zeros): the chunk image ends where the
  bss begins, and the plan counts the bss against the region. A 262,144-byte delay area adds 0 bytes to the
  container. The program must write reserved memory before it reads it.
- **`(SW)`/`(BW)` are ignored on indirect loads too** (`R1 = DM(I4, M5) (SW);` assembles as the normal-word load);
  only the direct form is rewritten by `pk_sharc`. Read a short word as a 32-bit word and extract it.
- **`FEXT/FDEP Rx BY Ry`: `Ry` = `bit6 | len6 << 6`** (the PRM figure: `len6` at Ry[11:6]); the immediate form is
  `BY pos:len`. `(SE)` is not parsed by this Selache (`unresolved symbol SE`): shift left and `ASHIFT` right by a
  register holding -16 instead.
- **Immediate arithmetic.** `Rn = Rx + 1` assembles; `Rn = Rx + 26` and `Rn = Rx - 64` do not (load the constant into
  a register). `INC R5` is read as a symbol. `MODIFY(I7, -26) (NW);` assembles to the firmware's own
  `1587 ffff ffe6`; without `(NW)` the immediate is in bytes (`1607 ...`).
- **`CALL (M13, I12)` is not followed by the tracer** (`tools/sharc_dev/sharc_core` ends the callee's `RTS` with "return
  without followed call") and the firmware has no indirect call. core/dsp-voice enters a registered routine with
  `JUMP (M13, I12)` and a return address in `I13`.

## Selache's 48-bit indirect jump (Type 9a) is mis-encoded (2026-10-17) **[D]**

One agent, met writing test/inline-tone (the H1 probe of core/dsp-inline); not checked on silicon.

- **What Selache emits.** Under `.NOCOMPRESS`, `JUMP (Md, Ic)` / `CALL (Md, Ic)` assemble to the 48-bit Type 9a with the
  delayed-branch flag in bit 32 and the I register in bits 26:24: `JUMP (M14, I12)` = `083e34000000`,
  `JUMP (M14, I12) (DB)` = `083f34000000`, `JUMP (M13, I12)` = `083e2c000000`, `JUMP (M8, I9)` = `083e01000000`.
- **What the PRM layout says** (tools/sharc_dev/sharc_visa_tables.py, `9a_abs`): bits 32:30 are PMI, 29:27 PMM, 26 J (the
  delayed flag), 25 E, 24 CI. That is the 32-bit Type 9b layout shifted up by 16 bits, and for Type 9b Selache and our
  decoder agree (`JUMP (M13, I12)` = `083f283f`, `JUMP (M13, I13)` = `083f683f`).
- **The stock code reads the PRM way.** Every confident aligned 9a word in the 1.17 blob (57) has bit 32 = 1 and bits
  31:30 = 0, which is PMI = 4 (I12, the return-address register), with J and E varying (50 delayed, 7 not; the
  conditional ones with E = 1 carry the else-compute the mnemonic shows). Read Selache's way, the same words would be
  jumps through I8, I10 and I12 that are all delayed. Only `JUMP (Mx, I12) (DB)` has the same bits both ways.
- **Effect.** Selache's `JUMP (M13, I12)` under `.NOCOMPRESS` is, by the PRM layout, `JUMP (M13, I8) (DB)`. The concrete
  core runs it that way: the two following instructions execute, then the PC goes to I8 + M13. On the unit this would
  jump into whatever I8 holds.
- **The encoding gate does not catch it.** It masks bit 32 as part of PMI, so `083e2c000000` has the shape of the stock
  `JUMP (M14, I12) (DB)` (`9a_abs:80004000000`) and passes. Selache's form with I13 sets bit 24 (CI) and fails.
- **Rule.** Write indirect jumps outside `.NOCOMPRESS`: Selache then picks the 32-bit Type 9b, which the stock code uses
  (`JUMP (M14, I12)` non-delayed, 64 times) and which our decoder, the core and Selache read alike. core/dsp-inline's
  `JUMP (M13, I12)` and test/inline-tone's return `JUMP (M13, I13)` are 9b. **[O]** A Selache patch, or a
  `pk_sharc` refusal of 48-bit 9a words whose bits disagree with their 9b form.
- **Closed by two checks (Task 5b, 2026-10-17).** Selache is not patched; instead (1) `pk_sharc` cross-checks every
  object it assembles or compiles (tools/patchkit/pk_crosscheck.py): seldump's text and our decoder must name the same
  registers, immediates, condition and DB/CI/LA flags for every instruction, so `.NOCOMPRESS; JUMP (M13, I12);` stops
  the build (`jump (m13,i12)` against our `JUMP (M13, I8) (DB)`) with its source line; and (2) the encoding gate
  matches indirect jumps and calls (Types 9a, 9b) by the whole word against the stock code, so the word `083e2c000000`
  is refused in an image even though its shape is stock. All committed module objects pass the cross-check unchanged.
- **[D] Found by the cross-check (not on silicon).** (a) Selache drops the PM / DAG2 selection of a Type 3a transfer:
  `PM(I12, M14) = R2` assembles to `59be81000000`, which is `DM(I4, M6) = R2`, and `PM(I9, M8) = R2` to `DM(I1, M0) = R2`;
  seldump lists the DM form too, so only comparing with the source statement shows it (pk_crosscheck does: the
  statement must name the registers the word holds). No module uses a PM data transfer. (b) seldump lists a Type 3a
  transfer's M0-M3 as M4-M7, while the encoder writes the register the source names (our decoder, the stock code and
  the concrete core read it that way). (c) seldump lists a 16-bit Type 3c transfer with M0-M3 as a 32-bit word;
  the stock code has such 16-bit words (dmm 1-3, 10 in its instruction chains). (b) and (c) are accepted when the
  source and our decoder agree.

## The stock code never returns through `JUMP (M13, I13)` (2026-10-17) **[D]/[O]**

One agent (Task 5b), from the 1.17 sharcdb (`aligned = 1`, `confidence = 'confident'`) and a byte search of the
section-7 boot stream of firmware/Digitakt_II_OS1.17.syx and of the images in out/patch/.

- **[D] Stock indirect jumps.** The 1.17 DSP code has 1,115 32-bit indirect jumps (Type 9b) in 9 distinct words:
  `083f343f` `JUMP (M14, I12) (DB)` (the return, 1,001), `083f303f` `JUMP (M14, I12)` (64), `083f2c3f`
  `JUMP (M13, I12) (DB)` (28), `083f283f` `JUMP (M13, I12)` (12; 15 byte matches in the stream), `083f6c3f`
  `JUMP (M13, I13) (DB)` (5), `083f743f` `JUMP (M14, I13) (DB)` (1), and three conditional I12 forms. All 57 48-bit
  ones (Type 9a) go through I12 as well. The non-delayed `JUMP (M13, I13)`, `083f683f`, occurs nowhere: not in the
  database (aligned or not) and not as a byte pattern in the stream.
- **[D] The images built so far.** Every image in out/patch/ with a custom DSP routine for core/dsp-voice adds
  `083f683f` (PLUCK `pluck-1.17` and the diagnostics E, F, G, H, I: 2 or 3 each) and one `083f283f` (the entry); the
  pre-5b H1 images add one of each. The lab-style images (diag-J..R xslice-labsine, diag-xslice, diag-D) add neither.
- **[O] Correlation, not cause.** Per the owner's tests as relayed by the controller, every custom routine that stalled
  the unit (PLUCK, F, H) returned with `083f683f`, and the lab-style images that worked used no indirect jump. Those
  stalled images were also whole-renderer replacements (core/dsp-voice) and also contain the `083f283f` entry jump,
  which the new H1 keeps, so the stall is not pinned to `083f683f`. Nothing executed `083f683f` on silicon in an image
  that passed.
- **Rule (since Task 5b).** The encoding gate requires the whole word of every indirect jump or call to occur in stock
  code (or in encoding_proven.toml as `word = "..."`), so `083f683f` is refused. core/dsp-inline routines return with a
  direct jump (Type 8a, as in the images that worked) to `_di_return_work` or `_di_return_out`; the hook enters them
  with the stock `083f283f`. The pre-5b H1 images are kept as `out/patch/diag-h1-i13return-{96k,48k}-1.17.ungated.syx`
  (optional diagnostic: they fail the new gate on exactly that word).

## selcc for a real engine: the PLAITS VA voice (2026-10-17) **[D]**

Measured building machines/plaits-48k/-96k (the Plaits VA engine, the voice wrapper and the half-band filter in C,
merged with the assembly entry shim by `pk_build`, tools/patchkit/pk_sharc.py `compile_c` and `merge`):

- **Shapes.** Over the whole object (3,382 instructions, 6,720 parcels) the encoding gate finds one shape absent
  from stock: selcc's `Fn = FLOAT Rm` with n != m, which selas compresses to a 32-bit 2a_short word (op 0xca; stock
  has FLOAT only as 2c `F0 = FLOAT R0`, 48-bit 2a and in multifunction forms). The C avoids it: int-to-float goes
  through `itof_nonneg` (modules/machines/plaits/src/dsp/dsp_util.h; `0x4B000000 | i` reinterpreted, minus 2^23, exact
  for 0 <= i < 2^23). Everything else, including the 9b return `JUMP (M14, I12) (DB)` (an exact stock word), passes.
- **Cross-check.** The only form selcc emits that the operand cross-check did not handle is Type 16a,
  `DM(I7, M7) = label - 1`, the return address of every call (the stock call sequence has the same word, e.g. SW
  0x1c4b30 `9fc0001c4b32`). 16a and 16b are now compared role by role (memory, I, M, immediate; width bits fail
  closed) against seldump's line (tools/patchkit/pk_crosscheck.py `STRICT_FORMS`). seldump lists some stock 16a/16b
  words with another M register or as a different form (stock `99840000`, our decoder `DM(I4, M6) = 0`, seldump
  `dm (i4,m4)=0x0`); those are refused, which costs nothing as selcc emits only I7/M7.
- **A data bug: negative float initialisers.** In an initialised table (`.VAR`), selcc writes a negative float as
  `0x80000000 | -bits(x)` (the two's-complement negation of the IEEE bits, with the sign bit set): `-1.0f` becomes
  `0xC0800000` (-4.0), `-5.258025909e-06f` becomes `0xC94F91E7` (-850206.4). The same constants in code are right
  (`R1 = -0x494F91E7`, i.e. `0xB6B06E19`), and negative integers in tables are right (`-1` is `0xFFFFFFFF`). The
  half-band filter's coefficient table hit it (its even phase was garbage on the core); the table is now IEEE bit
  patterns in an `unsigned int` array read through a union (modules/machines/plaits/src/dsp/gen_halfband.py). Neither
  the gate nor the cross-check can see data; the concrete-core equivalence tests did.
- **Pointers.** selcc's byte-address pointers are the firmware's own convention on the Digitakt: the voice records
  (0x2412cc), the voice outputs (0x252df8) and the patch region in L2 (0x2001xxxx) are byte addresses, and normal-word
  accesses through them scale the modifier by 4 (the stock renderer's frames, core/dsp-inline's table walk). The C
  stack lives in L2 (`_pl_stack`), so `DM(-n, I6)` frames and `&local` (`I6 = MODIFY(I6, -n) (NW)`) are byte
  addresses too. This settles the open question of the selcc section for this use; silicon evidence comes with the
  owner's H2 run.
- **Division** compiles to `RECIPS` plus two Newton steps and a correction; the concrete core needs
  `approx_recips=True` for it (an 8-bit seed model, tools/sharc_dev/sharc_core/floats.py), otherwise every quotient is
  unknown. With it, the compiled voice equals the host build of the same C within 1.8e-7 per sample.
- **Cost.** No optimiser: one even voice's block is about 60,400 instructions at 48 kHz direct and 84,000 with the
  half-band (22 taps x 32 samples in naive code); the odd voice's copy about 200. Against the DT2's 666,667-instruction
  block and its 208,600 busy (finding 06, "Headroom"), one PLAITS track is about 9 % (13 % with the half-band) before
  cache and memory stalls; 16 tracks do not fit without hand-written hot loops.
